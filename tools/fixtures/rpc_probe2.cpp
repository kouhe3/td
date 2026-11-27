// Oracle for rpc_error#2144ca19 (store and parse).
//
// rpc_probe2 store <code> <message>   -> bytes=<hex>
// rpc_probe2 parse <bytes_hex>        -> code=, message=
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/rpc_probe2.cpp -Wl,--start-group C:/tmp/td-build/libtdmtproto.a \
//       C:/tmp/td-build/libtdapi.a C:/tmp/td-build/tdutils/libtdutils.a \
//       C:/tmp/td-build/tdactor/libtdactor.a -Wl,--end-group -lssl -lcrypto -lz \
//       -lws2_32 -liphlpapi -lcrypt32 -lpsapi -lmswsock -o rpc_probe2.exe

#include "td/mtproto/mtproto_api.h"

#include "td/utils/Slice.h"
#include "td/utils/common.h"
#include "td/utils/tl_parsers.h"
#include "td/utils/tl_storers.h"

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
  std::printf("\n");
}

}  // namespace

int main(int argc, char **argv) {
  if (argc < 3) {
    std::fprintf(stderr, "usage: rpc_probe2 store <code> <message> | parse <bytes_hex>\n");
    return 2;
  }
  std::string mode = argv[1];
  if (mode == "store") {
    if (argc < 4) {
      std::fprintf(stderr, "store needs <code> <message>\n");
      return 2;
    }
    td::int32 code = std::atoi(argv[2]);
    auto object = td::make_tl_object<td::mtproto_api::rpc_error>(code, std::string(argv[3]));
    td::TlStorerCalcLength calc;
    object->store(calc);
    std::string bytes(calc.get_length(), '\0');
    td::TlStorerUnsafe storer(reinterpret_cast<td::uint8 *>(bytes.data()));
    object->store(storer);
    print_hex("bytes", td::Slice(bytes));
    return 0;
  }
  if (mode == "parse") {
    auto bytes = unhex(argv[2]);
    td::TlParser parser{td::Slice(bytes)};
    auto object = td::mtproto_api::rpc_error::fetch(parser);
    parser.fetch_end();
    if (parser.get_error() != nullptr) {
      std::fprintf(stderr, "parse error: %s\n", parser.get_error());
      return 1;
    }
    std::printf("code=%d\n", object->error_code_);
    std::printf("message=%s\n", object->error_message_.str().c_str());
    return 0;
  }
  std::fprintf(stderr, "unknown mode %s\n", mode.c_str());
  return 2;
}