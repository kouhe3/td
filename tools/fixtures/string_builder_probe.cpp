// Oracle for tdutils StringBuilder (StringBuilder.h / StringBuilder.cpp).
//
//   string_builder_probe <script.txt>
//
// Script ops, one per line, ASCII only (bytes always travel as hex, "-" = empty):
//   new                      StringBuilder()          internal 130-byte buffer, 100 usable
//   new <cap>                StringBuilder(MutableSlice(cap), use_buffer=false)
//   clr                      clear()
//   s <hex>                  operator<<(Slice)         length-carrying, keeps NULs
//   cstr <hex>               operator<<(const char*)   NUL-terminated, stops at the first 0 byte
//   lit <index>              operator<<(const char (&)[N])  a fixed literal, N-1 bytes
//   ch <byte>                operator<<(char)
//   sch <byte>               operator<<(signed char)   goes through int, so 0xff prints "-1"
//   uch <byte>               operator<<(unsigned char) goes through unsigned int
//   b <0|1>                  operator<<(bool)
//   i32|u32|i64|u64|il|ul <dec>  operator<<(int|unsigned|long long|unsigned long long|long|unsigned long)
//   d <hexbits>              operator<<(double)        == FixedDouble(d, 6)
//   dp <precision> <hexbits> operator<<(FixedDouble(d, precision))
//   ptr <hex>                operator<<(const void*)   only constant integers, never a real address
//   pair <op> <v> <op> <v>   operator<<(std::pair<A, B>)  op is i32/u32/ch/s
//   vec <type> <v>...        operator<<(std::vector<T>)   type is i32/u32/ch/s/b
//   ac <count> <byte>        append_char(count, c)
//   pb <byte>                push_back(c)
//   pop                      pop_back()                CHECK-aborts on an empty builder
//   dump                     size(), is_error(), as_cslice() content/size and its NUL
//
// Every op line prints `... out=<bytes appended> size=<size()> err=<is_error()>`; `out` is
// empty ("-") whenever the op appended nothing, including after an error. `` is not a line
// separator, lines starting with '#' and empty lines are skipped.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -I <repo> -I <repo>/tdutils -I <repo>/tdactor -I <repo>/tdnet
//       -I <repo>/tddb -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto
//       tools/fixtures/string_builder_probe.cpp -Wl,--start-group
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group
//       -lssl -lcrypto -lws2_32 -lpsapi -o C:/tmp/td-probe/string_builder_probe.exe

#include "td/utils/StringBuilder.h"

#include "td/utils/Slice.h"

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iterator>
#include <list>
#include <memory>
#include <sstream>
#include <string>
#include <utility>
#include <variant>
#include <vector>

