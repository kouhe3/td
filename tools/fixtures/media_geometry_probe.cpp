// Oracle for the media-geometry face (td/telegram/Dimensions.{h,cpp} and
// MediaAreaCoordinates.{h,cpp}), driven by a script:
//
//   media_geometry_probe <script.txt>
//   media_geometry_probe --check obj_invalid     get_area_position_object() on an
//                                                invalid object (CHECK)
//   media_geometry_probe --check input_invalid   get_input_media_area_coordinates()
//                                                likewise
//
// Commands:
//   dim <w> <h>                        get_dimensions + pixel count
//   dimcmp <w1> <h1> <w2> <h2>         operator== / !=
//   area <x> <y> <w> <h> <rot> <r>     MediaAreaCoordinates(td_api::areaPosition)
//   bits <x> <y> <w> <h> <rot> <r>     get_input_media_area_coordinates() bit patterns
//   eq <x> <y> <w> <h> <rot> <r> <x2> ...   epsilon comparison
//   tarea <flags> <x> <y> <w> <h> <rot> <r>  MediaAreaCoordinates(telegram_api)
//
// Doubles come in as decimal text or `nan` / `inf` / `-inf`. Every double that
// reaches the object is echoed as its 64-bit pattern first, so the narration
// stays exact even where `operator<<` rounds to six decimals.
//
// Build (both units live in libtdcore.a; the areaPosition JSON printer only
// exists in the `tdjson_private` object library):
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I tdactor -I C:/tmp/td-build/tdutils tools/fixtures/media_geometry_probe.cpp
//   C:/tmp/td-build/CMakeFiles/tdjson_private.dir/td/generate/auto/td/telegram/td_api_json_*.cpp.obj
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tdnet/libtdnet.a
//   C:/tmp/td-build/tdtl/libtdtl.a C:/tmp/td-build/tde2e/libtde2e.a
//   C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdutils/libtdutils.a
//   C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/sqlite/libtdsqlite.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi -lcrypt32 -lsecur32
//   -lmswsock -liphlpapi -ldnsapi -o C:/tmp/td-probe/media_geometry_probe.exe
//
// Measured behaviour this probe encodes (docs/facts.md F239): a single bad
// dimension zeroes the pair, non-finite doubles become 0, rotation is clamped to
// [-360, 360] and then made non-negative, the TL constructor ignores the
// incoming flags, `get_input_media_area_coordinates()` recomputes them from
// `radius > 0`, and `operator==` is a per-field `|delta| < 1e-6`.

#include "td/telegram/Dimensions.h"
#include "td/telegram/MediaAreaCoordinates.h"
#include "td/telegram/td_api.h"
#include "td/telegram/td_api_json.h"
#include "td/telegram/telegram_api.h"

#include "td/utils/JsonBuilder.h"
#include "td/utils/Slice.h"
#include "td/utils/StringBuilder.h"
#include "td/utils/misc.h"

#include <cstdio>
#include <cstring>
#include <fstream>
#include <limits>
#include <string>
#include <vector>

namespace {

using td::Slice;
using td::string;

string read_all(const char *path) {
  std::ifstream in(path, std::ios::binary);
  if (!in) {
    std::fprintf(stderr, "cannot open %s\n", path);
    std::exit(2);
  }
  return string(std::string((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>()));
}

std::vector<Slice> split_tokens(Slice line) {
  std::vector<Slice> tokens;
  while (!line.empty()) {
    while (!line.empty() && line[0] == ' ') {
      line.remove_prefix(1);
    }
    if (line.empty()) {
      break;
    }
    size_t i = 0;
    while (i < line.size() && line[i] != ' ') {
      i++;
    }
    tokens.push_back(line.substr(0, i));
    line.remove_prefix(i);
  }
  return tokens;
}

Slice trim_line(Slice line) {
  while (!line.empty() && (line.back() == '\r' || line.back() == ' ' || line.back() == '\t')) {
    line.remove_suffix(1);
  }
  while (!line.empty() && (line[0] == ' ' || line[0] == '\t')) {
    line.remove_prefix(1);
  }
  return line;
}

double parse_double(Slice token) {
  if (token == "nan") {
    return std::numeric_limits<double>::quiet_NaN();
  }
  if (token == "inf") {
    return std::numeric_limits<double>::infinity();
  }
  if (token == "-inf") {
    return -std::numeric_limits<double>::infinity();
  }
  return td::to_double(token);
}

td::int32 parse_int(Slice token) {
  return td::to_integer<td::int32>(token);
}

unsigned long long bits(double value) {
  unsigned long long raw = 0;
  std::memcpy(&raw, &value, sizeof(raw));
  return raw;
}

td::MediaAreaCoordinates make_coordinates(double x, double y, double w, double h, double rot, double radius) {
  return td::MediaAreaCoordinates(td::td_api::make_object<td::td_api::areaPosition>(x, y, w, h, rot, radius));
}

string json(const td::td_api::object_ptr<td::td_api::areaPosition> &object) {
  if (object == nullptr) {
    return "null";
  }
  return td::json_encode<string>(td::ToJson(*object));
}

void print_coordinates(const td::MediaAreaCoordinates &coordinates, double x, double y, double w, double h, double rot,
                       double radius) {
  td::StringBuilder sb;
  sb << coordinates;
  std::printf("area in=[%016llx %016llx %016llx %016llx %016llx %016llx] valid=%d print=%s\n", bits(x), bits(y),
              bits(w), bits(h), bits(rot), bits(radius), static_cast<int>(coordinates.is_valid()),
              sb.as_cslice().str().c_str());
  if (coordinates.is_valid()) {
    std::printf("  json=%s\n", json(coordinates.get_area_position_object()).c_str());
    auto input = coordinates.get_input_media_area_coordinates();
    std::printf("  input flags=%d bits=[%016llx %016llx %016llx %016llx %016llx %016llx]\n", input->flags_,
                bits(input->x_), bits(input->y_), bits(input->w_), bits(input->h_), bits(input->rotation_),
                bits(input->radius_));
  }
}

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc == 3 && std::strcmp(argv[1], "--check") == 0) {
    Slice check_case(argv[2]);
    auto coordinates = make_coordinates(10.0, 20.0, 0.0, 40.0, 0.0, 0.0);
    if (check_case == "obj_invalid") {
      std::printf("before valid=%d\n", static_cast<int>(coordinates.is_valid()));
      std::fflush(stdout);
      auto object = coordinates.get_area_position_object();
      std::printf("after=%s\n", json(object).c_str());
      std::fflush(stdout);
      return 0;
    }
    if (check_case == "input_invalid") {
      std::printf("before valid=%d\n", static_cast<int>(coordinates.is_valid()));
      std::fflush(stdout);
      auto input = coordinates.get_input_media_area_coordinates();
      std::printf("after flags=%d\n", input->flags_);
      std::fflush(stdout);
      return 0;
    }
    std::fprintf(stderr, "unknown check case\n");
    return 2;
  }

