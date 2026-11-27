// Oracle for the structural part of tdutils utf8.h (no unicode.h tables).
//
//   utf8_probe <script.txt>
//
// Script lines (all byte strings are hex, "-" = empty):
//   check <hex>                 check_utf8              -> `check in=… r=0|1`
//   len <hex>                   utf8_length
//   first <byte>                is_utf8_character_first_code_unit
//   next <hex>                  next_utf8_unsafe        -> `next in=… code=… used=…`
//   app <code>                  append_utf8_character   -> `app code=… out=<hex>`
//   trunc <hex> <n>             utf8_truncate
//   substr <hex> <off>          utf8_substr
//   substr2 <hex> <off> <n>     utf8_substr(offset, length)
//   u16len <hex>                utf8_utf16_length
//   u16trunc <hex> <n>          utf8_utf16_truncate
//   u16substr <hex> <off>       utf8_utf16_substr
//   u16substr2 <hex> <off> <n>
//
// `utf8_truncate` compares the **byte** size with the character budget in its
// fast path, so a 3-byte character is "truncated" to 2 only when a later
// character exists. `utf8_utf16_length` adds 1 for (c & 0xF8) == 0xF0 while
// `utf8_utf16_truncate` subtracts 2 for c >= 0xF0, so F8..FF are counted
// inconsistently. `next_utf8_unsafe` assumes a valid lead byte (0xFE/0xFF would
// hit UNREACHABLE), so only valid sequences are fed.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/utf8_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group \
//       -lssl -lcrypto -lws2_32 -lpsapi -o utf8_probe.exe

#include "td/utils/utf8.h"

#include "td/utils/Slice.h"

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>

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
    if (op == "check") {
      std::string hex;
      tokens >> hex;
      auto value = unhex(hex);
      std::printf("check in=%s r=%d\n", hex_or_dash(as_slice(value)).c_str(),
                  static_cast<int>(td::check_utf8(as_slice(value))));
    } else if (op == "len") {
      std::string hex;
      tokens >> hex;
      auto value = unhex(hex);
      std::printf("len in=%s n=%llu\n", hex_or_dash(as_slice(value)).c_str(),
                  static_cast<unsigned long long>(td::utf8_length(as_slice(value))));
    } else if (op == "first") {
      int code = 0;
      tokens >> code;
      std::printf("first c=%d r=%d\n", code,
                  static_cast<int>(td::is_utf8_character_first_code_unit(static_cast<unsigned char>(code))));
    } else if (op == "next") {
      std::string hex;
      tokens >> hex;
      auto value = unhex(hex);
      auto *begin = reinterpret_cast<const unsigned char *>(value.data());
      uint32 code = 0;
      auto *end = td::next_utf8_unsafe(begin, &code);
      std::printf("next in=%s code=%u used=%lld\n", hex_or_dash(as_slice(value)).c_str(), code,
                  static_cast<long long>(end - begin));
    } else if (op == "app") {
      uint32 code = 0;
      tokens >> code;
      std::string result;
      td::append_utf8_character(result, code);
      std::printf("app code=%u out=%s\n", code, hex_or_dash(as_slice(result)).c_str());
    } else if (op == "trunc") {
      std::string hex;
      size_t length = 0;
      tokens >> hex >> length;
      auto value = unhex(hex);
      auto result = td::utf8_truncate(value, length);
      std::printf("trunc in=%s n=%llu out=%s\n", hex_or_dash(as_slice(value)).c_str(),
                  static_cast<unsigned long long>(length), hex_or_dash(as_slice(result)).c_str());
    } else if (op == "substr" || op == "substr2") {
      std::string hex;
      size_t offset = 0;
      tokens >> hex >> offset;
      auto value = unhex(hex);
      std::string result;
      if (op == "substr") {
        result = td::utf8_substr(value, offset);
      } else {
        size_t length = 0;
        tokens >> length;
        result = td::utf8_substr(value, offset, length);
      }
      std::printf("%s in=%s off=%llu out=%s\n", op.c_str(), hex_or_dash(as_slice(value)).c_str(),
                  static_cast<unsigned long long>(offset), hex_or_dash(as_slice(result)).c_str());
    } else if (op == "u16len") {
      std::string hex;
      tokens >> hex;
      auto value = unhex(hex);
      std::printf("u16len in=%s n=%llu\n", hex_or_dash(as_slice(value)).c_str(),
                  static_cast<unsigned long long>(td::utf8_utf16_length(as_slice(value))));
    } else if (op == "u16trunc") {
      std::string hex;
      size_t length = 0;
      tokens >> hex >> length;
      auto value = unhex(hex);
      auto result = td::utf8_utf16_truncate(as_slice(value), length);
      std::printf("u16trunc in=%s n=%llu out=%s\n", hex_or_dash(as_slice(value)).c_str(),
                  static_cast<unsigned long long>(length), hex_or_dash(result).c_str());
    } else if (op == "u16substr" || op == "u16substr2") {
      std::string hex;
      size_t offset = 0;
      tokens >> hex >> offset;
      auto value = unhex(hex);
      td::Slice result;
      std::string combined;
      if (op == "u16substr") {
        result = td::utf8_utf16_substr(as_slice(value), offset);
      } else {
        size_t length = 0;
        tokens >> length;
        result = td::utf8_utf16_substr(as_slice(value), offset, length);
      }
      std::printf("%s in=%s off=%llu out=%s\n", op.c_str(), hex_or_dash(as_slice(value)).c_str(),
                  static_cast<unsigned long long>(offset), hex_or_dash(result).c_str());
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
    std::fprintf(stderr, "usage: utf8_probe <script.txt>\n");
    return 2;
  }
  return run_script(argv[1]);
}