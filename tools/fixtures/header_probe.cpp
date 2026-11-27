// Oracle for MtprotoHeader across its option matrix (default + anonymous).
//
// header_probe <api_id> <device_model> <system_version> <app_version>
//              <system_lang_code> <language_pack> <language_code> <tz_offset>
//              <emulator:0|1> <proxy:none|socks5|http|mtproto|web> [server] [port]
//
// Prints default=<hex> and anonymous=<hex>. Empty `parameters` only: the params
// field is then an empty jsonObject plus the tz_offset entry.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/header_probe.cpp -Wl,--start-group C:/tmp/td-build/libtdcore.a \
//       C:/tmp/td-build/libtdapi.a C:/tmp/td-build/libtdmtproto.a \
//       C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdnet/libtdnet.a \
//       C:/tmp/td-build/tde2e/libtde2e.a C:/tmp/td-build/tdutils/libtdutils.a \
//       C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/sqlite/libtdsqlite.a \
//       -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -liphlpapi -lcrypt32 \
//       -lpsapi -lmswsock -o header_probe.exe

#include "td/telegram/net/MtprotoHeader.h"
#include "td/telegram/net/Proxy.h"

#include "td/mtproto/ProxySecret.h"

#include "td/utils/Slice.h"
#include "td/utils/common.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

#include <windows.h>
#include <shellapi.h>

namespace {

// `main`'s argv is in the ANSI code page (GBK here), while the Rust probe sees
// UTF-8. Read the wide command line and convert, so both sides get the same
// bytes for non-ASCII device/language strings.
std::string to_utf8(const wchar_t *wide) {
  int size = WideCharToMultiByte(CP_UTF8, 0, wide, -1, nullptr, 0, nullptr, nullptr);
  std::string result(size > 0 ? size - 1 : 0, '\0');
  if (size > 0) {
    WideCharToMultiByte(CP_UTF8, 0, wide, -1, &result[0], size, nullptr, nullptr);
  }
  return result;
}

void print_header(const char *name, const td::string &header) {
  std::printf("%s=", name);
  for (unsigned char c : header) {
    std::printf("%02x", c);
  }
  std::printf("\n");
}

td::Proxy make_proxy(const char *kind, const char *server, td::int32 port) {
  if (std::strcmp(kind, "socks5") == 0) {
    return td::Proxy::socks5(server, port, "", "");
  }
  if (std::strcmp(kind, "http") == 0) {
    return td::Proxy::http_tcp(server, port, "", "");
  }
  auto secret = td::mtproto::ProxySecret::from_raw(td::Slice(td::string(16, '\0')));
  if (std::strcmp(kind, "mtproto") == 0) {
    return td::Proxy::mtproto(server, port, secret);
  }
  if (std::strcmp(kind, "web") == 0) {
    return td::Proxy::web(server, "/secret", secret);
  }
  return td::Proxy();
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
  if (argc < 11) {
    std::fprintf(stderr, "usage: header_probe <api_id> <device> <system> <app> <sys_lang> <lang_pack> <lang_code> "
                         "<tz_offset> <emulator> <proxy> [server] [port]\n");
    return 2;
  }
  td::MtprotoHeader::Options options;
  options.api_id = std::atoi(argv[1].c_str());
  options.device_model = argv[2];
  options.system_version = argv[3];
  options.application_version = argv[4];
  options.system_language_code = argv[5];
  options.language_pack = argv[6];
  options.language_code = argv[7];
  options.tz_offset = std::atoi(argv[8].c_str());
  options.is_emulator = std::atoi(argv[9].c_str()) != 0;
  std::string server = argc >= 12 ? argv[11] : "127.0.0.1";
  td::int32 port = argc >= 13 ? std::atoi(argv[12].c_str()) : 1080;
  options.proxy = make_proxy(argv[10].c_str(), server.c_str(), port);

  td::MtprotoHeader header(options);
  print_header("default", header.get_default_header());
  print_header("anonymous", header.get_anonymous_header());
  delete[] argv;
  return 0;
}