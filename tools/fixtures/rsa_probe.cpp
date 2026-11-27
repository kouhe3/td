// Oracle for td/mtproto/RSA.cpp.
//
//   rsa_probe <pem_file> load
//   rsa_probe <pem_file> fingerprint
//   rsa_probe <pem_file> encrypt <hex_256_bytes>
//   rsa_probe <pem_file> decrypt <hex_256_bytes>
//
// The key comes from RSA::from_pem_public_key(), so every path goes through the
// same OpenSSL validation. `encrypt` and `decrypt_signature` are plain
// x^e mod n, so both sides must agree byte for byte.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/rsa_probe.cpp -Wl,--start-group C:/tmp/td-build/libtdcore.a \
//       C:/tmp/td-build/libtdapi.a C:/tmp/td-build/libtdmtproto.a \
//       C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdnet/libtdnet.a \
//       C:/tmp/td-build/tde2e/libtde2e.a C:/tmp/td-build/tdutils/libtdutils.a \
//       C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/sqlite/libtdsqlite.a \
//       -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -liphlpapi -lcrypt32 \
//       -lpsapi -lmswsock -lshell32 -o rsa_probe.exe

#include "td/mtproto/RSA.h"

#include "td/utils/Slice.h"

#include <cstdio>
#include <cstring>
#include <fstream>
#include <iterator>
#include <string>

namespace {

std::string unhex(const char *text) {
  std::string out;
  size_t len = std::strlen(text);
  for (size_t i = 0; i + 1 < len; i += 2) {
    unsigned int byte = 0;
    std::sscanf(text + i, "%2x", &byte);
    out.push_back(static_cast<char>(byte));
  }
  return out;
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
    std::fprintf(stderr, "usage: rsa_probe <pem_file> <load|fingerprint|encrypt|decrypt> [hex]\n");
    return 2;
  }
  std::ifstream input(argv[1], std::ios::binary);
  std::string pem((std::istreambuf_iterator<char>(input)), std::istreambuf_iterator<char>());
  auto result = td::mtproto::RSA::from_pem_public_key(td::Slice(pem));
  if (result.is_error()) {
    std::printf("message=%s\n", result.error().message().str().c_str());
    return 0;
  }
  auto rsa = result.move_as_ok();

  std::string mode = argv[2];
  if (mode == "load") {
    std::printf("ok=1\n");
    return 0;
  }
  if (mode == "fingerprint") {
    std::printf("ok=1 fingerprint=%lld size=%zu\n", static_cast<long long>(rsa.get_fingerprint()), rsa.size());
    return 0;
  }
  if (argc < 4) {
    std::fprintf(stderr, "%s needs a hex payload\n", mode.c_str());
    return 2;
  }
  std::string from = unhex(argv[3]);
  if (from.size() != 256) {
    std::fprintf(stderr, "payload must be 256 bytes, got %zu\n", from.size());
    return 2;
  }
  std::string out(256, '\0');
  if (mode == "encrypt") {
    bool ok = rsa.encrypt(td::Slice(from), td::MutableSlice(&out[0], out.size()));
    std::printf("ok=%d\n", ok ? 1 : 0);
    if (ok) {
      print_hex("out", td::Slice(out));
    }
    return 0;
  }
  if (mode == "decrypt") {
    rsa.decrypt_signature(td::Slice(from), td::MutableSlice(&out[0], out.size()));
    print_hex("out", td::Slice(out));
    return 0;
  }
  std::fprintf(stderr, "unknown mode %s\n", mode.c_str());
  return 2;
}