// Oracle for the Web-proxy helpers in td/telegram/net/Proxy.cpp.
//
//   web_proxy_probe secret <encoded_secret>
//   web_proxy_probe link <link_secret> <has_path:0|1>
//   web_proxy_probe proxy <server> <path> <secret_hex>
//
// `secret` and `link` print status=ok raw=<hex> or status=err code/message;
// `proxy` prints the three derived strings as hex:
//   address=<hex> link=<hex> bridge=<hex>
//
// argv is read from the wide command line and converted to UTF-8, because
// `main`'s argv is in the ANSI code page (F119).
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/web_proxy_probe.cpp -Wl,--start-group C:/tmp/td-build/libtdcore.a \
//       C:/tmp/td-build/libtdapi.a C:/tmp/td-build/libtdmtproto.a \
//       C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdnet/libtdnet.a \
//       C:/tmp/td-build/tde2e/libtde2e.a C:/tmp/td-build/tdutils/libtdutils.a \
//       C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/sqlite/libtdsqlite.a \
//       -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -liphlpapi -lcrypt32 \
//       -lpsapi -lmswsock -lshell32 -o web_proxy_probe.exe

#include "td/telegram/net/Proxy.h"

#include "td/mtproto/ProxySecret.h"

#include "td/utils/Slice.h"
#include "td/utils/common.h"

#include <cstdio>
#include <cstdlib>
#include <string>

#include <windows.h>
#include <shellapi.h>

namespace {

std::string to_utf8(const wchar_t *wide) {
  int size = WideCharToMultiByte(CP_UTF8, 0, wide, -1, nullptr, 0, nullptr, nullptr);
  std::string result(size > 0 ? size - 1 : 0, '\0');
  if (size > 0) {
    WideCharToMultiByte(CP_UTF8, 0, wide, -1, &result[0], size, nullptr, nullptr);
  }
  return result;
}

td::string unhex(const std::string &text) {
  td::string out;
  for (size_t i = 0; i + 1 < text.size(); i += 2) {
    unsigned int byte = 0;
    std::sscanf(text.c_str() + i, "%2x", &byte);
    out.push_back(static_cast<char>(byte));
  }
  return out;
}

void print_hex(const char *name, td::Slice data) {
  std::printf("%s=", name);
  for (unsigned char c : data) {
    std::printf("%02x", c);
  }
}

void print_error(const td::Status &status) {
  std::printf("status=err code=%d message=%s\n", status.code(), status.message().str().c_str());
}

}  // namespace

int main() {
  int argc = 0;
  auto wide_argv = CommandLineToArgvW(GetCommandLineW(), &argc);
  if (wide_argv == nullptr) {
    return 2;
  }
  std::string *argv = new std::string[argc + 1];
  for (int i = 0; i < argc; i++) {
    argv[i] = to_utf8(wide_argv[i]);
  }
  LocalFree(wide_argv);
  if (argc < 3) {
    std::fprintf(stderr, "usage: web_proxy_probe secret|link|proxy ...\n");
    return 2;
  }
  std::string mode = argv[1];
  if (mode == "secret") {
    auto secret = td::Proxy::get_web_proxy_secret(argv[2]);
    if (secret.is_error()) {
      print_error(secret.error());
      return 0;
    }
    std::printf("status=ok ");
    print_hex("raw", secret.ok().get_raw_secret());
    std::printf("\n");
    return 0;
  }
  if (mode == "link" && argc >= 4) {
    auto secret = td::Proxy::get_web_proxy_link_secret(argv[2], std::atoi(argv[3].c_str()) != 0);
    if (secret.is_error()) {
      print_error(secret.error());
      return 0;
    }
    std::printf("status=ok ");
    print_hex("raw", secret.ok().get_raw_secret());
    std::printf("\n");
    return 0;
  }
  if (mode == "proxy" && argc >= 5) {
    auto secret = td::mtproto::ProxySecret::from_raw(td::Slice(unhex(argv[4])));
    auto proxy = td::Proxy::web(argv[2], argv[3], secret);
    std::printf("status=ok ");
    print_hex("address", proxy.get_web_proxy_address());
    std::printf(" ");
    print_hex("link", proxy.get_web_proxy_link_secret());
    std::printf(" ");
    print_hex("bridge", proxy.get_web_proxy_bridge_capability());
    std::printf("\n");
    return 0;
  }
  std::fprintf(stderr, "bad arguments for mode %s\n", mode.c_str());
  return 2;
}