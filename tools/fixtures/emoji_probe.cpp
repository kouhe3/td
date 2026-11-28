// Oracle for `tdutils/td/utils/emoji.h` driven by a script. No Td, no actor,
// no clock: seven pure functions over a 2370-entry table plus the ZWJ/selector
// rules on top of it.
//
//   emoji_probe <script.txt>
//
// Commands (blank lines and '#' comments ignored; every payload is hex, `-` is
// the empty string):
//   is <hex>              is_emoji
//   find <hex>            find_emojis, printed as `a,b;c,d` (hex, `[]` if none)
//   fp <hex>              get_fitzpatrick_modifier
//   rfp <hex>             remove_fitzpatrick_modifier
//   rem <hex> <0|1>       remove_emoji_modifiers(remove_selectors)
//   rs <hex>              remove_emoji_selectors
//
// Measured behaviour this probe encodes (docs/facts.md F292/F293):
//   * the table has no zero-width joiners; `is_emoji` splits on U+200D and
//     requires every segment to be an element, which is how ZWJ sequences pass;
//   * `is_emoji_element` also accepts an entry with a single trailing U+FE0F
//     stripped, but two stacked selectors are rejected;
//   * `is_emoji` only looks for the first 0xE2 inside the first 32 bytes;
//   * `remove_emoji_modifiers(.., false)` keeps the variation selector;
//   * `remove_emoji_selectors` is the identity for anything `is_emoji` rejects.
//
// Build from the repo root against a configured C:/tmp/td-build:
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I tdactor -I C:/tmp/td-build/tdutils tools/fixtures/emoji_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/tdutils/libtdutils.a
//   C:/tmp/td-build/tdactor/libtdactor.a -Wl,--end-group
//   -lssl -lcrypto -lz -lws2_32 -lpsapi -lcrypt32 -lsecur32 -lmswsock -liphlpapi -ldnsapi
//   -o C:/tmp/td-probe/emoji_probe.exe

#include "td/utils/emoji.h"

#include "td/utils/Slice.h"

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <string>
#include <vector>

namespace {

std::string hex_all(td::Slice s) {
  static const char *digits = "0123456789abcdef";
  if (s.empty()) {
    return "-";
  }
  std::string out;
  out.reserve(s.size() * 2);
  for (auto c : s) {
    auto u = static_cast<unsigned char>(c);
    out += digits[u >> 4];
    out += digits[u & 15];
  }
  return out;
}

bool from_hex(const std::string &h, std::string &out) {
  out.clear();
  if (h == "-") {
    return true;
  }
  if (h.size() % 2 != 0) {
    return false;
  }
  for (size_t i = 0; i < h.size(); i += 2) {
    auto nib = [](char c) -> int {
      if (c >= '0' && c <= '9') {
        return c - '0';
      }
      if (c >= 'a' && c <= 'f') {
        return c - 'a' + 10;
      }
      return -1;
    };
    int hi = nib(h[i]);
    int lo = nib(h[i + 1]);
    if (hi < 0 || lo < 0) {
      return false;
    }
    out.push_back(static_cast<char>((hi << 4) | lo));
  }
  return true;
}

std::vector<std::string> split(const std::string &line) {
  std::vector<std::string> out;
  size_t pos = 0;
  while (pos < line.size()) {
    while (pos < line.size() && (line[pos] == ' ' || line[pos] == '\t')) {
      pos++;
    }
    if (pos == line.size()) {
      break;
    }
    size_t start = pos;
    while (pos < line.size() && line[pos] != ' ' && line[pos] != '\t') {
      pos++;
    }
    out.push_back(line.substr(start, pos - start));
  }
  return out;
}

int run_script(const char *script_path) {
  std::ifstream in(script_path);
  if (!in) {
    std::fprintf(stderr, "cannot read %s\n", script_path);
    return 2;
  }
  std::string line;
  while (std::getline(in, line)) {
    if (!line.empty() && line.back() == '\r') {
      line.pop_back();
    }
    auto tok = split(line);
    if (tok.empty() || tok[0][0] == '#') {
      continue;
    }
    const std::string &cmd = tok[0];
    std::string bytes;
    if (cmd == "rem") {
      if (tok.size() != 3 || !from_hex(tok[1], bytes) || (tok[2] != "0" && tok[2] != "1")) {
        std::printf("reject rem argc=%zu\n", tok.size());
        continue;
      }
      auto out = td::remove_emoji_modifiers(td::Slice(bytes), tok[2] == "1");
      std::printf("rem %s %s %s\n", tok[1].c_str(), tok[2].c_str(), hex_all(out).c_str());
      continue;
    }
    if (tok.size() != 2 || !from_hex(tok[1], bytes)) {
      std::printf("reject %s argc=%zu\n", cmd.c_str(), tok.size());
      continue;
    }
    if (cmd == "is") {
      std::printf("is %s %d\n", tok[1].c_str(), static_cast<int>(td::is_emoji(td::Slice(bytes))));
    } else if (cmd == "find") {
      auto found = td::find_emojis(td::Slice(bytes));
      std::printf("find %s [", tok[1].c_str());
      for (size_t i = 0; i < found.size(); i++) {
        std::printf("%s%s", i ? ";" : "", hex_all(found[i]).c_str());
      }
      std::printf("]\n");
    } else if (cmd == "fp") {
      std::printf("fp %s %d\n", tok[1].c_str(), td::get_fitzpatrick_modifier(td::Slice(bytes)));
    } else if (cmd == "rfp") {
      std::printf("rfp %s %s\n", tok[1].c_str(), hex_all(td::remove_fitzpatrick_modifier(td::Slice(bytes))).c_str());
    } else if (cmd == "rs") {
      std::printf("rs %s %s\n", tok[1].c_str(), hex_all(td::remove_emoji_selectors(td::Slice(bytes))).c_str());
    } else {
      std::printf("reject unknown=%s\n", cmd.c_str());
    }
  }
  return 0;
}

}  // namespace

int main(int argc, char **argv) {
  if (argc != 2) {
    std::fprintf(stderr, "usage: emoji_probe <script.txt>\n");
    return 2;
  }
  return run_script(argv[1]);
}