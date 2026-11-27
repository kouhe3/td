// Oracle for DhHandshake::check_config(): the DH-parameter gate in
// td/mtproto/DhHandshake.cpp.
//
//   dh_handshake_probe <g_int> <prime_hex>
//
// `prime_hex` is the big-endian modulus, hex encoded. The probe passes a null
// DhCallback, so the cache branch is skipped and every call re-runs the primality
// tests.
//
// Output: `ok=1` or `message=<text>` (every failure is Status::Error(Slice),
// whose code is 0).
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/dh_handshake_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a \
//       C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tddb/libtddb.a \
//       C:/tmp/td-build/tdnet/libtdnet.a C:/tmp/td-build/tde2e/libtde2e.a \
//       C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a \
//       C:/tmp/td-build/sqlite/libtdsqlite.a -Wl,--end-group \
//       -lssl -lcrypto -lz -lws2_32 -liphlpapi -lcrypt32 -lpsapi -lmswsock \
//       -lshell32 -o dh_handshake_probe.exe

#include "td/mtproto/DhHandshake.h"

#include "td/utils/Slice.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
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

}  // namespace

int main(int argc, char **argv) {
  if (argc != 3) {
    std::fprintf(stderr, "usage: dh_handshake_probe <g_int> <prime_hex>\n");
    return 2;
  }
  auto g_int = static_cast<td::int32>(std::strtol(argv[1], nullptr, 10));
  auto prime = unhex(argv[2]);
  auto status = td::mtproto::DhHandshake::check_config(g_int, td::Slice(prime), nullptr);
  if (status.is_error()) {
    std::printf("message=%s\n", status.message().str().c_str());
  } else {
    std::printf("ok=1\n");
  }
  return 0;
}