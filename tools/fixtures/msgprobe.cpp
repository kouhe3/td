// Oracle for MTProto 2.0 message framing.
// Reads: <auth_key_hex> <plaintext_hex> [X]
// Writes: msg_key, aes_key, aes_iv, ciphertext (all hex, one per line).
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I C:/tmp/td-build/tdutils tools/fixtures/msgprobe.cpp \
//       -Wl,--start-group C:/tmp/td-build/libtdmtproto.a \
//       C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a \
//       -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -liphlpapi -o msgprobe.exe

#include "td/mtproto/KDF.h"

#include "td/utils/common.h"
#include "td/utils/crypto.h"
#include "td/utils/Slice.h"

#include <cstdio>
#include <cstdlib>
#include <string>

namespace {

td::string unhex(const char *s) {
  td::string res;
  size_t len = std::strlen(s);
  for (size_t i = 0; i + 1 < len; i += 2) {
    unsigned int byte = 0;
    std::sscanf(s + i, "%2x", &byte);
    res.push_back(static_cast<char>(byte));
  }
  return res;
}

void print_hex(const char *name, td::Slice data) {
  std::printf("%s=", name);
  for (unsigned char c : data) {
    std::printf("%02x", c);
  }
  std::printf("\n");
}

}  // namespace

int main(int argc, char **argv) {
  if (argc < 3) {
    std::fprintf(stderr, "usage: msgprobe <auth_key_hex> <plaintext_hex> [X]\n");
    return 2;
  }
  td::string auth_key = unhex(argv[1]);
  td::string plaintext = unhex(argv[2]);
  int X = argc > 3 ? std::atoi(argv[3]) : 8;
  if (auth_key.size() != 256) {
    std::fprintf(stderr, "auth_key must be 256 bytes\n");
    return 2;
  }

  // msg_key_large = SHA256(auth_key[88+X .. +32] || plaintext); msg_key = [8..24]
  td::string buf = td::Slice(auth_key).substr(88 + X, 32).str() + plaintext;
  td::uint8 large[32];
  td::sha256(buf, td::MutableSlice(large, 32));
  td::Slice msg_key(td::Slice(large + 8, 16));

  td::UInt256 aes_key;
  td::UInt256 aes_iv;
  td::UInt128 msg_key_int;
  td::as_mutable_slice(msg_key_int).copy_from(msg_key);
  td::mtproto::KDF2(td::Slice(auth_key), msg_key_int, X, &aes_key, &aes_iv);

  auto iv_before = td::as_slice(aes_iv).str();
  td::string encrypted(plaintext.size(), '\0');
  td::aes_ige_encrypt(td::as_slice(aes_key), td::as_mutable_slice(aes_iv), plaintext,
                      td::MutableSlice(encrypted));

  print_hex("msg_key", msg_key);
  print_hex("aes_key", td::as_slice(aes_key));
  print_hex("aes_iv", td::Slice(iv_before));
  print_hex("ciphertext", td::Slice(encrypted));
  return 0;
}