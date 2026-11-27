// Scripted oracle for SessionConnection: fake transport in, events out.
//
//   session_probe <script.txt>
//
// Script lines:
//   newconn <session_id:u64> <salt:u64>   start a fresh connection + AuthData
//   in <msg_id:u64> <seq:int> <body_hex>  inject one plaintext packet
//   salt                                  print the current server salt
//
// The plaintext handed to RawConnection::Callback is the message prefix plus the
// body: [msg_id:long][seq_no:int][length:int][body]; SessionConnection re-parses
// it as mtproto_api::message (F123). Containers are
// [msg_container#73f1f8dc][count:int][message...] with no vector constructor,
// matching TDLib's own ContainerImpl.
//
// Output: one line per op (conn/in/salt) plus the callback events; two runs are
// byte-identical, so the Rust side can be compared line by line.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/session_probe.cpp -Wl,--start-group C:/tmp/td-build/libtdcore.a \
//       C:/tmp/td-build/libtdapi.a C:/tmp/td-build/libtdmtproto.a \
//       C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdnet/libtdnet.a \
//       C:/tmp/td-build/tde2e/libtde2e.a C:/tmp/td-build/tdutils/libtdutils.a \
//       C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/sqlite/libtdsqlite.a \
//       -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -liphlpapi -lcrypt32 \
//       -lpsapi -lmswsock -lshell32 -o session_probe.exe

#include "td/mtproto/AuthData.h"
#include "td/mtproto/RawConnection.h"
#include "td/mtproto/SessionConnection.h"

#include "td/utils/Slice.h"
#include "td/utils/Time.h"
#include "td/utils/buffer.h"
#include "td/utils/common.h"

#include <cstdio>
#include <cstring>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>
#include <utility>
#include <vector>

using td::int32;
using td::uint64;

namespace {

struct FakeRaw final : public td::mtproto::RawConnection {
  std::vector<std::pair<td::mtproto::PacketInfo, std::string>> inbound;
  std::vector<std::string> sent;
  td::PollableFdInfo poll_info;
  PublicFields extra_;

  void set_connection_token(td::mtproto::ConnectionManager::ConnectionToken) final {
  }
  bool can_send() const final {
    return true;
  }
  td::mtproto::TransportType get_transport_type() const final {
    return td::mtproto::TransportType();
  }
  size_t send_crypto(const td::Storer &storer, uint64, td::int64, const td::mtproto::AuthKey &, uint64) final {
    std::string buffer(storer.size(), '\0');
    (void)storer.store(reinterpret_cast<td::uint8 *>(&buffer[0]));
    sent.push_back(buffer);
    return buffer.size();
  }
  void send_no_crypto(const td::Storer &storer) final {
    std::string buffer(storer.size(), '\0');
    (void)storer.store(reinterpret_cast<td::uint8 *>(&buffer[0]));
    sent.push_back(buffer);
  }
  td::PollableFdInfo &get_poll_info() final {
    return poll_info;
  }
  StatsCallback *stats_callback() final {
    return nullptr;
  }
  td::Status flush(const td::mtproto::AuthKey &, Callback &callback) final {
    for (auto &packet : inbound) {
      auto status = callback.on_raw_packet(packet.first, td::BufferSlice(packet.second));
      if (status.is_error()) {
        inbound.clear();
        return status;
      }
    }
    inbound.clear();
    return td::Status::OK();
  }
  bool has_error() const final {
    return false;
  }
  void close() final {
  }
  PublicFields &extra() final {
    return extra_;
  }
  const PublicFields &extra() const final {
    return extra_;
  }
};

struct Events final : public td::mtproto::SessionConnection::Callback {
  void on_connected() final {
    std::printf("event=connected\n");
  }
  void on_closed(td::Status status) final {
    std::printf("event=closed status=%s\n", status.message().str().c_str());
  }
  void on_server_salt_updated() final {
    std::printf("event=server_salt_updated\n");
  }
  void on_server_time_difference_updated(bool force) final {
    std::printf("event=time_difference force=%d\n", force ? 1 : 0);
  }
  void on_new_session_created(uint64 unique_id, td::mtproto::MessageId first_message_id) final {
    std::printf("event=new_session unique=%llu first=%llu\n", static_cast<unsigned long long>(unique_id),
                static_cast<unsigned long long>(first_message_id.get()));
  }
  void on_session_failed(td::Status status) final {
    std::printf("event=session_failed status=%s\n", status.message().str().c_str());
  }
  void on_container_sent(td::mtproto::MessageId, td::vector<td::mtproto::MessageId>) final {
  }
  td::Status on_pong(double, double, double) final {
    return td::Status::OK();
  }
  td::Status on_update(td::BufferSlice packet) final {
    std::printf("event=update size=%zu\n", packet.size());
    return td::Status::OK();
  }
  void on_message_ack(td::mtproto::MessageId message_id) final {
    std::printf("event=ack msg=%llu\n", static_cast<unsigned long long>(message_id.get()));
  }
  td::Status on_message_result_ok(td::mtproto::MessageId message_id, td::BufferSlice packet, size_t original_size) final {
    std::printf("event=result_ok req=%llu original=%zu size=%zu\n",
                static_cast<unsigned long long>(message_id.get()), original_size, packet.size());
    return td::Status::OK();
  }
  void on_message_result_error(td::mtproto::MessageId message_id, int code, td::string message) final {
    std::printf("event=result_error req=%llu code=%d message=%s\n",
                static_cast<unsigned long long>(message_id.get()), code, message.c_str());
  }
  void on_message_failed(td::mtproto::MessageId message_id, td::Status status) final {
    std::printf("event=message_failed msg=%llu status=%s\n", static_cast<unsigned long long>(message_id.get()),
                status.message().str().c_str());
  }
  void on_message_info(td::mtproto::MessageId message_id, int32 state, td::mtproto::MessageId answer_message_id,
                       int32 answer_size, int32 source) final {
    std::printf("event=message_info msg=%llu state=%d answer=%llu size=%d source=%d\n",
                static_cast<unsigned long long>(message_id.get()), state,
                static_cast<unsigned long long>(answer_message_id.get()), answer_size, source);
  }
  td::Status on_destroy_auth_key() final {
    std::printf("event=destroy_auth_key\n");
    return td::Status::OK();
  }
};

std::string unhex(const std::string &text) {
  std::string out;
  for (size_t i = 0; i + 1 < text.size(); i += 2) {
    unsigned int byte = 0;
    std::sscanf(text.c_str() + i, "%2x", &byte);
    out.push_back(static_cast<char>(byte));
  }
  return out;
}

void put_i32(std::string &out, td::uint32 value) {
  for (int i = 0; i < 4; i++) {
    out.push_back(static_cast<char>((value >> (8 * i)) & 0xff));
  }
}
void put_i64(std::string &out, uint64 value) {
  for (int i = 0; i < 8; i++) {
    out.push_back(static_cast<char>((value >> (8 * i)) & 0xff));
  }
}

struct Harness {
  td::unique_ptr<td::mtproto::AuthData> auth;
  FakeRaw *raw = nullptr;
  td::unique_ptr<td::mtproto::SessionConnection> conn;
  Events events;

