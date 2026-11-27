// Oracle for tdutils begins_with / ends_with (free functions in misc.h).
//
//   slice_prefix_probe <script.txt>
//
// Script lines:
//   new <name> <hex|->        slice over a private copy of the bytes ("-" = empty)
//   begins <str> <affix>      -> `begins <str> <affix> result=0|1`
//   ends <str> <affix>        -> `ends <str> <affix> result=0|1`
//
// Both reject an affix longer than the string without touching memory, accept
// the empty affix, and compare byte for byte. `ends_with` with an empty suffix
// builds a zero-length slice at the one-past-end pointer — legal, and the byte
// there is never read.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/slice_prefix_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group \
//       -lssl -lcrypto -lws2_32 -lpsapi -o slice_prefix_probe.exe

#include "td/utils/misc.h"

#include "td/utils/Slice.h"

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <map>
#include <sstream>
#include <string>
#include <vector>

namespace {

std::vector<char> unhex(const std::string &text) {
  std::vector<char> bytes;
  if (text == "-") {
    return bytes;
  }
  bytes.reserve(text.size() / 2);
  for (size_t i = 0; i + 1 < text.size(); i += 2) {
    bytes.push_back(static_cast<char>(std::stoi(text.substr(i, 2), nullptr, 16)));
  }
  return bytes;
}

int run_script(const char *path) {
  std::ifstream input(path, std::ios::binary);
  if (!input) {
    std::fprintf(stderr, "cannot read %s\n", path);
    return 2;
  }
  std::string text((std::istreambuf_iterator<char>(input)), std::istreambuf_iterator<char>());
  std::istringstream lines(text);
  std::map<std::string, std::string> payloads;
  std::string line;
  while (std::getline(lines, line)) {
    std::istringstream tokens(line);
    std::string op;
    if (!(tokens >> op) || op[0] == '#') {
      continue;
    }
    std::string name;
    tokens >> name;
    if (op == "new") {
      std::string hex;
      tokens >> hex;
      auto bytes = unhex(hex);
      payloads[name] = std::string(bytes.begin(), bytes.end());
    } else if (op == "begins" || op == "ends") {
      std::string affix_name;
      tokens >> affix_name;
      auto &str = payloads.at(name);
      auto &affix = payloads.at(affix_name);
      td::Slice str_view(str.data(), str.size());
      td::Slice affix_view(affix.data(), affix.size());
      bool result = op == "begins" ? td::begins_with(str_view, affix_view) : td::ends_with(str_view, affix_view);
      std::printf("%s %s %s result=%d\n", op.c_str(), name.c_str(), affix_name.c_str(), static_cast<int>(result));
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
    std::fprintf(stderr, "usage: slice_prefix_probe <script.txt>\n");
    return 2;
  }
  return run_script(argv[1]);
}