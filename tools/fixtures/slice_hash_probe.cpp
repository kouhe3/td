// Oracle for tdutils SliceHash (Slice.h).
//
//   slice_hash_probe <script.txt>
//
// Script lines:
//   new <name> <hex|->        Slice over a private copy ("-" = empty)
//   hash <name>               -> `hash <name> size=… h=…`
//   mhash <name>              -> same, but through MutableSlice -> Slice
//
// The hash is `uint32 result = 0; for (char c : slice) result = result * 123456789 + c;`
// so it wraps modulo 2^32 every step, and `char` is signed here: 0x80 adds
// 0xFFFFFF80. The empty slice hashes to 0.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/slice_hash_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group \
//       -lssl -lcrypto -lws2_32 -lpsapi -o slice_hash_probe.exe

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
    } else if (op == "hash" || op == "mhash") {
      auto &bytes = payloads.at(name);
      td::Slice view(bytes.data(), bytes.size());
      td::uint32 value =
          op == "hash" ? td::SliceHash()(view) : td::SliceHash()(td::MutableSlice(bytes.data(), bytes.size()));
      std::printf("%s %s size=%llu h=%u\n", op.c_str(), name.c_str(),
                  static_cast<unsigned long long>(view.size()), static_cast<unsigned>(value));
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
    std::fprintf(stderr, "usage: slice_hash_probe <script.txt>\n");
    return 2;
  }
  return run_script(argv[1]);
}