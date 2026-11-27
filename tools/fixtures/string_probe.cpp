// Oracle for the string helpers in tdutils misc.h.
//
//   string_probe <script.txt>
//
// Script lines (all strings are hex, "-" = empty):
//   lower <hex>              to_lower
//   upper <hex>              to_upper
//   trim <hex>               trim
//   oneline <hex>            oneline
//   lpad <hex> <size> <char> lpad with a decimal char code
//   lpad0 <hex> <size>       lpad0
//   rpad <hex> <size> <char> rpad
//   replace <hex> <chars>    replace_with_spaces
//   implode <delim> <hex>…   implode with a decimal delimiter
//   split <hex> <delim>      split -> two parts, always
//   fullsplit <hex> <delim> <max_parts>
//   pred <hex>               one line per byte: is_space/alpha/digit/alnum/hex_digit
//
// `is_space` is { ' ', '\t', '\r', '\n', '\0', '\v' } — note it excludes '\f'.
// `to_lower`/`to_upper` are ASCII-only; bytes >= 0x80 pass through.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/string_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group \
//       -lssl -lcrypto -lws2_32 -lpsapi -o string_probe.exe

#include "td/utils/misc.h"

#include "td/utils/Slice.h"

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>
#include <vector>

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

std::string join_parts(const std::vector<std::string> &parts) {
  if (parts.empty()) {
    return "none";
  }
  std::string out;
  for (size_t i = 0; i < parts.size(); i++) {
    if (i != 0) {
      out += "|";
    }
    out += hex_of_string(parts[i]);
  }
  return out;
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
    if (op == "lower" || op == "upper" || op == "trim" || op == "oneline") {
      std::string hex;
      tokens >> hex;
      auto value = unhex(hex);
      std::string result;
      if (op == "lower") {
        result = td::to_lower(as_slice(value));
      } else if (op == "upper") {
        result = td::to_upper(as_slice(value));
      } else if (op == "trim") {
        result = td::trim(value);
      } else {
        result = td::oneline(as_slice(value));
      }
      std::printf("%s in=%s out=%s\n", op.c_str(), hex_or_dash(as_slice(value)).c_str(),
                  hex_of_string(result).c_str());
    } else if (op == "lpad" || op == "lpad0" || op == "rpad") {
      std::string hex;
      size_t size = 0;
      tokens >> hex >> size;
      auto value = unhex(hex);
      int code = ' ';
      std::string result;
      if (op == "lpad") {
        tokens >> code;
        result = td::lpad(value, size, static_cast<char>(code));
      } else if (op == "lpad0") {
        result = td::lpad0(value, size);
      } else {
        tokens >> code;
        result = td::rpad(value, size, static_cast<char>(code));
      }
      std::printf("%s in=%s size=%llu out=%s\n", op.c_str(), hex_or_dash(as_slice(value)).c_str(),
                  static_cast<unsigned long long>(size), hex_of_string(result).c_str());
    } else if (op == "replace") {
      std::string hex;
      std::string chars_hex;
      tokens >> hex >> chars_hex;
      auto value = unhex(hex);
      auto chars = unhex(chars_hex);
      std::string mutable_value = value;
      td::replace_with_spaces(td::MutableSlice(mutable_value.data(), mutable_value.size()), as_slice(chars));
      std::printf("replace in=%s chars=%s out=%s\n", hex_or_dash(as_slice(value)).c_str(),
                  hex_or_dash(as_slice(chars)).c_str(), hex_of_string(mutable_value).c_str());
    } else if (op == "implode") {
      int delimiter = ' ';
      tokens >> delimiter;
      std::vector<std::string> parts;
      std::string hex;
      while (tokens >> hex) {
        parts.push_back(unhex(hex));
      }
      auto result = td::implode(parts, static_cast<char>(delimiter));
      std::printf("implode delim=%d parts=%s out=%s\n", delimiter, join_parts(parts).c_str(),
                  hex_of_string(result).c_str());
    } else if (op == "split") {
      std::string hex;
      int delimiter = ' ';
      tokens >> hex >> delimiter;
      auto value = unhex(hex);
      auto result = td::split(value, static_cast<char>(delimiter));
      std::printf("split in=%s delim=%d a=%s b=%s\n", hex_or_dash(as_slice(value)).c_str(), delimiter,
                  hex_of_string(result.first).c_str(), hex_of_string(result.second).c_str());
    } else if (op == "fullsplit") {
      std::string hex;
      int delimiter = ' ';
      unsigned long long max_parts = 0;
      tokens >> hex >> delimiter >> max_parts;
      auto value = unhex(hex);
      auto result = td::full_split(value, static_cast<char>(delimiter), static_cast<size_t>(max_parts));
      std::printf("fullsplit in=%s delim=%d max=%llu parts=%s\n", hex_or_dash(as_slice(value)).c_str(), delimiter,
                  max_parts, join_parts(result).c_str());
    } else if (op == "pred") {
      std::string hex;
      tokens >> hex;
      auto value = unhex(hex);
      for (auto c : value) {
        std::printf("pred b=%02x space=%d alpha=%d digit=%d alnum=%d hex=%d\n", static_cast<unsigned char>(c),
                    static_cast<int>(td::is_space(c)), static_cast<int>(td::is_alpha(c)),
                    static_cast<int>(td::is_digit(c)), static_cast<int>(td::is_alnum(c)),
                    static_cast<int>(td::is_hex_digit(c)));
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
    std::fprintf(stderr, "usage: string_probe <script.txt>\n");
    return 2;
  }
  return run_script(argv[1]);
}