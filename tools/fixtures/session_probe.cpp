// Oracle scaffold for SessionConnection: a fake RawConnection injects plaintext
// packets and a fake callback records what TDLib does with them.
//
// This answers the C-tier question -- can rpc_result / msg_container parsing,
// the bad_msg_notification policy and the bad_server_salt switch be tested
// without a network? Yes: the class takes its transport by unique_ptr and its
// callback per flush, so both can be faked. It is not a gate item yet; turning
// it into an instrument needs a Rust counterpart and a criterion.
//
// Two layout facts only show up when running it:
//   * the plaintext passed to RawConnection::Callback is the message prefix plus
//     the body: [msg_id:long][seq_no:int][length:int][body] (SessionConnection
//     re-parses it as mtproto_api::message);
//   * a container is [msg_container#73f1f8dc][count:int][message...] with no
//     vector constructor, matching TDLib's own ContainerImpl.
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
//
// Usage: session_probe.exe  (prints six scenarios to stdout, exit code 0)

#include "td/mtproto/AuthData.h"
#include "td/mtproto/RawConnection.h"
#include "td/mtproto/SessionConnection.h"

#include "td/utils/Slice.h"
#include "td/utils/Time.h"
#include "td/utils/buffer.h"
#include "td/utils/common.h"

#include <cstdio>
#include <cstring>
#include <string>
#include <utility>
#include <vector>

using td::int32;
using td::int64;
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
  size_t send_crypto(const td::Storer &storer, uint64, int64, const td::mtproto::AuthKey &, uint64) final {
    std::string buffer(storer.size(), '\0');
    storer.store(reinterpret_cast<td::uint8 *>(&buffer[0]));
    sent.push_back(buffer);
    return buffer.size();
  }
  void send_no_crypto(const td::Storer &storer) final {
    std::string buffer(storer.size(), '\0');
    storer.store(reinterpret_cast<td::uint8 *>(&buffer[0]));
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

std::string message_envelope(uint64 msg_id, int32 seq_no, const std::string &body) {
  std::string out;
  put_i64(out, msg_id);
  put_i32(out, static_cast<td::uint32>(seq_no));
  put_i32(out, static_cast<td::uint32>(body.size()));
  out += body;
  return out;
}

std::string container(const std::vector<std::string> &messages) {
  std::string out;
  put_i32(out, 0x73f1f8dc);
  put_i32(out, static_cast<td::uint32>(messages.size()));
  for (auto &message : messages) {
    out += message;
  }
  return out;
}

uint64 now_message_id(int low) {
  return (static_cast<uint64>(td::Time::now()) << 32) | static_cast<uint64>(low);
}

// The plaintext handed to RawConnection::Callback is the message prefix plus the
// body: [msg_id][seq_no][length][body]. SessionConnection re-parses it as
// `mtproto_api::message`, so a bare object must be wrapped here.
void send_single(FakeRaw &raw, uint64 outer_msg_id, const std::string &body) {
  td::mtproto::PacketInfo info;
  info.session_id = 777;
  info.message_id = td::mtproto::MessageId(outer_msg_id);
  info.seq_no = 1;
  info.version = 2;
  raw.inbound.emplace_back(info, message_envelope(outer_msg_id, 1, body));
}

struct Harness {
  td::mtproto::AuthData auth;
  FakeRaw *raw = nullptr;
  td::unique_ptr<td::mtproto::SessionConnection> conn;
  Events events;

  explicit Harness(int64 salt) {
    auth.set_use_pfs(false);
    auth.set_main_auth_key(td::mtproto::AuthKey(1, td::string(256, 'a')));
    auth.set_session_id(777);
    auth.set_server_salt(static_cast<uint64>(salt), td::Time::now());
  }
  void connect() {
    auto fake = td::make_unique<FakeRaw>();
    raw = fake.get();
    conn = td::make_unique<td::mtproto::SessionConnection>(td::mtproto::SessionConnection::Mode::Tcp, std::move(fake),
                                                           &auth);
    conn->flush(&events);
  }
};

void scenario_rpc_result() {
  std::printf("scenario=rpc_result\n");
  Harness h(1234);
  h.connect();
  uint64 outer = now_message_id(1);
  std::string child;
  put_i32(child, 0xf35c6d01);  // rpc_result
  put_i64(child, 4001);        // req_msg_id
  put_i32(child, 0x12345678);  // not gzip_packed: the default branch passes it through
  send_single(*h.raw, outer, child);
  h.conn->flush(&h.events);
  std::printf("end\n");
}

void scenario_bad_server_salt() {
  std::printf("scenario=bad_server_salt\n");
  Harness h(1234);
  h.connect();
  std::string child;
  put_i32(child, 0xedab447b);
  put_i64(child, 4001);
  put_i32(child, 1);
  put_i32(child, 48);
  put_i64(child, 0x99aabbccddeeff00ULL);
  send_single(*h.raw, now_message_id(1), child);
  h.conn->flush(&h.events);
  std::printf("salt_after=%llu\n", static_cast<unsigned long long>(h.auth.get_server_salt(td::Time::now())));
  std::printf("end\n");
}

void scenario_bad_msg(int code) {
  std::printf("scenario=bad_msg code=%d\n", code);
  Harness h(1234);
  h.connect();
  std::string child;
  put_i32(child, 0xa7eff811);
  put_i64(child, 4001);
  put_i32(child, 1);
  put_i32(child, static_cast<td::uint32>(code));
  send_single(*h.raw, now_message_id(1), child);
  h.conn->flush(&h.events);
  std::printf("end\n");
}

void scenario_container() {
  std::printf("scenario=container\n");
  Harness h(1234);
  h.connect();
  std::string first;
  put_i32(first, 0xf35c6d01);
  put_i64(first, 4001);
  put_i32(first, 0x12345678);
  std::string second;
  put_i32(second, 0xa7eff811);
  put_i64(second, 4003);
  put_i32(second, 1);
  put_i32(second, 16);
  std::string body = container({message_envelope(4002, 1, first), message_envelope(4004, 1, second)});
  send_single(*h.raw, now_message_id(1), body);
  h.conn->flush(&h.events);
  std::printf("end\n");
}

}  // namespace

int main() {
  td::mtproto::RawConnection *dummy = nullptr;
  (void)dummy;
  scenario_rpc_result();
  scenario_bad_server_salt();
  scenario_bad_msg(16);
  scenario_bad_msg(17);
  scenario_bad_msg(99);
  scenario_container();
  return 0;
}