  void open(uint64 session_id, uint64 salt) {
    conn.reset();
    raw = nullptr;
    auth = td::make_unique<td::mtproto::AuthData>();
    auth->set_use_pfs(false);
    auth->set_main_auth_key(td::mtproto::AuthKey(1, td::string(256, 'a')));
    auth->set_session_id(session_id);
    auth->set_server_salt(salt, td::Time::now());
    auto fake = td::make_unique<FakeRaw>();
    raw = fake.get();
    conn = td::make_unique<td::mtproto::SessionConnection>(td::mtproto::SessionConnection::Mode::Tcp,
                                                           std::move(fake), auth.get());
    conn->flush(&events);
  }

  void inject(uint64 message_id, int32 seq_no, const std::string &body) {
    std::string packet;
    put_i64(packet, message_id);
    put_i32(packet, static_cast<td::uint32>(seq_no));
    put_i32(packet, static_cast<td::uint32>(body.size()));
    packet += body;
    td::mtproto::PacketInfo info;
    info.session_id = auth->get_session_id();
    info.message_id = td::mtproto::MessageId(message_id);
    info.seq_no = seq_no;
    info.version = 2;
    raw->inbound.emplace_back(info, packet);
    conn->flush(&events);
  }
};

}  // namespace

int main(int argc, char **argv) {
  if (argc != 2) {
    std::fprintf(stderr, "usage: session_probe <script.txt>\n");
    return 2;
  }
  std::ifstream input(argv[1], std::ios::binary);
  std::string text((std::istreambuf_iterator<char>(input)), std::istreambuf_iterator<char>());
  std::istringstream lines(text);
  std::string line;
  Harness harness;
  while (std::getline(lines, line)) {
    std::istringstream tokens(line);
    std::string op;
    if (!(tokens >> op) || op[0] == '#') {
      continue;
    }
    if (op == "newconn") {
      unsigned long long session_id = 0;
      unsigned long long salt = 0;
      tokens >> session_id >> salt;
      harness.open(session_id, salt);
      std::printf("conn session=%llu salt=%llu\n", session_id, salt);
    } else if (op == "in") {
      unsigned long long message_id = 0;
      int seq_no = 0;
      std::string body;
      tokens >> message_id >> seq_no >> body;
      std::printf("in msg=%llu\n", message_id);
      harness.inject(message_id, seq_no, unhex(body));
    } else if (op == "salt") {
      std::printf("salt=%llu\n", static_cast<unsigned long long>(harness.auth->get_server_salt(td::Time::now())));
    } else {
      std::fprintf(stderr, "unknown op %s\n", op.c_str());
      return 2;
    }
  }
  return 0;
}