// Oracle for td/utils/Hash.h in this build configuration (TD_HAVE_ABSL 0).
//
//   hash_probe <script.txt>
//
// Script lines:
//   i32 <v>                AbslHash<int32_t>       -> `i32 v=… hash=…`
//   i64 <v>                AbslHash<int64_t>
//   u32 <v>                AbslHash<uint32_t>
//   u64 <v>                AbslHash<uint64_t>
//   b <0|1>                AbslHash<bool>
//   ch <v>                 AbslHash<char>
//   uch <v>                AbslHash<unsigned char>
//   pair i32 <a> <b>       AbslHash<std::pair<int32_t, int32_t>>
//   pair i64 <a> <b>       AbslHash<std::pair<int64_t, int64_t>>
//   hasher <init> [v…]     Hasher(init) + Hasher::combine for every v
//   hasherpair <init> <a> <b>
//
// td::AbslHash is TdHash here, which is `size_t(v)` — a sign-extending integer
// conversion. Pairs are combined by XOR, so (a, b) and (b, a) hash alike.
// Hash values are printed as unsigned 64-bit.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/hash_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group \
//       -lssl -lcrypto -lws2_32 -lpsapi -o hash_probe.exe

#include "td/utils/Hash.h"

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>
#include <utility>

namespace {

template <class T>
unsigned long long hash_of(T value) {
  return static_cast<unsigned long long>(td::AbslHash<T>()(value));
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
    if (op == "i32" || op == "i64" || op == "u32" || op == "u64" || op == "b" || op == "ch" || op == "uch") {
      long long signed_value = 0;
      unsigned long long unsigned_value = 0;
      if (op[0] == 'u' || op == "b" || op == "ch") {
        tokens >> unsigned_value;
      } else {
        tokens >> signed_value;
      }
      if (op == "i32") {
        auto value = static_cast<int32_t>(signed_value);
        std::printf("i32 v=%lld hash=%llu\n", static_cast<long long>(value), hash_of(value));
      } else if (op == "i64") {
        auto value = static_cast<int64_t>(signed_value);
        std::printf("i64 v=%lld hash=%llu\n", static_cast<long long>(value), hash_of(value));
      } else if (op == "u32") {
        auto value = static_cast<uint32_t>(unsigned_value);
        std::printf("u32 v=%llu hash=%llu\n", static_cast<unsigned long long>(value), hash_of(value));
      } else if (op == "u64") {
        auto value = static_cast<uint64_t>(unsigned_value);
        std::printf("u64 v=%llu hash=%llu\n", unsigned_value, hash_of(value));
      } else if (op == "b") {
        auto value = unsigned_value != 0;
        std::printf("b v=%d hash=%llu\n", static_cast<int>(value), hash_of(value));
      } else if (op == "ch") {
        auto value = static_cast<char>(unsigned_value);
        std::printf("ch v=%d hash=%llu\n", static_cast<int>(value), hash_of(value));
      } else {
        auto value = static_cast<unsigned char>(unsigned_value);
        std::printf("uch v=%d hash=%llu\n", static_cast<int>(value), hash_of(value));
      }
    } else if (op == "pair") {
      std::string kind;
      tokens >> kind;
      long long first = 0;
      long long second = 0;
      tokens >> first >> second;
      if (kind == "i32") {
        auto value = std::make_pair(static_cast<int32_t>(first), static_cast<int32_t>(second));
        std::printf("pair i32 a=%lld b=%lld hash=%llu\n", first, second, hash_of(value));
      } else {
        auto value = std::make_pair(static_cast<int64_t>(first), static_cast<int64_t>(second));
        std::printf("pair i64 a=%lld b=%lld hash=%llu\n", first, second, hash_of(value));
      }
    } else if (op == "hasher") {
      unsigned long long init = 0;
      tokens >> init;
      td::Hasher hasher(static_cast<size_t>(init));
      unsigned long long value = 0;
      int count = 0;
      while (tokens >> value) {
        hasher = td::Hasher::combine(hasher, static_cast<size_t>(value));
        count++;
      }
      std::printf("hasher init=%llu n=%d hash=%llu\n", init, count,
                  static_cast<unsigned long long>(hasher.finalize()));
    } else if (op == "hasherpair") {
      unsigned long long init = 0;
      long long first = 0;
      long long second = 0;
      tokens >> init >> first >> second;
      auto hasher = td::Hasher::combine(td::Hasher(static_cast<size_t>(init)),
                                        std::make_pair(static_cast<int32_t>(first), static_cast<int32_t>(second)));
      std::printf("hasherpair init=%llu a=%lld b=%lld hash=%llu\n", init, first, second,
                  static_cast<unsigned long long>(hasher.finalize()));
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
    std::fprintf(stderr, "usage: hash_probe <script.txt>\n");
    return 2;
  }
  return run_script(argv[1]);
}