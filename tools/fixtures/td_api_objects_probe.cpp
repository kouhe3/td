// Oracle for the generated `td_api` object face (td/telegram/td_api.h +
// td_api_json), driven by a script:
//
//   td_api_objects_probe <script.txt>
//   td_api_objects_probe --check badutf     one process, prints "before", then
//                                          the JsonBuilder CHECK fires
//
// Commands (blank lines and '#' comments ignored):
//
//   ulist <first|-> <items|->      build a real td::Usernames and print the JSON
//                                 of its get_usernames_object() ("null" when the
//                                 object is empty)
//   obj <active|-> <disabled|-> <editable|-> <collectible|-> <secondary|->
//                                 build td_api::usernames directly and print it
//   outline <zoom> <svg path...>  encode the SVG text into the compact path (the
//                                 inverse of the decoder), then print the JSON of
//                                 get_outline_object() ("null" when it returns
//                                 nullptr)
//   msgid <message|empty|service> <id> <date> <0|1>
//                                 MessageId::get_message_id(message, scheduled)
//   rules                         the fixed object matrix of the printer rules
//
// A `<name>` is plain text or `hex-<bytes>` (so the corpus can feed control
// characters and non-ASCII); `-` is the empty string.
//
// Build. The JSON printers (`td_api::to_json`) are **not** in any static
// library: they are compiled into the `tdjson_private` object library only, so
// the probe links that directory's ten generated objects directly (`libtdapi.a`
// has only the log-form `td_api::to_string`):
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I tdactor -I C:/tmp/td-build/tdutils tools/fixtures/td_api_objects_probe.cpp
//   C:/tmp/td-build/CMakeFiles/tdjson_private.dir/td/generate/auto/td/telegram/td_api_json_*.cpp.obj
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tdnet/libtdnet.a
//   C:/tmp/td-build/tdtl/libtdtl.a C:/tmp/td-build/tde2e/libtde2e.a
//   C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdutils/libtdutils.a
//   C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/sqlite/libtdsqlite.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi -lcrypt32 -lsecur32
//   -lmswsock -liphlpapi -ldnsapi -o C:/tmp/td-probe/td_api_objects_probe.exe
//
// The printing rules this probe pins (docs/facts.md F234): `@type` first, fields
// in declaration order, `int64` as a quoted string, `int32` as a number,
// `double` as fixed six decimals (so `nan` / `inf` come out unquoted and invalid
// as JSON), `bytes` as base64, control characters escaped, and any invalid
// UTF-8 in a string aborts inside JsonBuilder.

#include "td/telegram/MessageId.h"
#include "td/telegram/Outline.h"
#include "td/telegram/Usernames.h"
#include "td/telegram/td_api.h"
#include "td/telegram/td_api_json.h"

#include "td/utils/JsonBuilder.h"
#include "td/utils/Slice.h"
#include "td/utils/StringBuilder.h"
#include "td/utils/misc.h"

#include <cmath>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <limits>
#include <string>
#include <vector>