namespace {

std::unique_ptr<td::StringBuilder> builder;
std::vector<char> storage;
// Slice values handed to the builder must outlive the streaming call; std::list nodes are stable.
std::list<std::string> keep_alive;

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

td::Slice keep_slice(const std::string &hex) {
  keep_alive.push_back(unhex(hex));
  const std::string &text = keep_alive.back();
  return td::Slice(text);
}

unsigned long long parse_u64(const std::string &text) {
  return std::stoull(text, nullptr, 10);
}

long long parse_i64(const std::string &text) {
  return std::stoll(text, nullptr, 10);
}

unsigned long long parse_bits(const std::string &text) {
  return std::stoull(text, nullptr, 16);
}

// The bytes appended by the op that just ran, its size and its error flag.
void finish(const std::string &prefix, size_t before) {
  auto cs = builder->as_cslice();
  size_t after = builder->size();
  std::string added;
  if (after > before) {
    added.assign(cs.begin() + before, after - before);
  }
  std::printf("%s out=%s size=%zu err=%d\n", prefix.c_str(), hex_or_dash(td::Slice(added)).c_str(), after,
              static_cast<int>(builder->is_error()));
}

using Val = std::variant<int, unsigned int, char, td::Slice>;

Val parse_val(const std::string &op, const std::string &arg) {
  if (op == "i32") {
    return static_cast<int>(parse_i64(arg));
  }
  if (op == "u32") {
    return static_cast<unsigned int>(parse_u64(arg));
  }
  if (op == "ch") {
    return static_cast<char>(parse_u64(arg));
  }
  return keep_slice(arg);
}

double from_bits(unsigned long long bits) {
  double value = 0;
  std::memcpy(&value, &bits, sizeof(value));
  return value;
}

// Fixed literals for the `char (&)[N]` overload; N-1 bytes are appended, so "" contributes nothing.
const char literal0[] = "";
const char literal1[] = "abc";
const char literal2[] = "0123456789";

void dump() {
  auto cs = builder->as_cslice();
  std::printf("dump size=%zu err=%d cs=%s csize=%zu nul=%d\n", builder->size(),
              static_cast<int>(builder->is_error()), hex_or_dash(td::Slice(cs)).c_str(), cs.size(),
              static_cast<int>(cs.begin()[cs.size()] == 0));
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
    if (op == "new") {
      std::string cap_text;
      std::string prefix;
      if (tokens >> cap_text) {
        size_t cap = static_cast<size_t>(parse_u64(cap_text));
        // Slice CHECKs for a non-null begin, so cap 0 still gets one allocated byte.
        storage.assign(cap == 0 ? 1 : cap, 0);
        builder = std::make_unique<td::StringBuilder>(td::MutableSlice(storage.data(), cap));
        prefix = "new kind=slice cap=" + cap_text;
      } else {
        builder = std::make_unique<td::StringBuilder>();
        prefix = "new kind=default";
      }
      std::printf("%s\n", prefix.c_str());
      continue;
    }
    if (!builder) {
      std::printf("unknown %s\n", op.c_str());
      continue;
    }
    if (op == "clr") {
      builder->clear();
      std::printf("clr size=%zu err=%d\n", builder->size(), static_cast<int>(builder->is_error()));
    } else if (op == "dump") {
      dump();
    } else if (op == "s") {
      std::string hex;
      tokens >> hex;
      auto value = unhex(hex);
      size_t before = builder->size();
      *builder << td::Slice(value);
      finish("s in=" + hex, before);
    } else if (op == "cstr") {
      std::string hex;
      tokens >> hex;
      auto value = unhex(hex);
      size_t before = builder->size();
      *builder << value.c_str();
      finish("cstr in=" + hex, before);
    } else if (op == "lit") {
      std::string index_text;
      tokens >> index_text;
      int index = std::stoi(index_text);
      size_t before = builder->size();
      if (index == 0) {
        *builder << literal0;
      } else if (index == 1) {
        *builder << literal1;
      } else {
        *builder << literal2;
      }
      finish("lit idx=" + index_text, before);
    } else if (op == "ch" || op == "sch" || op == "uch") {
      std::string value_text;
      tokens >> value_text;
      unsigned long long raw = parse_u64(value_text);
      size_t before = builder->size();
      if (op == "ch") {
        *builder << static_cast<char>(raw);
      } else if (op == "sch") {
        *builder << static_cast<signed char>(raw);
      } else {
        *builder << static_cast<unsigned char>(raw);
      }
      finish(op + " v=" + std::to_string(raw), before);
    } else if (op == "b") {
      std::string value_text;
      tokens >> value_text;
      bool value = parse_u64(value_text) != 0;
      size_t before = builder->size();
      *builder << value;
      finish("b v=" + std::to_string(static_cast<int>(value)), before);
    } else if (op == "i32" || op == "u32" || op == "il" || op == "ul" || op == "i64" || op == "u64") {
      std::string value_text;
      tokens >> value_text;
      size_t before = builder->size();
      int width = 0;
      if (op == "i32") {
        int value = static_cast<int>(parse_i64(value_text));
        width = static_cast<int>(sizeof(value));
        *builder << value;
      } else if (op == "u32") {
        unsigned int value = static_cast<unsigned int>(parse_u64(value_text));
        width = static_cast<int>(sizeof(value));
        *builder << value;
      } else if (op == "il") {
        long value = static_cast<long>(parse_i64(value_text));
        width = static_cast<int>(sizeof(value));
        *builder << value;
      } else if (op == "ul") {
        unsigned long value = static_cast<unsigned long>(parse_u64(value_text));
        width = static_cast<int>(sizeof(value));
        *builder << value;
      } else if (op == "i64") {
        long long value = parse_i64(value_text);
        width = static_cast<int>(sizeof(value));
        *builder << value;
      } else {
        unsigned long long value = parse_u64(value_text);
        width = static_cast<int>(sizeof(value));
        *builder << value;
      }
      finish(op + " v=" + value_text + " w=" + std::to_string(width), before);
    } else if (op == "d" || op == "dp") {
      std::string precision_text = "6";
      std::string bits_text;
      if (op == "dp") {
        tokens >> precision_text >> bits_text;
      } else {
        tokens >> bits_text;
      }
      int precision = std::stoi(precision_text);
      double value = from_bits(parse_bits(bits_text));
      size_t before = builder->size();
      if (op == "dp") {
        *builder << td::StringBuilder::FixedDouble{value, precision};
      } else {
        *builder << value;
      }
      finish(op + " bits=" + bits_text + " p=" + std::to_string(precision), before);
    } else if (op == "ptr") {
      std::string value_text;
      tokens >> value_text;
      auto raw = static_cast<uintptr_t>(parse_bits(value_text));
      size_t before = builder->size();
      *builder << reinterpret_cast<const void *>(raw);
      finish("ptr v=" + value_text, before);
    } else if (op == "pair") {
      std::string op1, arg1, op2, arg2;
      tokens >> op1 >> arg1 >> op2 >> arg2;
      auto a = parse_val(op1, arg1);
      auto b = parse_val(op2, arg2);
      size_t before = builder->size();
      std::visit([&](auto left, auto right) { *builder << std::make_pair(left, right); }, a, b);
      finish("pair a=" + op1 + ":" + arg1 + " b=" + op2 + ":" + arg2, before);
    } else if (op == "vec") {
      std::string type;
      tokens >> type;
      std::string value_text;
      size_t before = builder->size();
      size_t count = 0;
      if (type == "i32") {
        std::vector<int> values;
        while (tokens >> value_text) {
          values.push_back(static_cast<int>(parse_i64(value_text)));
          count++;
        }
        *builder << values;
      } else if (type == "u32") {
        std::vector<unsigned int> values;
        while (tokens >> value_text) {
          values.push_back(static_cast<unsigned int>(parse_u64(value_text)));
          count++;
        }
        *builder << values;
      } else if (type == "ch") {
        std::vector<char> values;
        while (tokens >> value_text) {
          values.push_back(static_cast<char>(parse_u64(value_text)));
          count++;
        }
        *builder << values;
      } else if (type == "b") {
        std::vector<bool> values;
        while (tokens >> value_text) {
          values.push_back(parse_u64(value_text) != 0);
          count++;
        }
        *builder << values;
      } else {
        std::vector<td::Slice> values;
        while (tokens >> value_text) {
          values.push_back(keep_slice(value_text));
          count++;
        }
        *builder << values;
      }
      finish("vec ty=" + type + " n=" + std::to_string(count), before);
    } else if (op == "ac") {
      std::string count_text, value_text;
      tokens >> count_text >> value_text;
      size_t count = static_cast<size_t>(parse_u64(count_text));
      char value = static_cast<char>(parse_u64(value_text));
      size_t before = builder->size();
      builder->append_char(count, value);
      finish("ac count=" + count_text + " v=" + value_text, before);
    } else if (op == "pb") {
      std::string value_text;
      tokens >> value_text;
      char value = static_cast<char>(parse_u64(value_text));
      size_t before = builder->size();
      builder->push_back(value);
      finish("pb v=" + value_text, before);
    } else if (op == "pop") {
      builder->pop_back();
      std::printf("pop size=%zu err=%d\n", builder->size(), static_cast<int>(builder->is_error()));
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
    std::fprintf(stderr, "usage: string_builder_probe <script.txt>\n");
    return 2;
  }
  return run_script(argv[1]);
}