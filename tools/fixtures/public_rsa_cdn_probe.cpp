// Oracle for td/telegram/net/PublicRsaKeySharedCdn: the dynamic CDN key table.
//
//   public_rsa_cdn_probe <script.txt>
//
// Script lines:
//   add <pem_path>        load a PEM and add_rsa(); prints `add ok=1` / `add failed message=…`
//   get <fp1,fp2,...>     get_rsa_key(); prints `get ok=1 fingerprint=…` / `get message=…`
//   drop                  drop_keys()
//   has                   has_keys() -> `has=0|1`
//
// The dc id is external(2), which is what the constructor requires.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/public_rsa_cdn_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a \
//       C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tddb/libtddb.a \
//       C:/tmp/td-build/tdnet/libtdnet.a C:/tmp/td-build/tde2e/libtde2e.a \
//       C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a \
//       C:/tmp/td-build/sqlite/libtdsqlite.a -Wl,--end-group \
//       -lssl -lcrypto -lz -lws2_32 -liphlpapi -lcrypt32 -lpsapi -lmswsock \
//       -lshell32 -o public_rsa_cdn_probe.exe

#include "td/telegram/net/DcId.h"
#include "td/telegram/net/PublicRsaKeySharedCdn.h"

#include "td/mtproto/RSA.h"

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>
#include <vector>

namespace {

std::vector<td::int64> parse_fingerprints(const std::string &text) {
  std::vector<td::int64> out;
  size_t start = 0;
  while (true) {
    size_t comma = text.find(',', start);
    std::string part = text.substr(start, comma == std::string::npos ? std::string::npos : comma - start);
    if (!part.empty()) {
      out.push_back(std::strtoll(part.c_str(), nullptr, 10));
    }
    if (comma == std::string::npos) {
      break;
    }
    start = comma + 1;
  }
  return out;
}

}  // namespace

int main(int argc, char **argv) {
  if (argc != 2) {
    std::fprintf(stderr, "usage: public_rsa_cdn_probe <script.txt>\n");
    return 2;
  }
  td::PublicRsaKeySharedCdn keys(td::DcId::external(2));

  std::ifstream input(argv[1], std::ios::binary);
  std::string text((std::istreambuf_iterator<char>(input)), std::istreambuf_iterator<char>());
  std::istringstream lines(text);
  std::string line;
  while (std::getline(lines, line)) {
    std::istringstream tokens(line);
    std::string op;
    if (!(tokens >> op) || op[0] == '#') {
      continue;
    }
    if (op == "add") {
      std::string path;
      tokens >> path;
      std::ifstream pem_file(path, std::ios::binary);
      std::string pem((std::istreambuf_iterator<char>(pem_file)), std::istreambuf_iterator<char>());
      auto rsa = td::mtproto::RSA::from_pem_public_key(td::Slice(pem));
      if (rsa.is_error()) {
        std::printf("add failed message=%s\n", rsa.error().message().str().c_str());
        continue;
      }
      keys.add_rsa(rsa.move_as_ok());
      std::printf("add ok=1\n");
      continue;
    }
    if (op == "get") {
      std::string list;
      tokens >> list;
      auto result = keys.get_rsa_key(parse_fingerprints(list));
      if (result.is_error()) {
        std::printf("get message=%s\n", result.error().message().str().c_str());
      } else {
        std::printf("get ok=1 fingerprint=%lld\n", static_cast<long long>(result.ok().fingerprint));
      }
      continue;
    }
    if (op == "drop") {
      keys.drop_keys();
      std::printf("dropped\n");
      continue;
    }
    if (op == "has") {
      std::printf("has=%d\n", keys.has_keys() ? 1 : 0);
      continue;
    }
    std::fprintf(stderr, "unknown op %s\n", op.c_str());
    return 2;
  }
  return 0;
}