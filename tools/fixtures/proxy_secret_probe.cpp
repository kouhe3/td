// Oracle for td::mtproto::ProxySecret: MTProxy secret validation and encoding.
//
//   proxy_secret_probe link <text> [--truncate]
//   proxy_secret_probe binary <hex> [--truncate]
//   proxy_secret_probe encoded <hex>     (from_raw, then get_encoded_secret)
//   proxy_secret_probe derived <hex>     (from_raw, then proxy/padding/tls/domain)
//
// Every accepted secret prints the same line:
//   status=ok raw=<hex> proxy=<hex> encoded=<hex-of-the-encoded-string> padding=<0|1> tls=<0|1>
// and ` domain=<hex>` when it emulates TLS (get_domain() CHECKs emulate_tls()).
// Rejected secrets print: status=err code=<n> message=<text>
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I C:/tmp/td-build/tdutils tools/fixtures/proxy_secret_probe.cpp \
//       -Wl,--start-group C:/tmp/td-build/libtdmtproto.a \
//       C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a \
//       -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -liphlpapi -o proxy_secret_probe.exe

#include "td/mtproto/ProxySecret.h"

#include "td/utils/Slice.h"
#include "td/utils/common.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
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
  std::printf(" ");
}

void print_secret(const td::mtproto::ProxySecret &secret) {
  print_hex("raw", secret.get_raw_secret());
  print_hex("proxy", secret.get_proxy_secret());
  print_hex("encoded", secret.get_encoded_secret());
  std::printf("padding=%d tls=%d", secret.use_random_padding() ? 1 : 0, secret.emulate_tls() ? 1 : 0);
  if (secret.emulate_tls()) {
    std::printf(" ");
    print_hex("domain", secret.get_domain());
  }
  std::printf("\n");
}

void print_error(const td::Status &status) {
  std::printf("status=err code=%d message=%s\n", status.code(), status.message().str().c_str());
}

}  // namespace

int main(int argc, char **argv) {
  if (argc < 3) {
    std::fprintf(stderr, "usage: proxy_secret_probe <link|binary|encoded|derived> <value> [--truncate]\n");
    return 2;
  }
  std::string mode = argv[1];
  bool truncate = argc >= 4 && std::strcmp(argv[3], "--truncate") == 0;

  if (mode == "link") {
    auto result = td::mtproto::ProxySecret::from_link(td::Slice(argv[2]), truncate);
    if (result.is_error()) {
      print_error(result.error());
      return 0;
    }
    std::printf("status=ok ");
    print_secret(result.ok());
    return 0;
  }
  if (mode == "binary") {
    auto bytes = unhex(argv[2]);
    auto result = td::mtproto::ProxySecret::from_binary(td::Slice(bytes), truncate);
    if (result.is_error()) {
      print_error(result.error());
      return 0;
    }
    std::printf("status=ok ");
    print_secret(result.ok());
    return 0;
  }
  if (mode == "encoded" || mode == "derived") {
    auto bytes = unhex(argv[2]);
    auto secret = td::mtproto::ProxySecret::from_raw(td::Slice(bytes));
    std::printf("status=ok ");
    print_secret(secret);
    return 0;
  }
  std::fprintf(stderr, "unknown mode %s\n", mode.c_str());
  return 2;
}