// Oracle for the simple string sanitizers in td/telegram/misc.cpp.
//
//   misc_probe username <hex>    clean_username
//   misc_probe phone <hex>       clean_phone_number
//   misc_probe offending <hex>   replace_offending_characters
//
// Input and output are hex-encoded bytes, so argv encoding cannot interfere.
// clean_name and clean_input_string are deliberately out of scope: they need
// utf8_truncate, the space-character table and the emoji handling.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/misc_probe.cpp -Wl,--start-group C:/tmp/td-build/libtdcore.a \
//       C:/tmp/td-build/libtdapi.a C:/tmp/td-build/libtdmtproto.a \
//       C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdnet/libtdnet.a \
//       C:/tmp/td-build/tde2e/libtde2e.a C:/tmp/td-build/tdutils/libtdutils.a \
//       C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/sqlite/libtdsqlite.a \
//       -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -liphlpapi -lcrypt32 \
//       -lpsapi -lmswsock -lshell32 -o misc_probe.exe

#include "td/telegram/misc.h"

#include "td/utils/Slice.h"
#include "td/utils/common.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

namespace {

td::string unhex(const char *text) {
  td::string out;
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
    std::fprintf(stderr, "usage: misc_probe <username|phone|offending> <hex>\n");
    return 2;
  }
  std::string mode = argv[1];
  td::string input = unhex(argv[2]);
  if (mode == "username") {
    print_hex("out", td::clean_username(std::move(input)));
    return 0;
  }
  if (mode == "phone") {
    auto value = input;
    td::clean_phone_number(value);
    print_hex("out", value);
    return 0;
  }
  if (mode == "offending") {
    auto value = input;
    td::replace_offending_characters(value);
    print_hex("out", value);
    return 0;
  }
  std::fprintf(stderr, "unknown mode %s\n", mode.c_str());
  return 2;
}