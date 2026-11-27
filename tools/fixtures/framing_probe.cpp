// Oracle for the MTProto intermediate transport framing.
//
// framing_probe <payload_hex> <with_padding 0|1> <quick_ack 0|1>
//   -> magic=<hex> packet=<hex>
//
// `write_prepare_inplace` prepends the 4-byte little-endian length (with the
// quick-ack bit in the high bit) and appends random padding when asked.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I C:/tmp/td-build/tdutils tools/fixtures/framing_probe.cpp \
//       -Wl,--start-group C:/tmp/td-build/libtdmtproto.a \
//       C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a \
//       -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -liphlpapi -o framing_probe.exe

#include "td/mtproto/TcpTransport.h"

#include "td/utils/Slice.h"
#include "td/utils/buffer.h"
#include "td/utils/common.h"

#include <cstdio>
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
  if (argc < 4) {
    std::fprintf(stderr, "usage: framing_probe <payload_hex> <with_padding 0|1> <quick_ack 0|1>\n");
    return 2;
  }
  td::string payload = unhex(argv[1]);
  bool with_padding = std::atoi(argv[2]) != 0;
  bool quick_ack = std::atoi(argv[3]) != 0;

  td::mtproto::tcp::IntermediateTransport transport(with_padding);

  td::ChainBufferWriter out;
  transport.init_output_stream(&out);
  auto reader = out.extract_reader();
  print_hex("magic", reader.prepare_read());

  td::BufferWriter message{td::Slice(payload), 4, with_padding ? 15 : 0};
  transport.write_prepare_inplace(&message, quick_ack);
  print_hex("packet", message.as_slice());
  return 0;
}