// Oracle for TlsInit's ClientHello: prints the bytes TDLib would put on the
// wire, one line per run.
//
//   tls_init_probe <domain> <secret_hex> <unix_time> <runs>
//
// `<secret_hex>` is 32 hex chars (the 16-byte proxy secret). `<unix_time>` is
// an int32: it is XORed into the last four bytes of the ClientHello random.
//
// TlsObfusaction only exists inside td/mtproto/TlsInit.cpp, so this probe
// includes that translation unit directly and links with
// -Wl,--allow-multiple-definition (libtdcore carries the same objects).
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/tls_init_probe.cpp -Wl,--allow-multiple-definition \
//       -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a \
//       C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tddb/libtddb.a \
//       C:/tmp/td-build/tdnet/libtdnet.a C:/tmp/td-build/tde2e/libtde2e.a \
//       C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a \
//       C:/tmp/td-build/sqlite/libtdsqlite.a -Wl,--end-group \
//       -lssl -lcrypto -lz -lws2_32 -liphlpapi -lcrypt32 -lpsapi -lmswsock \
//       -lshell32 -o tls_init_probe.exe

#include "td/mtproto/TlsInit.cpp"

#include "td/utils/misc.h"

#include <cstdio>
#include <cstdlib>
#include <string>

int main(int argc, char **argv) {
  if (argc != 5) {
    std::fprintf(stderr, "usage: tls_init_probe <domain> <secret_hex> <unix_time> <runs>\n");
    return 2;
  }
  std::string domain = argv[1];
  auto secret_result = td::hex_decode(td::Slice(argv[2]));
  if (secret_result.is_error()) {
    std::fprintf(stderr, "bad secret: %s\n", secret_result.error().message().str().c_str());
    return 2;
  }
  auto secret = secret_result.move_as_ok();
  if (secret.size() != 16) {
    std::fprintf(stderr, "secret must be 16 bytes, got %zu\n", secret.size());
    return 2;
  }
  auto unix_time = static_cast<td::int32>(std::strtol(argv[3], nullptr, 10));
  int runs = std::atoi(argv[4]);

  for (int i = 0; i < runs; i++) {
    auto hello =
        td::mtproto::TlsObfusaction::generate_header(domain, td::Slice(secret), unix_time);
    std::printf("hello len=%zu %s\n", hello.size(), td::hex_encode(hello).c_str());
  }
  return 0;
}