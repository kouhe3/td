// Oracle for the RPC request header (invokeWithLayer + initConnection).
//
// rpc_probe <api_id> <device_model> <system_version> <app_version> <lang_code>
//   -> header=<hex>
//
// TDLib builds this by hand in td/telegram/net/MtprotoHeader.cpp; the class
// exposes the finished bytes, so the oracle is the real code path.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/rpc_probe.cpp -Wl,--start-group C:/tmp/td-build/libtdcore.a \
//       C:/tmp/td-build/libtdapi.a C:/tmp/td-build/libtdmtproto.a \
//       C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdnet/libtdnet.a \
//       C:/tmp/td-build/tde2e/libtde2e.a C:/tmp/td-build/tdutils/libtdutils.a \
//       C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/sqlite/libtdsqlite.a \
//       -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -liphlpapi -lcrypt32 \
//       -lpsapi -lmswsock -o rpc_probe.exe

#include "td/telegram/net/MtprotoHeader.h"

#include "td/utils/Slice.h"
#include "td/utils/common.h"

#include <cstdio>
#include <cstdlib>

int main(int argc, char **argv) {
  if (argc < 6) {
    std::fprintf(stderr, "usage: rpc_probe <api_id> <device_model> <system_version> <app_version> <lang_code>\n");
    return 2;
  }
  td::MtprotoHeader::Options options;
  options.api_id = std::atoi(argv[1]);
  options.system_language_code = argv[5];
  options.device_model = argv[2];
  options.system_version = argv[3];
  options.application_version = argv[4];
  options.language_code = argv[5];

  td::MtprotoHeader header(options);
  auto data = header.get_default_header();
  std::printf("header=");
  for (unsigned char c : data) {
    std::printf("%02x", c);
  }
  std::printf("\n");
  return 0;
}