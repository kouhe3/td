// Oracle for MTProto 2.0 packet framing (Transport::write / Transport::read).
//
// write: transport_probe write <auth_key_hex> <akid> <salt> <session_id> <msg_id> <seq_no> <payload_hex>
//        -> writes the whole packet (hex) on stdout
// read:  transport_probe read <auth_key_hex> <akid> <packet_hex>
//        -> writes the decrypted payload (hex) and salt/session/msg_id/seq_no
//
// The padding is random, so byte-for-byte comparison only holds within one
// process; cross-implementation checks compare the payload and the prefix.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I C:/tmp/td-build/tdutils tools/fixtures/transport_probe.cpp \
//       -Wl,--start-group C:/tmp/td-build/libtdmtproto.a \
//       C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a \
//       -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -liphlpapi -o transport_probe.exe

#include "td/mtproto/AuthKey.h"
#include "td/mtproto/PacketInfo.h"
#include "td/mtproto/Transport.h"

#include "td/utils/Slice.h"
#include "td/utils/StorerBase.h"
#include "td/utils/buffer.h"
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
  std::printf("\n");
}

struct SliceStorer final : td::Storer {
  td::Slice data;
  explicit SliceStorer(td::Slice data_) : data(data_) {
  }
  size_t size() const final {
    return data.size();
  }
  size_t store(td::uint8 *ptr) const final {
    std::memcpy(ptr, data.data(), data.size());
    return data.size();
  }
};

}  // namespace

int main(int argc, char **argv) {
  if (argc < 4) {
    std::fprintf(stderr, "usage: transport_probe write|read ...\n");
    return 2;
  }
  std::string mode = argv[1];
  td::string auth_key = unhex(argv[2]);
  td::uint64 akid = std::strtoull(argv[3], nullptr, 10);
  td::mtproto::AuthKey key(akid, std::move(auth_key));

  if (mode == "write") {
    if (argc < 9) {
      std::fprintf(stderr, "write needs <salt> <session_id> <msg_id> <seq_no> <payload_hex>\n");
      return 2;
    }
    td::mtproto::PacketInfo info;
    info.salt = std::strtoull(argv[4], nullptr, 10);
    info.session_id = std::strtoull(argv[5], nullptr, 10);
    info.message_id = td::mtproto::MessageId(std::strtoull(argv[6], nullptr, 10));
    info.seq_no = static_cast<td::int32>(std::strtol(argv[7], nullptr, 10));
    info.version = 2;
    td::string payload = unhex(argv[8]);
    // The storer carries the CryptoPrefix itself: msg_id, seq_no, length, data.
    td::string body;
    td::uint64 mid = std::strtoull(argv[6], nullptr, 10);
    td::uint32 seq = static_cast<td::uint32>(std::strtol(argv[7], nullptr, 10));
    td::uint32 len = static_cast<td::uint32>(payload.size());
    body.append(reinterpret_cast<const char *>(&mid), sizeof(mid));
    body.append(reinterpret_cast<const char *>(&seq), sizeof(seq));
    body.append(reinterpret_cast<const char *>(&len), sizeof(len));
    body.append(payload);
    SliceStorer storer{td::Slice(body)};
    auto packet = td::mtproto::Transport::write(storer, key, &info);
    print_hex("packet", packet.as_slice());
    return 0;
  }

  if (mode == "read") {
    if (argc < 5) {
      std::fprintf(stderr, "read needs <packet_hex>\n");
      return 2;
    }
    td::string packet = unhex(argv[4]);
    td::MutableSlice message(packet);
    td::mtproto::PacketInfo info;
    info.version = 2;
    auto result = td::mtproto::Transport::read(message, 0, key, &info);
    if (result.is_error()) {
      std::fprintf(stderr, "read error: %s\n", result.error().message().c_str());
      return 1;
    }
    auto read = result.move_as_ok();
    if (read.type() != td::mtproto::Transport::ReadResult::Packet) {
      std::fprintf(stderr, "not a packet: %d\n", static_cast<int>(read.type()));
      return 1;
    }
    print_hex("payload", read.packet());
    std::printf("salt=%llu\n", static_cast<unsigned long long>(info.salt));
    std::printf("session_id=%llu\n", static_cast<unsigned long long>(info.session_id));
    std::printf("message_id=%llu\n", static_cast<unsigned long long>(info.message_id.get()));
    std::printf("seq_no=%d\n", info.seq_no);
    return 0;
  }

  std::fprintf(stderr, "unknown mode %s\n", mode.c_str());
  return 2;
}