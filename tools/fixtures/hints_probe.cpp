// Oracle for tdutils Hints (Hints.h + Hints.cpp), ASCII-only word surface.
//
//   hints_probe <script.txt>
//
// Script lines (names/queries are hex of ASCII text, "-" = empty):
//   add <key> <name>        `add key=… size=…`
//   remove <key>            `remove key=… size=…`
//   rate <key> <rating>     `rate key=… rating=…`
//   search <query> <limit> [all]   `search q=… limit=… total=… keys=…`
//   sempty <limit>
//   has <key>               `has key=… r=0|1`
//   name <key>              `name key=… name=…`
//   size                    `size n=…`
//
// Results are sorted by (rating ascending, key ascending) — the *lowest* rating
// comes first — and `total` is the count before the limit is applied.
// `utf8_get_search_words` and the transliteration tables are not replicated:
// the corpus stays within [a-z0-9] and single spaces.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/hints_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group \
//       -lssl -lcrypto -lws2_32 -lpsapi -o hints_probe.exe

#include "td/utils/Hints.h"

#include "td/utils/Slice.h"

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>
#include <vector>

namespace {

using td::int32;
using td::int64;

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

std::string keys_to_string(const std::vector<int64> &keys) {
  if (keys.empty()) {
    return "-";
  }
  std::string out;
  for (size_t i = 0; i < keys.size(); i++) {
    if (i != 0) {
      out += ',';
    }
    out += std::to_string(keys[i]);
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
  td::Hints hints;
  std::string line;
  while (std::getline(lines, line)) {
    std::istringstream tokens(line);
    std::string op;
    if (!(tokens >> op) || op[0] == '#') {
      continue;
    }
    if (op == "add") {
      int64 key = 0;
      std::string hex;
      tokens >> key >> hex;
      auto name = unhex(hex);
      hints.add(key, as_slice(name));
      std::printf("add key=%lld size=%llu\n", static_cast<long long>(key),
                  static_cast<unsigned long long>(hints.size()));
    } else if (op == "remove") {
      int64 key = 0;
      tokens >> key;
      hints.remove(key);
      std::printf("remove key=%lld size=%llu\n", static_cast<long long>(key),
                  static_cast<unsigned long long>(hints.size()));
    } else if (op == "rate") {
      int64 key = 0;
      int64 rating = 0;
      tokens >> key >> rating;
      hints.set_rating(key, rating);
      std::printf("rate key=%lld rating=%lld\n", static_cast<long long>(key), static_cast<long long>(rating));
    } else if (op == "search" || op == "sempty") {
      std::string hex;
      int32 limit = 0;
      bool all = false;
      if (op == "search") {
        tokens >> hex >> limit;
        std::string flag;
        tokens >> flag;
        all = flag == "all";
      } else {
        tokens >> limit;
      }
      auto query = unhex(hex);
      auto result = op == "search" ? hints.search(as_slice(query), limit, all) : hints.search_empty(limit);
      std::printf("%s q=%s limit=%d total=%llu keys=%s\n", op.c_str(), hex_or_dash(as_slice(query)).c_str(), limit,
                  static_cast<unsigned long long>(result.first), keys_to_string(result.second).c_str());
    } else if (op == "has") {
      int64 key = 0;
      tokens >> key;
      std::printf("has key=%lld r=%d\n", static_cast<long long>(key), static_cast<int>(hints.has_key(key)));
    } else if (op == "name") {
      int64 key = 0;
      tokens >> key;
      auto name = hints.key_to_string(key);
      std::printf("name key=%lld name=%s\n", static_cast<long long>(key), hex_or_dash(as_slice(name)).c_str());
    } else if (op == "size") {
      std::printf("size n=%llu\n", static_cast<unsigned long long>(hints.size()));
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
    std::fprintf(stderr, "usage: hints_probe <script.txt>\n");
    return 2;
  }
  return run_script(argv[1]);
}