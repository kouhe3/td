// Oracle for the crypto primitives behind AesCtrEncryptionEvent, taken from
// td/utils/crypto.h under this build configuration (TD_HAVE_OPENSSL 1, OpenSSL
// 4.1.0, TD_HAVE_ZLIB 1):
//
//   pbkdf2_sha256, hmac_sha256, crc32, AesCtrState (init + encrypt).
//
//   crypto_probe <script.txt>
//
// Script lines, every payload hex-encoded with `-` for the empty string:
//
//   pbkdf2 <password> <salt> <iterations> <outsize>
//   hmac <key> <message>
//   crc32 <data>
//   ctr <key> <iv> <data>
//   ctrsplit <key> <iv> <part1> [part2 ...]
//
// `ctrsplit` inits one AesCtrState and feeds the parts through separate
// encrypt() calls, printing the concatenated ciphertext; it is the resume
// check for the keystream state.
//
// Output, one line per command, all hex:
//
//   pbkdf2 it=<n> out=<dest>
//   hmac out=<32 bytes>
//   crc32 v=<8 hex digits>
//   ctr n=<bytes> out=<ciphertext>
//   ctrsplit n=<bytes> parts=<count> out=<ciphertext>
//
// Preconditions are the td CHECKs, so invalid inputs abort the process:
// pbkdf2 needs outsize == 32 (`CHECK(dest.size() == 32)`, crypto.cpp:944) and
// iterations > 0 (`CHECK(iteration_count > 0)`, crypto.cpp:945); AesCtrState
// needs a 32-byte key and a 16-byte iv (crypto.cpp:683-684).
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/crypto_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group \
//       -lssl -lcrypto -lz -lws2_32 -lpsapi -o C:/tmp/td-probe/crypto_probe.exe

#include "td/utils/crypto.h"

#include <cstdint>
#include <cstdio>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>
#include <vector>

namespace {

int hex_digit(char c) {
  if (c >= '0' && c <= '9') {
    return c - '0';
  }
  if (c >= 'a' && c <= 'f') {
    return c - 'a' + 10;
  }
  if (c >= 'A' && c <= 'F') {
    return c - 'A' + 10;
  }
  return -1;
}

bool parse_hex(const std::string &token, std::string *out) {
  out->clear();
  if (token == "-") {
    return true;
  }
  if (token.size() % 2 != 0) {
    return false;
  }
  for (size_t i = 0; i < token.size(); i += 2) {
    int high = hex_digit(token[i]);
    int low = hex_digit(token[i + 1]);
    if (high < 0 || low < 0) {
      return false;
    }
    out->push_back(static_cast<char>((high << 4) | low));
  }
  return true;
}

std::string to_hex(const std::string &bytes) {
  static const char *digits = "0123456789abcdef";
  std::string result;
  result.reserve(bytes.size() * 2);
  for (unsigned char byte : bytes) {
    result.push_back(digits[byte >> 4]);
    result.push_back(digits[byte & 15]);
  }
  return result;
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
  int line_number = 0;
  while (std::getline(lines, line)) {
    line_number++;
    std::istringstream tokens(line);
    std::string op;
    if (!(tokens >> op) || op[0] == '#') {
      continue;
    }
    if (op == "pbkdf2") {
      std::string password_hex;
      std::string salt_hex;
      int iterations = 0;
      int outsize = 0;
      std::string password;
      std::string salt;
      if (!(tokens >> password_hex >> salt_hex >> iterations >> outsize) || !parse_hex(password_hex, &password) ||
          !parse_hex(salt_hex, &salt)) {
        std::fprintf(stderr, "line %d: bad pbkdf2 arguments\n", line_number);
        return 2;
      }
      std::string dest(static_cast<size_t>(outsize), '\0');
      td::pbkdf2_sha256(td::Slice(password), td::Slice(salt), iterations,
                        td::MutableSlice(dest.data(), dest.size()));
      std::printf("pbkdf2 it=%d out=%s\n", iterations, to_hex(dest).c_str());
    } else if (op == "hmac") {
      std::string key_hex;
      std::string message_hex;
      std::string key;
      std::string message;
      if (!(tokens >> key_hex >> message_hex) || !parse_hex(key_hex, &key) || !parse_hex(message_hex, &message)) {
        std::fprintf(stderr, "line %d: bad hmac arguments\n", line_number);
        return 2;
      }
      std::string dest(32, '\0');
      td::hmac_sha256(td::Slice(key), td::Slice(message), td::MutableSlice(dest.data(), dest.size()));
      std::printf("hmac out=%s\n", to_hex(dest).c_str());
    } else if (op == "crc32") {
      std::string data_hex;
      std::string data;
      if (!(tokens >> data_hex) || !parse_hex(data_hex, &data)) {
        std::fprintf(stderr, "line %d: bad crc32 arguments\n", line_number);
        return 2;
      }
      std::printf("crc32 v=%08x\n", static_cast<unsigned>(td::crc32(td::Slice(data))));
    } else if (op == "ctr") {
      std::string key_hex;
      std::string iv_hex;
      std::string data_hex;
      std::string key;
      std::string iv;
      std::string data;
      if (!(tokens >> key_hex >> iv_hex >> data_hex) || !parse_hex(key_hex, &key) || !parse_hex(iv_hex, &iv) ||
          !parse_hex(data_hex, &data)) {
        std::fprintf(stderr, "line %d: bad ctr arguments\n", line_number);
        return 2;
      }
      td::AesCtrState state;
      state.init(td::Slice(key), td::Slice(iv));
      std::string ciphertext(data.size(), '\0');
      state.encrypt(td::Slice(data), td::MutableSlice(ciphertext.data(), ciphertext.size()));
      std::printf("ctr n=%u out=%s\n", static_cast<unsigned>(data.size()), to_hex(ciphertext).c_str());
    } else if (op == "ctrsplit") {
      std::string key_hex;
      std::string iv_hex;
      std::string key;
      std::string iv;
      if (!(tokens >> key_hex >> iv_hex) || !parse_hex(key_hex, &key) || !parse_hex(iv_hex, &iv)) {
        std::fprintf(stderr, "line %d: bad ctrsplit arguments\n", line_number);
        return 2;
      }
      td::AesCtrState state;
      state.init(td::Slice(key), td::Slice(iv));
      std::string ciphertext;
      std::string part_hex;
      size_t parts = 0;
      while (tokens >> part_hex) {
        std::string part;
        if (!parse_hex(part_hex, &part)) {
          std::fprintf(stderr, "line %d: bad ctrsplit part\n", line_number);
          return 2;
        }
        parts++;
        std::string out(part.size(), '\0');
        state.encrypt(td::Slice(part), td::MutableSlice(out.data(), out.size()));
        ciphertext += out;
      }
      if (parts == 0) {
        std::fprintf(stderr, "line %d: ctrsplit needs at least one part\n", line_number);
        return 2;
      }
      std::printf("ctrsplit n=%u parts=%u out=%s\n", static_cast<unsigned>(ciphertext.size()),
                  static_cast<unsigned>(parts), to_hex(ciphertext).c_str());
    } else {
      std::fprintf(stderr, "line %d: unknown command %s\n", line_number, op.c_str());
      return 2;
    }
  }
  return 0;
}

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc != 2) {
    std::fprintf(stderr, "usage: crypto_probe <script.txt>\n");
    return 2;
  }
  td::init_crypto();
  return run_script(argv[1]);
}