// Oracle for tdutils unicode.h (the tables in unicode.cpp) plus the two utf8.cpp users of them.
//
//   unicode_probe <script.txt>
//
// Script lines (all byte strings are hex, "-" = empty, all codes are decimal uint32):
//   cat <code>          get_unicode_simple_category  -> `cat c=0x… v=<enum>`
//   prep <code>         prepare_search_character     -> `prep c=0x… v=…`
//   low <code>          unicode_to_lower             -> `low c=0x… v=…`
//   dia <code>          remove_diacritics            -> `dia c=0x… v=…`
//   sweep <lo> <hi>     all four for every code      -> `c=0x… cat=… prep=… low=… dia=…`
//   utfl <hex>          utf8_to_lower                -> `utfl in=… out=…`
//   words <hex>         utf8_get_search_words        -> `words in=… n=…` + one word per line
//
// `sweep` exists so a difference localises to a single code point instead of a function.
// `get_unicode_simple_category` indexes `unicode_simple_category_jump_pos` by
// `0x400 - 2 + (code >> 16)`, i.e. it stops being defined above 0x11FFFF (the jump table
// holds 1040 entries); `prep`/`low`/`dia` have an explicit `code > 0x10ffff` early return.
// `next_utf8_unsafe` assumes a valid lead byte and reads up to 4 bytes, so only complete
// sequences (or a lone continuation byte, which reads just the string terminator) are fed.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/unicode_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group \
//       -lssl -lcrypto -lws2_32 -lpsapi -o unicode_probe.exe

#include "td/utils/unicode.h"
#include "td/utils/utf8.h"

#include "td/utils/Slice.h"

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <string>
#include <vector>

namespace {

using td::uint32;

std::string unhex(const std::string &text) {
  if (text == "-") {
    return std::string();
  }
  std::string bytes;
  bytes.reserve(text.size() / 2);
  for (size_t i = 0; i + 1 < text.size(); i += 2) {
    bytes.push_back(static_cast<char>(std::stoi(text.substr(i, 2), nullptr, 16)));
  }
  return bytes;
}

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

uint32 parse_code(const char *text) {
  return static_cast<uint32>(std::strtoull(text, nullptr, 10));
}

std::vector<std::string> split(const std::string &line) {
  std::vector<std::string> parts;
  size_t pos = 0;
  while (pos < line.size()) {
    while (pos < line.size() && (line[pos] == ' ' || line[pos] == '\t' || line[pos] == '\r')) {
      ++pos;
    }
    size_t begin = pos;
    while (pos < line.size() && line[pos] != ' ' && line[pos] != '\t' && line[pos] != '\r') {
      ++pos;
    }
    if (pos > begin) {
      parts.push_back(line.substr(begin, pos - begin));
    }
  }
  return parts;
}

int run_script(const char *path) {
  std::ifstream input(path, std::ios::binary);
  if (!input) {
    std::fprintf(stderr, "cannot read %s\n", path);
    return 2;
  }
  std::string line;
  while (std::getline(input, line)) {
    auto parts = split(line);
    if (parts.empty() || parts[0][0] == '#') {
      continue;
    }
    const std::string &op = parts[0];
    const auto code_at = [&parts](size_t index) { return parse_code(parts.at(index).c_str()); };
    if (op == "cat" || op == "prep" || op == "low" || op == "dia") {
      auto code = code_at(1);
      auto value = op == "cat"   ? static_cast<uint32>(td::get_unicode_simple_category(code))
                   : op == "prep" ? td::prepare_search_character(code)
                   : op == "low"  ? td::unicode_to_lower(code)
                                  : td::remove_diacritics(code);
      std::printf("%s c=0x%x v=%u\n", op.c_str(), code, value);
    } else if (op == "sweep") {
      auto lo = code_at(1);
      auto hi = code_at(2);
      for (unsigned long long code = lo; code <= hi; ++code) {
        auto c = static_cast<uint32>(code);
        std::printf("c=0x%x cat=%u prep=%u low=%u dia=%u\n", c,
                    static_cast<uint32>(td::get_unicode_simple_category(c)), td::prepare_search_character(c),
                    td::unicode_to_lower(c), td::remove_diacritics(c));
      }
    } else if (op == "utfl") {
      auto value = unhex(parts.at(1));
      std::printf("utfl in=%s out=%s\n", hex_or_dash(value).c_str(), hex_or_dash(td::utf8_to_lower(as_slice(value))).c_str());
    } else if (op == "words") {
      auto value = unhex(parts.at(1));
      auto words = td::utf8_get_search_words(as_slice(value));
      std::printf("words in=%s n=%zu\n", hex_or_dash(value).c_str(), words.size());
      if (words.empty()) {
        std::printf("-\n");
      }
      for (const auto &word : words) {
        std::printf("%s\n", hex_or_dash(word).c_str());
      }
    } else {
      std::printf("unknown %s\n", op.c_str());
    }
  }
  return 0;
}

}  // namespace

int main(int argc, char **argv) {
  if (argc != 2) {
    std::fprintf(stderr, "usage: unicode_probe <script.txt>\n");
    return 2;
  }
  return run_script(argv[1]);
}