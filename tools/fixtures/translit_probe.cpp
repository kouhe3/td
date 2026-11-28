// Oracle for tdutils/translit.h: td::get_word_transliterations.
//
//   translit_probe <script.txt>
//
// Script lines (words are hex, "-" = empty, so the script itself stays ASCII):
//   tr <hex> <0|1>     get_word_transliterations(word, allow_partial == 1)
//                      -> `tr in=<hex> partial=<0|1> n=<k>` then ` out<i>=<hex>` per i
//   once <hex> <0|1>   the same word ONCE_REPEATS times in this process
//                      -> `once in=<hex> partial=<0|1> repeats=<r> identical=<0|1> n=<k>`
//                         then ` out=<h0>,<h1>,…` ("-" when the result is empty)
//
// The `tr` output keeps the returned order, so the set *and* the ordering are
// compared. Both passes are visible: the first maps each character with the
// simple table, the second restarts from the beginning, prefers the first
// complex rule (table order) that the suffix begins with, and appends
// `s + rule.second` at every scan position where a rule begins with the whole
// remaining suffix when `allow_partial` is set. `td::unique` then sorts by bytes
// and drops adjacent duplicates.
//
// `next_utf8_unsafe` does not validate a sequence: it picks the length from the
// lead byte (1 for < 0x80, 2 for 0x80..0xDF, 3 for 0xE0..0xEF, 4 for 0xF0..0xF7,
// UNREACHABLE for 0xF8..0xFF) and reads that many bytes. The driver therefore
// feeds only payloads whose last sequence fits, so the probe never reads past
// the word; the one deliberately malformed payload (c0af, an overlong "/") has a
// length-valid shape and shows the re-encoding through append_utf8_character.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/translit_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group \
//       -lssl -lcrypto -lws2_32 -lpsapi -o translit_probe.exe

#include "td/utils/translit.h"

#include "td/utils/Slice.h"

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>

namespace {

constexpr int ONCE_REPEATS = 5;

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

std::string join_hex(const td::vector<td::string> &values) {
  if (values.empty()) {
    return "-";
  }
  std::string out;
  for (size_t i = 0; i < values.size(); i++) {
    if (i != 0) {
      out += ',';
    }
    out += hex_or_dash(as_slice(values[i]));
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
    if (op == "tr" || op == "once") {
      std::string hex;
      int partial = 0;
      tokens >> hex >> partial;
      auto word = unhex(hex);
      auto in = hex_or_dash(as_slice(word));
      const bool allow_partial = partial != 0;
      if (op == "tr") {
        auto result = td::get_word_transliterations(as_slice(word), allow_partial);
        std::printf("tr in=%s partial=%d n=%llu\n", in.c_str(), static_cast<int>(allow_partial),
                    static_cast<unsigned long long>(result.size()));
        for (size_t i = 0; i < result.size(); i++) {
          std::printf(" out%llu=%s\n", static_cast<unsigned long long>(i), hex_or_dash(as_slice(result[i])).c_str());
        }
      } else {
        auto first_result = td::get_word_transliterations(as_slice(word), allow_partial);
        std::string first = join_hex(first_result);
        size_t count = first_result.size();
        bool identical = true;
        for (int repeat = 1; repeat < ONCE_REPEATS; repeat++) {
          auto again = td::get_word_transliterations(as_slice(word), allow_partial);
          count = again.size();
          if (join_hex(again) != first) {
            identical = false;
          }
        }
        std::printf("once in=%s partial=%d repeats=%d identical=%d n=%llu\n", in.c_str(),
                    static_cast<int>(allow_partial), ONCE_REPEATS, static_cast<int>(identical),
                    static_cast<unsigned long long>(count));
        std::printf(" out=%s\n", first.c_str());
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
    std::fprintf(stderr, "usage: translit_probe <script.txt>\n");
    return 2;
  }
  return run_script(argv[1]);
}