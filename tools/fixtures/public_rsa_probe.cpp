// Oracle for the built-in RSA key table: td/telegram/net/PublicRsaKeySharedMain.
//
//   public_rsa_probe <main|test> <fp1,fp2,...>
//
// Prints `ok=1 fingerprint=<matched>` or the `Unknown Main fingerprints [...]`
// message. The fingerprint list may be empty (argv[2] == "").
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/public_rsa_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a \
//       C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tddb/libtddb.a \
//       C:/tmp/td-build/tdnet/libtdnet.a C:/tmp/td-build/tde2e/libtde2e.a \
//       C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a \
//       C:/tmp/td-build/sqlite/libtdsqlite.a -Wl,--end-group \
//       -lssl -lcrypto -lz -lws2_32 -liphlpapi -lcrypt32 -lpsapi -lmswsock \
//       -lshell32 -o public_rsa_probe.exe

#include "td/telegram/net/PublicRsaKeySharedMain.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

int main(int argc, char **argv) {
  if (argc < 3) {
    std::fprintf(stderr, "usage: public_rsa_probe <main|test> <fp1,fp2,...>\n");
    return 2;
  }
  bool is_test = std::strcmp(argv[1], "test") == 0;
  auto keys = td::PublicRsaKeySharedMain::create(is_test);

  std::vector<td::int64> fingerprints;
  std::string list = argv[2];
  size_t start = 0;
  while (true) {
    size_t comma = list.find(',', start);
    std::string part = list.substr(start, comma == std::string::npos ? std::string::npos : comma - start);
    if (!part.empty()) {
      fingerprints.push_back(std::strtoll(part.c_str(), nullptr, 10));
    }
    if (comma == std::string::npos) {
      break;
    }
    start = comma + 1;
  }

  auto result = keys->get_rsa_key(fingerprints);
  if (result.is_error()) {
    std::printf("message=%s\n", result.error().message().str().c_str());
  } else {
    std::printf("ok=1 fingerprint=%lld\n", static_cast<long long>(result.ok().fingerprint));
  }
  return 0;
}