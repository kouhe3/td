// Oracle for AuthData's salt / server-time / packet state machine, driven by an
// injected clock (every `now` comes from the script, never from Time::now).
//
// auth_data_probe <script.txt>
//
// Script lines (whitespace separated, empty lines and '#' lines ignored):
//   session <u64>
//   set <salt:u64> <now:f64>
//   get <now:f64>                       -> get salt=<u64>
//   valid <now:f64>                     -> valid=<0|1>
//   has <now:f64>                       -> has=<0|1>
//   need <now:f64>                      -> need=<0|1>
//   future <now:f64> <salt> <since> <until> [<salt> <since> <until> ...]
//   diff <d:f64>                        -> diff updated=<0|1> cur=<bits>
//   reset <d:f64>                       -> reset cur=<bits>
//   servertime <now:f64>                -> servertime t=<bits>
//   out <msg_id:u64> <now:f64>          -> out=<0|1>
//   in <msg_id:u64> <now:f64>           -> in=<0|1>
//   packet <session:u64> <msg_id:u64> <now:f64>
//                                       -> packet ok updated=<0|1>
//                                       |  packet err=<code> updated=<0|1> text=<message>
//
// Every double is printed as its 64-bit pattern, so the two sides can be
// compared without depending on printf/format float formatting.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I C:/tmp/td-build/tdutils tools/fixtures/auth_data_probe.cpp \
//       -Wl,--start-group C:/tmp/td-build/libtdmtproto.a \
//       C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a \
//       -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -liphlpapi -o auth_data_probe.exe

#include "td/mtproto/AuthData.h"

#include "td/utils/common.h"

#include <cstdio>
#include <cstring>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>
#include <vector>

namespace {

void print_bits(const char *name, double value) {
  td::uint64 bits = 0;
  std::memcpy(&bits, &value, sizeof(bits));
  std::printf("%s=%016llx\n", name, static_cast<unsigned long long>(bits));
}

std::string read_file(const char *path) {
  std::ifstream input(path, std::ios::binary);
  return std::string(std::istreambuf_iterator<char>(input), std::istreambuf_iterator<char>());
}

td::Status run_line(td::mtproto::AuthData &data, std::istringstream &line) {
  std::string op;
  if (!(line >> op)) {
    return td::Status::OK();
  }
  if (op == "session") {
    unsigned long long session_id = 0;
    line >> session_id;
    data.set_session_id(session_id);
    std::printf("session ok\n");
    return td::Status::OK();
  }
  if (op == "set") {
    unsigned long long salt = 0;
    double now = 0;
    line >> salt >> now;
    data.set_server_salt(salt, now);
    std::printf("set ok\n");
    return td::Status::OK();
  }
  if (op == "get") {
    double now = 0;
    line >> now;
    std::printf("get salt=%llu\n", static_cast<unsigned long long>(data.get_server_salt(now)));
    return td::Status::OK();
  }
  if (op == "valid" || op == "has" || op == "need") {
    double now = 0;
    line >> now;
    int result = 0;
    if (op == "valid") {
      result = data.is_server_salt_valid(now) ? 1 : 0;
    } else if (op == "has") {
      result = data.has_salt(now) ? 1 : 0;
    } else {
      result = data.need_future_salts(now) ? 1 : 0;
    }
    std::printf("%s %d\n", op.c_str(), result);
    return td::Status::OK();
  }
  if (op == "future") {
    double now = 0;
    line >> now;
    std::vector<td::mtproto::ServerSalt> salts;
    unsigned long long salt = 0;
    double valid_since = 0;
    double valid_until = 0;
    while (line >> salt >> valid_since >> valid_until) {
      salts.push_back(td::mtproto::ServerSalt{static_cast<td::int64>(salt), valid_since, valid_until});
    }
    data.set_future_salts(salts, now);
    auto result = data.get_future_salts();
    std::printf("future n=%zu", result.size());
    for (const auto &entry : result) {
      td::uint64 since_bits = 0;
      td::uint64 until_bits = 0;
      std::memcpy(&since_bits, &entry.valid_since, sizeof(since_bits));
      std::memcpy(&until_bits, &entry.valid_until, sizeof(until_bits));
      std::printf(" %llu:%016llx:%016llx", static_cast<unsigned long long>(entry.salt),
                  static_cast<unsigned long long>(since_bits), static_cast<unsigned long long>(until_bits));
    }
    std::printf("\n");
    return td::Status::OK();
  }
  if (op == "diff") {
    double diff = 0;
    line >> diff;
    bool updated = data.update_server_time_difference(diff);
    std::printf("diff updated=%d ", updated ? 1 : 0);
    print_bits("cur", data.get_server_time_difference());
    return td::Status::OK();
  }
  if (op == "reset") {
    double diff = 0;
    line >> diff;
    data.reset_server_time_difference(diff);
    std::printf("reset ");
    print_bits("cur", data.get_server_time_difference());
    return td::Status::OK();
  }
  if (op == "servertime") {
    double now = 0;
    line >> now;
    std::printf("servertime ");
    print_bits("t", data.get_server_time(now));
    return td::Status::OK();
  }
  if (op == "out" || op == "in") {
    unsigned long long msg_id = 0;
    double now = 0;
    line >> msg_id >> now;
    bool valid = op == "out" ? data.is_valid_outbound_msg_id(td::mtproto::MessageId(msg_id), now)
                             : data.is_valid_inbound_msg_id(td::mtproto::MessageId(msg_id), now);
    std::printf("%s %d\n", op.c_str(), valid ? 1 : 0);
    return td::Status::OK();
  }
  if (op == "packet") {
    unsigned long long session_id = 0;
    unsigned long long msg_id = 0;
    double now = 0;
    line >> session_id >> msg_id >> now;
    bool updated = false;
    auto status = data.check_packet(session_id, td::mtproto::MessageId(msg_id), now, updated);
    if (status.is_ok()) {
      std::printf("packet ok updated=%d\n", updated ? 1 : 0);
    } else {
      std::printf("packet err=%d updated=%d text=%s\n", status.code(), updated ? 1 : 0,
                  status.message().str().c_str());
    }
    return td::Status::OK();
  }
  std::printf("unknown %s\n", op.c_str());
  return td::Status::Error(400, "unknown script op");
}

}  // namespace

int main(int argc, char **argv) {
  if (argc != 2) {
    std::fprintf(stderr, "usage: auth_data_probe <script.txt>\n");
    return 2;
  }
  auto text = read_file(argv[1]);
  td::mtproto::AuthData data;
  std::istringstream lines(text);
  std::string line;
  while (std::getline(lines, line)) {
    if (line.find_first_not_of(" \t\r") == std::string::npos || line[0] == '#') {
      continue;
    }
    std::istringstream tokens(line);
    auto status = run_line(data, tokens);
    if (status.is_error()) {
      std::fprintf(stderr, "script error: %s\n", status.message().str().c_str());
      return 1;
    }
  }
  return 0;
}