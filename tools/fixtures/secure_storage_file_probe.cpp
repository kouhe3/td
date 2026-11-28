// Oracle for the file-level half of `td/telegram/SecureStorage.{h,cpp}`:
//
//   secure_storage_file_probe <script.txt> <workdir>
//
// These two functions only touch `FileFd`, `AesCbcState` and the `Encryptor` /
// `Decryptor` state machines, so `libtdcore.a` is enough: no `Td`, no actor, no
// clock (measured on the first pass of this lane). The probe reads the script,
// then chdir()s into <workdir>, so every path in the script and in the narration
// is relative and both implementations print identical error strings.
//
// Commands (`-` is the empty byte string; blank lines and '#' comments ignored):
//   put <file> <hex>                      write raw bytes to <file>
//   enc <name> [src] [dest]               encrypt_file(secret, src, dest)
//   encdec <name>                         encrypt_file + decrypt_file back
//   dec <name> <hex hash> [src] [dest]    decrypt_file; prints len + md5 or the error
//   mkenc <name> <hex payload>            craft a deterministic ciphertext
//   tamper <name>                         flip the last byte of <name>.enc
//
// `enc` reads <name>.in and writes <name>.enc plus a raw 32-byte <name>.hash
// sidecar (the hash is random, because the random prefix is: it is published,
// never narrated). On success it re-decrypts the produced ciphertext in-process
// to report the *structure*: the ciphertext length, its 16-byte alignment, the
// skipped prefix's first byte (== the prefix length, 32..47), whether
// sha256(plaintext) equals the returned hash, and whether the ciphertext
// contains the payload as a substring (only judged from 16 bytes up, where a
// false hit would need a 2^-128 coin).
//
// `mkenc` is the deterministic counterpart of `enc`: a prefix of the same
// length, filled with `i * 37 + 11`, then the payload, hashed and encrypted with
// the same `calc_aes_cbc_state_sha512(secret || hash)`; it publishes the hash
// and writes <name>.enc, so `dec` can be judged on fixed bytes.
//
// Build (`SecureStorage.cpp` lives in libtdcore.a; only tdutils and tdactor are
// needed on top of it):
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I tdactor -I C:/tmp/td-build/tdutils tools/fixtures/secure_storage_file_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi -lshell32
//   -o C:/tmp/td-probe/secure_storage_file_probe.exe
//
// Measured behaviour (docs/facts.md F244 for the primitives): the prefix length
// is `((32 + 15 + n) & -16) - n` and its first byte is that length; the AES key
// is sha512(secret || value_hash) with key = bytes 0..31 and iv = bytes 32..47;
// the ciphertext has no padding on top of the prefix, so its size is always a
// multiple of 16; a one-bit flip in the last ciphertext block is caught by the
// hash check (`Hash mismatch`) *after* the plaintext was already written to the
// destination, and a ciphertext whose size is not a multiple of 16 fails in the
// 128 KiB read loop before anything but the empty destination file is created.

#include "td/telegram/SecureStorage.h"

#include "td/utils/buffer.h"
#include "td/utils/common.h"
#include "td/utils/crypto.h"
#include "td/utils/misc.h"
#include "td/utils/port/path.h"
#include "td/utils/Slice.h"
#include "td/utils/SliceBuilder.h"
#include "td/utils/Status.h"

#include <algorithm>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <iterator>
#include <string>
#include <vector>

namespace {

using td::Slice;
using td::string;
namespace ss = td::secure_storage;

// A valid secret: 00..00 EF has sum 239 and 239 % 255 == 239.
const char *kSecretHex = "00000000000000000000000000000000000000000000000000000000000000ef";

string read_all(const string &path) {
  std::ifstream in(path.c_str(), std::ios::binary);
  if (!in) {
    std::fprintf(stderr, "cannot open %s\n", path.c_str());
    std::exit(2);
  }
  return string(std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>()));
}

void write_all(const string &path, Slice data) {
  std::ofstream out(path.c_str(), std::ios::binary | std::ios::trunc);
  out.write(data.data(), static_cast<std::streamsize>(data.size()));
  if (!out) {
    std::fprintf(stderr, "cannot write %s\n", path.c_str());
    std::exit(2);
  }
}

// "-" when the file does not exist, its size otherwise (an empty file prints 0).
string size_tag(const string &path) {
  std::ifstream in(path.c_str(), std::ios::binary | std::ios::ate);
  if (!in) {
    return string("-");
  }
  return td::to_string(static_cast<long long>(in.tellg()));
}