namespace {

using td::Slice;
using td::string;

const char *kTable = "AACAAAAHAAALMAAAQASTAVAAAZaacaaaahaaalmaaaqastava.az0123456789-,";

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

int hex_digit(char c) {
  if (c >= '0' && c <= '9') {
    return c - '0';
  }
  if (c >= 'a' && c <= 'f') {
    return c - 'a' + 10;
  }
  return -1;
}

string parse_hex(Slice hex) {
  string result;
  if (hex.size() % 2 != 0) {
    std::fprintf(stderr, "bad hex\n");
    std::exit(2);
  }
  for (size_t i = 0; i < hex.size(); i += 2) {
    int hi = hex_digit(hex[i]);
    int lo = hex_digit(hex[i + 1]);
    if (hi < 0 || lo < 0) {
      std::fprintf(stderr, "bad hex digit\n");
      std::exit(2);
    }
    result.push_back(static_cast<char>((hi << 4) | lo));
  }
  return result;
}

// "<name>" or "hex-<bytes>"; "-" is the empty string.
string parse_name(Slice token) {
  if (token == "-") {
    return string();
  }
  if (token.size() > 4 && token.substr(0, 4) == "hex-") {
    return parse_hex(token.substr(4));
  }
  return token.str();
}

std::vector<string> parse_csv(Slice token) {
  std::vector<string> result;
  if (token == "-") {
    return result;
  }
  for (auto part : td::full_split(token, ',')) {
    result.push_back(parse_name(part));
  }
  return result;
}

std::vector<td::telegram_api::object_ptr<td::telegram_api::username>> parse_items(Slice token) {
  std::vector<td::telegram_api::object_ptr<td::telegram_api::username>> result;
  if (token == "-") {
    return result;
  }
  for (auto item : td::full_split(token, ',')) {
    auto parts = td::full_split(item, ':');
    if (parts.size() != 5) {
      std::fprintf(stderr, "bad item\n");
      std::exit(2);
    }
    result.push_back(td::telegram_api::make_object<td::telegram_api::username>(
        0, parts[2] == "1", parts[1] == "1", parts[3] == "1", parts[4] == "1", parse_name(parts[0])));
  }
  return result;
}

int table_index(char c) {
  for (int i = 0; i < 64; i++) {
    if (kTable[i] == c) {
      return i;
    }
  }
  return -1;
}

string encode_svg(Slice svg) {
  string encoded;
  for (char raw : svg) {
    char c = raw == ' ' ? ',' : raw;
    int index = table_index(c);
    if (index < 0) {
      std::fprintf(stderr, "character '%c' is not in the compact table\n", c);
      std::exit(2);
    }
    encoded.push_back(static_cast<char>(192 + index));
  }
  return encoded;
}

template <class T>
string json(const td::td_api::object_ptr<T> &value) {
  if (value == nullptr) {
    return "null";
  }
  return td::json_encode<string>(td::ToJson(*value));
}

string json(const td::td_api::Object &value) {
  return td::json_encode<string>(td::ToJson(value));
}

void print_rules() {
  std::printf("ok=%s\n", json(td::td_api::ok()).c_str());
  std::printf("error=%s\n", json(td::td_api::error(0, "")).c_str());
  string controls;
  for (int i = 0; i < 32; i++) {
    controls.push_back(static_cast<char>(i));
  }
  controls.push_back(static_cast<char>(127));
  std::printf("escapes=%s\n", json(td::td_api::error(1, controls)).c_str());
  std::printf("utfe=%s\n",
              json(td::td_api::error(std::numeric_limits<td::int32>::min(), "a\"b\\c\xc3\xa9\xf0\x9f\x98\x80")).c_str());
  string bytes_data("\x00\x01\xff\x7f", 4);
  std::printf("data=%s\n", json(td::td_api::make_object<td::td_api::data>(bytes_data)).c_str());
  std::printf("dataempty=%s\n", json(td::td_api::make_object<td::td_api::data>(string())).c_str());
  std::printf("point=%s\n", json(td::td_api::make_object<td::td_api::point>(1.5, -0.0)).c_str());
  std::printf("pointbig=%s\n", json(td::td_api::make_object<td::td_api::point>(1e300, 1e-7)).c_str());
  std::printf("pointint=%s\n", json(td::td_api::make_object<td::td_api::point>(2.0, 3.0)).c_str());
  std::printf("pointnan=%s\n",
              json(td::td_api::make_object<td::td_api::point>(std::nan(""), std::numeric_limits<double>::infinity()))
                  .c_str());
  std::printf("pointneginf=%s\n",
              json(td::td_api::make_object<td::td_api::point>(-std::numeric_limits<double>::infinity(), 0.0000005))
                  .c_str());
  auto pos_max = td::td_api::make_object<td::td_api::chatPosition>(
      td::td_api::make_object<td::td_api::chatListMain>(), std::numeric_limits<td::int64>::max(), true, nullptr);
  std::printf("posmax=%s\n", json(pos_max).c_str());
  auto pos_min = td::td_api::make_object<td::td_api::chatPosition>(
      td::td_api::make_object<td::td_api::chatListArchive>(), std::numeric_limits<td::int64>::min(), false, nullptr);
  std::printf("posmin=%s\n", json(pos_min).c_str());
  std::printf("posnull=%s\n",
              json(td::td_api::make_object<td::td_api::chatPosition>(nullptr, 0, false, nullptr)).c_str());
  std::printf("cpath_empty=%s\n", json(td::td_api::make_object<td::td_api::closedVectorPath>()).c_str());
  std::printf("outline_empty=%s\n", json(td::td_api::make_object<td::td_api::outline>()).c_str());
  auto line = td::td_api::make_object<td::td_api::vectorPathCommandLine>(
      td::td_api::make_object<td::td_api::point>(0.25, -3.5));
  auto bezier = td::td_api::make_object<td::td_api::vectorPathCommandCubicBezierCurve>(
      td::td_api::make_object<td::td_api::point>(1, 2), td::td_api::make_object<td::td_api::point>(3, 4),
      td::td_api::make_object<td::td_api::point>(5, 6));
  std::vector<td::td_api::object_ptr<td::td_api::VectorPathCommand>> commands;
  commands.push_back(std::move(line));
  commands.push_back(std::move(bezier));
  std::vector<td::td_api::object_ptr<td::td_api::closedVectorPath>> paths;
  paths.push_back(td::td_api::make_object<td::td_api::closedVectorPath>(std::move(commands)));
  paths.push_back(nullptr);
  std::printf("outline_full=%s\n", json(td::td_api::make_object<td::td_api::outline>(std::move(paths))).c_str());
}

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc == 3 && std::strcmp(argv[1], "--check") == 0) {
    Slice check_case(argv[2]);
    if (check_case == "badutf") {
      std::printf("before\n");
      std::fflush(stdout);
      std::printf("after=%s\n", json(td::td_api::error(1, string("x\xffy", 3))).c_str());
      std::fflush(stdout);
      return 0;
    }
    std::fprintf(stderr, "unknown check case\n");
    return 2;
  }

  if (argc != 2) {
    std::fprintf(stderr, "usage: td_api_objects_probe <script.txt>\n");
    return 2;
  }

  auto script = read_all(argv[1]);
  td::Usernames current;
  for (auto raw_line : td::full_split(Slice(script), '\n')) {
    auto line = trim_line(raw_line);
    if (line.empty() || line[0] == '#') {
      continue;
    }
    if (line.substr(0, 7) == "outline") {
      // The SVG path may contain spaces, so the rest of the line is the payload.
      auto parts = td::full_split(line, ' ', 3);
      if (parts.size() < 2 || parts[0] != "outline") {
        std::fprintf(stderr, "bad outline line\n");
        return 2;
      }
      auto zoom = td::to_double(parts[1]);
      auto svg = parts.size() == 3 ? parts[2] : Slice();
      auto object = td::get_outline_object(encode_svg(svg), zoom, "td_api_objects_probe");
      std::printf("outline=%s\n", json(object).c_str());
      continue;
    }
    auto tokens = split_tokens(line);
    auto command = tokens[0];
    if (command == "ulist") {
      current = td::Usernames(parse_name(tokens[1]), parse_items(tokens[2]));
      std::printf("ulist=%s\n", json(current.get_usernames_object()).c_str());
    } else if (command == "obj") {
      auto active = parse_csv(tokens[1]);
      auto disabled = parse_csv(tokens[2]);
      auto editable = parse_name(tokens[3]);
      auto collectible = parse_csv(tokens[4]);
      auto secondary = parse_name(tokens[5]);
      auto object = td::td_api::make_object<td::td_api::usernames>(
          std::vector<string>(active.begin(), active.end()), std::vector<string>(disabled.begin(), disabled.end()),
          editable, std::vector<string>(collectible.begin(), collectible.end()), secondary);
      std::printf("obj=%s\n", json(object).c_str());
    } else if (command == "msgid") {
      auto id = td::to_integer<td::int32>(tokens[2]);
      auto date = td::to_integer<td::int32>(tokens[3]);
      bool scheduled = tokens[4] == "1";
      td::telegram_api::object_ptr<td::telegram_api::Message> message;
      if (tokens[1] == "message") {
        auto typed = td::telegram_api::make_object<td::telegram_api::message>();
        typed->id_ = id;
        typed->date_ = date;
        message = std::move(typed);
      } else if (tokens[1] == "empty") {
        auto typed = td::telegram_api::make_object<td::telegram_api::messageEmpty>();
        typed->id_ = id;
        message = std::move(typed);
      } else if (tokens[1] == "service") {
        auto typed = td::telegram_api::make_object<td::telegram_api::messageService>();
        typed->id_ = id;
        typed->date_ = date;
        message = std::move(typed);
      } else {
        std::fprintf(stderr, "bad message kind\n");
        return 2;
      }
      auto message_id = td::MessageId::get_message_id(message.get(), scheduled);
      td::StringBuilder sb;
      sb << message_id;
      std::printf("msgid raw=%lld valid=%d sched=%d type=%d text=%s\n",
                  static_cast<long long>(message_id.get()), static_cast<int>(message_id.is_valid()),
                  static_cast<int>(message_id.is_scheduled()), static_cast<int>(message_id.get_type()),
                  sb.as_cslice().str().c_str());
    } else if (command == "rules") {
      print_rules();
    } else {
      std::fprintf(stderr, "unknown command %s\n", command.str().c_str());
      return 2;
    }
  }
  return 0;
}