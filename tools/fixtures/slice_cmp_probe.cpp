// Oracle for tdutils Slice::find / rfind and operator== / operator!= / operator<.
//
//   slice_cmp_probe <script.txt>
//
// Script lines:
//   new <name> <hex|->        slice over a private copy of the bytes ("-" = empty)
//   find <name> <byte>        find(c)   -> `find <name> c=… pos=…`
//   rfind <name> <byte>       rfind(c)  -> `rfind <name> c=… pos=…`
//   cmp <a> <b>               `cmp <a> <b> eq=… ne=… lt=… gt=…`
//
// The byte argument is decimal 0..255. `pos` is printed as an unsigned 64-bit
// integer, so "not found" shows up as 18446744073709551615 (td::Slice::npos).
//
// operator< is memcmp over the common prefix — memcmp compares as *unsigned*
// char — then the lengths, so "" < "a", "ab" < "abc", and 0x7f < 0x80.
// operator== is size + memcmp, so any two empty slices are equal.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/slice_cmp_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group \
//       -lssl -lcrypto -lws2_32 -lpsapi -o slice_cmp_probe.exe

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
    } else if (op == "find" || op == "rfind") {
      int byte = 0;
      tokens >> byte;
      auto &bytes = payloads.at(name);
      td::Slice view(bytes.data(), bytes.size());
      size_t pos = op == "find" ? view.find(static_cast<char>(byte)) : view.rfind(static_cast<char>(byte));
      std::printf("%s %s c=%d pos=%llu\n", op.c_str(), name.c_str(), byte, static_cast<unsigned long long>(pos));
    } else if (op == "cmp") {
      std::string other;
      tokens >> other;
      auto &left_bytes = payloads.at(name);
      auto &right_bytes = payloads.at(other);
      td::Slice left(left_bytes.data(), left_bytes.size());
      td::Slice right(right_bytes.data(), right_bytes.size());
      std::printf("cmp %s %s eq=%d ne=%d lt=%d gt=%d\n", name.c_str(), other.c_str(), static_cast<int>(left == right),
                  static_cast<int>(left != right), static_cast<int>(left < right), static_cast<int>(right < left));
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
    std::fprintf(stderr, "usage: slice_cmp_probe <script.txt>\n");
    return 2;
  }
  return run_script(argv[1]);
}