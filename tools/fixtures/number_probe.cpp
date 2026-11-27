// Oracle for the integer parsing helpers in tdutils misc.h.
//
//   number_probe <script.txt>
//
// Script lines (the input text is hex, "-" = empty):
//   i32 <text>       to_integer<int32>     -> `i32 in=… out=…`
//   i64 <text>       to_integer<int64>
//   u32 <text>       to_integer<uint32>
//   u64 <text>       to_integer<uint64>
//   i32s / i64s / u32s / u64s <text>   to_integer_safe -> `… ok=1 out=…` or `… ok=0 err=<hex>`
//   hx32 / hx64 <text>                 hex_to_integer
//   hxs32 / hxs64 <text>               hex_to_integer_safe
//   hdec <text>                        hex_decode
//   henc <text>                        hex_encode
//
// The signed parser accumulates in the *unsigned* type (so it wraps modulo
// 2^bits), then negates when the value exceeds T::max and clamps to T::min if
// the negated value is still too large. That makes "4294967296" parse to 0 and
// "2147483649" to -2147483647 for int32. `+` is not a sign character, and
// parsing stops at the first non-digit.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/number_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group \
//       -lssl -lcrypto -lws2_32 -lpsapi -o number_probe.exe

#include "td/utils/misc.h"

#include "td/utils/Slice.h"

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>

namespace {

using td::int32;
using td::int64;
using td::uint32;
using td::uint64;

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

std::string hex_of_string(const std::string &text) {
  return hex_or_dash(td::Slice(text.data(), text.size()));
}

td::Slice as_slice(const std::string &text) {
  return td::Slice(text.data(), text.size());
}

template <class T>
void print_safe(const char *op, const std::string &input, const td::Result<T> &result) {
  if (result.is_ok()) {
    std::string printed;
    if (std::is_signed<T>::value) {
      printed = std::to_string(static_cast<long long>(result.ok()));
    } else {
      printed = std::to_string(static_cast<unsigned long long>(result.ok()));
    }
    std::printf("%s in=%s ok=1 out=%s\n", op, hex_or_dash(as_slice(input)).c_str(), printed.c_str());
  } else {
    std::printf("%s in=%s ok=0 err=%s\n", op, hex_or_dash(as_slice(input)).c_str(),
                hex_or_dash(result.error().message()).c_str());
  }
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
    std::string hex;
    tokens >> hex;
    auto value = unhex(hex);
    auto slice = as_slice(value);
    if (op == "i32") {
      std::printf("i32 in=%s out=%lld\n", hex_or_dash(slice).c_str(),
                  static_cast<long long>(td::to_integer<int32>(slice)));
    } else if (op == "i64") {
      std::printf("i64 in=%s out=%lld\n", hex_or_dash(slice).c_str(),
                  static_cast<long long>(td::to_integer<int64>(slice)));
    } else if (op == "u32") {
      std::printf("u32 in=%s out=%llu\n", hex_or_dash(slice).c_str(),
                  static_cast<unsigned long long>(td::to_integer<uint32>(slice)));
    } else if (op == "u64") {
      std::printf("u64 in=%s out=%llu\n", hex_or_dash(slice).c_str(),
                  static_cast<unsigned long long>(td::to_integer<uint64>(slice)));
    } else if (op == "i32s") {
      print_safe("i32s", value, td::to_integer_safe<int32>(slice));
    } else if (op == "i64s") {
      print_safe("i64s", value, td::to_integer_safe<int64>(slice));
    } else if (op == "u32s") {
      print_safe("u32s", value, td::to_integer_safe<uint32>(slice));
    } else if (op == "u64s") {
      print_safe("u64s", value, td::to_integer_safe<uint64>(slice));
    } else if (op == "hx32") {
      std::printf("hx32 in=%s out=%llu\n", hex_or_dash(slice).c_str(),
                  static_cast<unsigned long long>(td::hex_to_integer<uint32>(slice)));
    } else if (op == "hx64") {
      std::printf("hx64 in=%s out=%llu\n", hex_or_dash(slice).c_str(),
                  static_cast<unsigned long long>(td::hex_to_integer<uint64>(slice)));
    } else if (op == "hxs32") {
      print_safe("hxs32", value, td::hex_to_integer_safe<uint32>(slice));
    } else if (op == "hxs64") {
      print_safe("hxs64", value, td::hex_to_integer_safe<uint64>(slice));
    } else if (op == "hdec") {
      auto result = td::hex_decode(slice);
      if (result.is_ok()) {
        std::printf("hdec in=%s ok=1 out=%s\n", hex_or_dash(slice).c_str(), hex_of_string(result.ok()).c_str());
      } else {
        std::printf("hdec in=%s ok=0 err=%s\n", hex_or_dash(slice).c_str(),
                    hex_or_dash(result.error().message()).c_str());
      }
    } else if (op == "henc") {
      auto encoded = td::hex_encode(slice);
      std::printf("henc in=%s out=%s\n", hex_or_dash(slice).c_str(), encoded.empty() ? "-" : encoded.c_str());
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
    std::fprintf(stderr, "usage: number_probe <script.txt>\n");
    return 2;
  }
  return run_script(argv[1]);
}