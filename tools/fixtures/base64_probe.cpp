// Oracle for tdutils base64 / base32 (base64.h + base64.cpp).
//
//   base64_probe <script.txt>
//
// Script lines (byte inputs are hex, text inputs are the hex of the ASCII text):
//   enc <hex>              base64_encode        -> `enc in=… out=<hex of text>`
//   encurl <hex>           base64url_encode
//   dec <text>             base64_decode        -> `dec in=… ok=1 out=…` or `ok=0 err=<hex>`
//   decurl <text>
//   is64 / is64url / is64c / is64urlc <text>     -> `… in=… r=0|1`
//   isany <byte>           is_base64any_character(c)
//   filter <text>          base64_filter
//   b32 <hex> <upper>      base32_encode
//   b32d <text>            base32_decode
//
// Decoding strips trailing '=' first: 3+ of them is "Wrong string padding",
// a length that is not a multiple of 4 (after padding) is "Wrong padding
// length", and the leftover bits of a short final group must be zero or it is
// "Wrong padding in the string" — that is why "ab==" is rejected while "aQ=="
// decodes to 'i'. The url variant uses -_ and adds no padding when encoding.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/base64_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group \
//       -lssl -lcrypto -lws2_32 -lpsapi -o base64_probe.exe

#include "td/utils/base64.h"

#include "td/utils/Slice.h"

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>

namespace {

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
    if (op == "enc" || op == "encurl") {
      std::string hex;
      tokens >> hex;
      auto value = unhex(hex);
      auto encoded = op == "enc" ? td::base64_encode(as_slice(value)) : td::base64url_encode(as_slice(value));
      std::printf("%s in=%s out=%s\n", op.c_str(), hex_or_dash(as_slice(value)).c_str(), hex_of_string(encoded).c_str());
    } else if (op == "dec" || op == "decurl") {
      std::string hex;
      tokens >> hex;
      auto value = unhex(hex);
      auto result = op == "dec" ? td::base64_decode(as_slice(value)) : td::base64url_decode(as_slice(value));
      if (result.is_ok()) {
        std::printf("%s in=%s ok=1 out=%s\n", op.c_str(), hex_or_dash(as_slice(value)).c_str(),
                    hex_of_string(result.ok()).c_str());
      } else {
        std::printf("%s in=%s ok=0 err=%s\n", op.c_str(), hex_or_dash(as_slice(value)).c_str(),
                    hex_or_dash(result.error().message()).c_str());
      }
    } else if (op == "is64" || op == "is64url" || op == "is64c" || op == "is64urlc") {
      std::string hex;
      tokens >> hex;
      auto value = unhex(hex);
      bool result = false;
      if (op == "is64") {
        result = td::is_base64(as_slice(value));
      } else if (op == "is64url") {
        result = td::is_base64url(as_slice(value));
      } else if (op == "is64c") {
        result = td::is_base64_characters(as_slice(value));
      } else {
        result = td::is_base64url_characters(as_slice(value));
      }
      std::printf("%s in=%s r=%d\n", op.c_str(), hex_or_dash(as_slice(value)).c_str(), static_cast<int>(result));
    } else if (op == "isany") {
      int code = 0;
      tokens >> code;
      std::printf("isany c=%d r=%d\n", code, static_cast<int>(td::is_base64any_character(static_cast<char>(code))));
    } else if (op == "filter") {
      std::string hex;
      tokens >> hex;
      auto value = unhex(hex);
      std::printf("filter in=%s out=%s\n", hex_or_dash(as_slice(value)).c_str(),
                  hex_of_string(td::base64_filter(as_slice(value))).c_str());
    } else if (op == "b32") {
      std::string hex;
      int upper = 0;
      tokens >> hex >> upper;
      auto value = unhex(hex);
      auto encoded = td::base32_encode(as_slice(value), upper != 0);
      std::printf("b32 in=%s upper=%d out=%s\n", hex_or_dash(as_slice(value)).c_str(), upper,
                  hex_of_string(encoded).c_str());
    } else if (op == "b32d") {
      std::string hex;
      tokens >> hex;
      auto value = unhex(hex);
      auto result = td::base32_decode(as_slice(value));
      if (result.is_ok()) {
        std::printf("b32d in=%s ok=1 out=%s\n", hex_or_dash(as_slice(value)).c_str(),
                    hex_of_string(result.ok()).c_str());
      } else {
        std::printf("b32d in=%s ok=0 err=%s\n", hex_or_dash(as_slice(value)).c_str(),
                    hex_or_dash(result.error().message()).c_str());
      }
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
    std::fprintf(stderr, "usage: base64_probe <script.txt>\n");
    return 2;
  }
  return run_script(argv[1]);
}