  if (argc != 2) {
    std::fprintf(stderr, "usage: probe <script.txt>\n");
    return 2;
  }

  auto script = read_all(argv[1]);
  for (auto raw_line : td::full_split(Slice(script), '\n')) {
    auto line = trim_line(raw_line);
    if (line.empty() || line[0] == '#') {
      continue;
    }
    auto tokens = split_tokens(line);
    auto command = tokens[0];
    if (command == "dim") {
      auto dimensions = td::get_dimensions(parse_int(tokens[1]), parse_int(tokens[2]), "probe");
      td::StringBuilder sb;
      sb << dimensions;
      std::printf("dim w=%d h=%d px=%u print=%s\n", static_cast<int>(dimensions.width),
                  static_cast<int>(dimensions.height), td::get_dimensions_pixel_count(dimensions),
                  sb.as_cslice().str().c_str());
    } else if (command == "dimcmp") {
      auto lhs = td::get_dimensions(parse_int(tokens[1]), parse_int(tokens[2]), "probe");
      auto rhs = td::get_dimensions(parse_int(tokens[3]), parse_int(tokens[4]), "probe");
      std::printf("dimcmp eq=%d ne=%d\n", static_cast<int>(lhs == rhs), static_cast<int>(lhs != rhs));
    } else if (command == "area") {
      auto x = parse_double(tokens[1]);
      auto y = parse_double(tokens[2]);
      auto w = parse_double(tokens[3]);
      auto h = parse_double(tokens[4]);
      auto rot = parse_double(tokens[5]);
      auto radius = parse_double(tokens[6]);
      print_coordinates(make_coordinates(x, y, w, h, rot, radius), x, y, w, h, rot, radius);
    } else if (command == "tarea") {
      auto flags = parse_int(tokens[1]);
      auto x = parse_double(tokens[2]);
      auto y = parse_double(tokens[3]);
      auto w = parse_double(tokens[4]);
      auto h = parse_double(tokens[5]);
      auto rot = parse_double(tokens[6]);
      auto radius = parse_double(tokens[7]);
      auto tl = td::telegram_api::make_object<td::telegram_api::mediaAreaCoordinates>(flags, x, y, w, h, rot, radius);
      td::MediaAreaCoordinates coordinates(tl);
      print_coordinates(coordinates, x, y, w, h, rot, radius);
    } else if (command == "eq") {
      auto lhs = make_coordinates(parse_double(tokens[1]), parse_double(tokens[2]), parse_double(tokens[3]),
                                 parse_double(tokens[4]), parse_double(tokens[5]), parse_double(tokens[6]));
      auto rhs = make_coordinates(parse_double(tokens[7]), parse_double(tokens[8]), parse_double(tokens[9]),
                                 parse_double(tokens[10]), parse_double(tokens[11]), parse_double(tokens[12]));
      std::printf("eq eq=%d ne=%d\n", static_cast<int>(lhs == rhs), static_cast<int>(lhs != rhs));
    } else {
      std::fprintf(stderr, "unknown command %s\n", command.str().c_str());
      return 2;
    }
  }
  return 0;
}