std::vector<Slice> split_tokens(Slice line) {
  std::vector<Slice> tokens;
  while (!line.empty()) {
    while (!line.empty() && line[0] == ' ') {
      line.remove_prefix(1);
    }
    if (line.empty()) {
      break;
    }
    size_t i = 0;
    while (i < line.size() && line[i] != ' ') {
      i++;
    }
    tokens.push_back(line.substr(0, i));
    line.remove_prefix(i);
  }
  return tokens;
}

Slice trim_line(Slice line) {
  while (!line.empty() && (line.back() == '\r' || line.back() == ' ' || line.back() == '\t')) {
    line.remove_suffix(1);
  }
  while (!line.empty() && (line[0] == ' ' || line[0] == '\t')) {
    line.remove_prefix(1);
  }
  return line;
}

int hex_digit(char c) {
  if (c >= '0' && c <= '9') {
    return c - '0';
  }
  if (c >= 'a' && c <= 'f') {
    return c - 'a' + 10;
  }
  return -1;
}

string unhex(Slice hex) {
  if (hex == "-") {
    return string();
  }
  string out;
  if (hex.size() % 2 != 0) {
    std::fprintf(stderr, "bad hex\n");
    std::exit(2);
  }
  for (size_t i = 0; i < hex.size(); i += 2) {
    out.push_back(static_cast<char>((hex_digit(hex[i]) << 4) | hex_digit(hex[i + 1])));
  }
  return out;
}

string hex(Slice data) {
  return td::hex_encode(data);
}

string md5_hex(Slice data) {
  unsigned char digest[16];
  td::md5(data, td::MutableSlice(digest, sizeof(digest)));
  return hex(Slice(reinterpret_cast<const char *>(digest), sizeof(digest)));
}

bool contains(Slice haystack, Slice needle) {
  if (needle.empty()) {
    return true;
  }
  return std::search(haystack.begin(), haystack.end(), needle.begin(), needle.end()) != haystack.end();
}

td::int64 prefix_size(td::int64 data_size) {
  return ((32 + 15 + data_size) & -16) - data_size;
}

