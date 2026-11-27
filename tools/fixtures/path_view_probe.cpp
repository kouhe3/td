// Oracle for tdutils PathView (PathView.h + PathView.cpp).
//
//   path_view_probe <script.txt>
//
// Script lines:
//   path <text>                 -> `path in=… empty=… dir=… abs=… rel=… parent=… pns=… ext=… stem=… name=… nodot=…`
//   rel <path> <dir> [force]    -> `rel in=… dir=… force=… out=…`
//   dirfile <text>              -> `dirfile in=… out=…`
//
// Slice outputs are hex ("-" for empty), so backslashes and trailing spaces are
// unambiguous. `parent_dir_noslash` returns TD_DIR_SLASH for a root-only path,
// which is '\\' in this Windows build.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/path_view_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group \
//       -lssl -lcrypto -lws2_32 -lpsapi -o path_view_probe.exe

#include "td/utils/PathView.h"

#include "td/utils/Slice.h"

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>

namespace {

std::string hex_or_dash(td::Slice data) {
  if (data.empty()) {
    return "-";
  }
  std::string out;
  out.reserve(data.size() * 2);
  for (auto c : data) {
    char buffer[3];
    std::snprintf(buffer, sizeof(buffer), "%02x", static_cast<unsigned char>(c));
    out += buffer;
  }
  return out;
}

td::Slice as_slice(const std::string &text) {
  return td::Slice(text.data(), text.size());
}

// "-" stands for the empty path, which cannot be written as a token.
std::string decode(const std::string &token) {
  return token == "-" ? std::string() : token;
}

int run_script(const char *path) {
  std::ifstream input(path, std::ios::binary);
  if (!input) {
    std::fprintf(stderr, "cannot read %s\n", path);
    return 2;
  }
  std::string text((std::istreambuf_iterator<char>(input)), std::istreambuf_iterator<char>());
  std::istringstream lines(text);
  std::string line;
  while (std::getline(lines, line)) {
    std::istringstream tokens(line);
    std::string op;
    if (!(tokens >> op) || op[0] == '#') {
      continue;
    }
    if (op == "path") {
      std::string value;
      tokens >> value;
      auto decoded = decode(value);
      td::PathView view(as_slice(decoded));
      std::printf(
          "path in=%s empty=%d dir=%d abs=%d rel=%d parent=%s pns=%s ext=%s stem=%s name=%s nodot=%s nox=%s\n",
          hex_or_dash(as_slice(decoded)).c_str(), static_cast<int>(view.empty()), static_cast<int>(view.is_dir()),
          static_cast<int>(view.is_absolute()), static_cast<int>(view.is_relative()),
          hex_or_dash(view.parent_dir()).c_str(), hex_or_dash(view.parent_dir_noslash()).c_str(),
          hex_or_dash(view.extension()).c_str(), hex_or_dash(view.file_stem()).c_str(),
          hex_or_dash(view.file_name()).c_str(), hex_or_dash(view.file_name_without_extension()).c_str(),
          hex_or_dash(view.without_extension()).c_str());
    } else if (op == "rel") {
      std::string path_value;
      std::string dir_value;
      std::string force_value;
      tokens >> path_value >> dir_value;
      tokens >> force_value;
      bool force = force_value == "1";
      auto decoded_path = decode(path_value);
      auto decoded_dir = decode(dir_value);
      auto result = td::PathView::relative(as_slice(decoded_path), as_slice(decoded_dir), force);
      std::printf("rel in=%s dir=%s force=%d out=%s\n", hex_or_dash(as_slice(decoded_path)).c_str(),
                  hex_or_dash(as_slice(decoded_dir)).c_str(), static_cast<int>(force), hex_or_dash(result).c_str());
    } else if (op == "dirfile") {
      std::string value;
      tokens >> value;
      auto decoded = decode(value);
      auto result = td::PathView::dir_and_file(as_slice(decoded));
      std::printf("dirfile in=%s out=%s\n", hex_or_dash(as_slice(decoded)).c_str(), hex_or_dash(result).c_str());
    } else {
      std::printf("unknown %s\n", op.c_str());
    }
  }
  return 0;
}

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc != 2) {
    std::fprintf(stderr, "usage: path_view_probe <script.txt>\n");
    return 2;
  }
  return run_script(argv[1]);
}