// Decrypts a whole ciphertext (the size is always a multiple of 16 by the time
// this is called), so `enc` can report what it wrote without a second file.
string decrypt_whole(const ss::Secret &secret, Slice hash, Slice cipher) {
  string plain = cipher.str();
  auto state = ss::calc_aes_cbc_state_sha512(PSLICE() << secret.as_slice() << hash);
  state.decrypt(Slice(cipher), td::MutableSlice(plain));
  return plain;
}

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc != 3) {
    std::fprintf(stderr, "usage: secure_storage_file_probe <script.txt> <workdir>\n");
    return 2;
  }
  // The script path is relative to the caller's directory, every path inside
  // the script is relative to <workdir>: read the script, then chdir.
  auto script = read_all(argv[1]);

  auto chdir_status = td::chdir(td::CSlice(argv[2]));
  if (chdir_status.is_error()) {
    std::fprintf(stderr, "cannot chdir into %s: %s\n", argv[2], chdir_status.message().c_str());
    return 2;
  }
  ss::Secret secret = ss::Secret::create(unhex(Slice(kSecretHex))).move_as_ok();

  for (auto raw_line : td::full_split(Slice(script), '\n')) {
    auto line = trim_line(raw_line);
    if (line.empty() || line[0] == '#') {
      continue;
    }
    auto tokens = split_tokens(line);
    auto command = tokens[0];
    if (command == "put") {
      auto file = tokens[1].str();
      auto data = unhex(tokens[2]);
      write_all(file, data);
      std::printf("put %s len=%zu\n", file.c_str(), data.size());
    } else if (command == "enc") {
      auto name = tokens[1].str();
      auto src = tokens.size() > 2 ? tokens[2].str() : name + ".in";
      auto dest = tokens.size() > 3 ? tokens[3].str() : name + ".enc";
      auto result = ss::encrypt_file(secret, src, dest);
      if (result.is_error()) {
        std::printf("enc %s err=%s dst=%s\n", name.c_str(), result.error().message().str().c_str(),
                    size_tag(dest).c_str());
        continue;
      }
      auto hash = result.move_as_ok();
      write_all(name + ".hash", hash.as_slice());
      auto cipher = read_all(dest);
      auto plain = decrypt_whole(secret, hash.as_slice(), cipher);
      auto payload = read_all(src);
      unsigned char recomputed[32];
      td::sha256(Slice(plain), td::MutableSlice(recomputed, sizeof(recomputed)));
      auto stored = Slice(reinterpret_cast<const char *>(recomputed), sizeof(recomputed)) == hash.as_slice();
      std::printf("enc %s src=%zu dst=%zu aligned=%d prefix=%d stored=%d contains=%s\n", name.c_str(),
                  payload.size(), cipher.size(), cipher.size() % 16 == 0 ? 1 : 0,
                  plain.empty() ? -1 : static_cast<int>(static_cast<td::uint8>(plain[0])), stored ? 1 : 0,
                  payload.size() < 16
                      ? "-"
                      : (contains(Slice(cipher), Slice(payload)) ? "1" : "0"));
    } else if (command == "encdec") {
      auto name = tokens[1].str();
      auto src = name + ".in";
      auto enc_result = ss::encrypt_file(secret, src, name + ".enc");
      if (enc_result.is_error()) {
        std::printf("encdec %s err=enc:%s\n", name.c_str(), enc_result.error().message().str().c_str());
        continue;
      }
      auto hash = enc_result.move_as_ok();
      auto dec_status = ss::decrypt_file(secret, hash, name + ".enc", name + ".dec");
      if (dec_status.is_error()) {
        std::printf("encdec %s err=dec:%s\n", name.c_str(), dec_status.message().str().c_str());
        continue;
      }
      auto back = read_all(name + ".dec");
      auto payload = read_all(src);
      std::printf("encdec %s same=%d len=%zu md5=%s\n", name.c_str(),
                  Slice(back) == Slice(payload) ? 1 : 0, back.size(),
                  md5_hex(Slice(back)).c_str());
    } else if (command == "dec") {
      auto name = tokens[1].str();
      auto src = tokens.size() > 3 ? tokens[3].str() : name + ".enc";
      auto dest = tokens.size() > 4 ? tokens[4].str() : name + ".dec";
      auto hash_result = ss::ValueHash::create(unhex(tokens[2]));
      if (hash_result.is_error()) {
        std::printf("dec %s err=%s leftover=%s\n", name.c_str(),
                    hash_result.error().message().str().c_str(), size_tag(dest).c_str());
        continue;
      }
      auto &hash = hash_result.ok();
      auto status = ss::decrypt_file(secret, hash, src, dest);
      if (status.is_error()) {
        std::printf("dec %s err=%s leftover=%s\n", name.c_str(), status.message().str().c_str(),
                    size_tag(dest).c_str());
        continue;
      }
      auto back = read_all(dest);
      std::printf("dec %s ok len=%zu md5=%s\n", name.c_str(), back.size(), md5_hex(Slice(back)).c_str());
    } else if (command == "mkenc") {
      auto name = tokens[1].str();
      auto payload = unhex(tokens[2]);
      auto size = static_cast<td::int64>(payload.size());
      auto prefix = prefix_size(size);
      string plain(static_cast<size_t>(prefix), '\0');
      for (size_t i = 1; i < plain.size(); i++) {
        plain[i] = static_cast<char>((i * 37 + 11) & 0xff);
      }
      plain[0] = static_cast<char>(prefix);
      plain += payload;

      auto hash = ss::calc_value_hash(Slice(plain));
      auto state = ss::calc_aes_cbc_state_sha512(PSLICE() << secret.as_slice() << hash.as_slice());
      string cipher = plain;
      state.encrypt(Slice(plain), td::MutableSlice(cipher));
      write_all(name + ".enc", cipher);
      write_all(name + ".hash", hash.as_slice());
      std::printf("mkenc %s hash=%s size=%zu prefix=%lld\n", name.c_str(),
                  hex(hash.as_slice()).c_str(), cipher.size(), static_cast<long long>(prefix));
    } else if (command == "tamper") {
      auto name = tokens[1].str();
      auto cipher = read_all(name + ".enc");
      if (cipher.empty()) {
        std::printf("tamper %s size=0 empty=1\n", name.c_str());
        continue;
      }
      cipher.back() = static_cast<char>(static_cast<td::uint8>(cipher.back()) ^ 1);
      write_all(name + ".enc", cipher);
      std::printf("tamper %s size=%zu\n", name.c_str(), cipher.size());
    } else {
      std::fprintf(stderr, "unknown command %s\n", command.str().c_str());
      return 2;
    }
  }
  return 0;
}