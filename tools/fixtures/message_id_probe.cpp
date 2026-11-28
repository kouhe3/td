// Oracle for td::MessageId (td/telegram/MessageId.{h,cpp}) driven by a script.
//
//   message_id_probe <script.txt>
//   message_id_probe --check <case>      one fixed case per process, only to see
//                                        whether the implementation aborts
//
// `MessageId` is the 64-bit tag in front of every message in the client state:
// a server id in the high 31 bits, a local id in the next 17 bits and a two-bit
// type, with a scheduled layout on top. It is a pure value type -- no `Td`, no
// actor -- so the probe builds the values itself and drives the real code.
//
// Script lines (blank lines and '#' comments ignored):
//   raw <int64>                MessageId(int64)
//   srv <int32>                MessageId(ServerMessageId)
//   sched <id> <date> [force]  MessageId(ScheduledServerMessageId, send_date)
//   dump                       one line with the rendering and every predicate
//   prev | nextsrv             get_prev/next_server_message_id (guarded)
//   next <0|1|2>               get_next_message_id(Server|YetUnsent|Local)
//   date | srvget | ssrv       scheduled date / server id accessors (guarded)
//   cmp <a> <b>                ordering between two raw ids (same layout only)
//   ids <csv>                  get_message_ids then render each
//   srvids <csv>               get_server_message_ids
//   ssrvids <csv>              get_scheduled_server_message_ids
//
// `n/a` marks an accessor whose CHECK would fire for this value; the guarded
// forms keep the narration comparable instead of aborting. The abort cases are
// separate (`--check`).
//
// Build (`MessageId.cpp` lives in libtdcore.a):
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I C:/tmp/td-build/tdutils tools/fixtures/message_id_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi
//   -o C:/tmp/td-probe/message_id_probe.exe
//
// Measured behaviour this probe encodes (docs/facts.md F208): the ordinary
// layout is `|31 server|17 local|1 zero|2 type|`, the scheduled layout is
// `|30 date-2^30|18 server|1 one|2 type|`, and `operator<<` renders the local
// part as `<server>.<low 20 bits>`.

#include "td/telegram/MessageId.h"

#include "td/utils/Slice.h"
#include "td/utils/StringBuilder.h"
#include "td/utils/misc.h"

#include <cstdio>
#include <cstring>
#include <fstream>
#include <string>
#include <vector>

namespace {

using td::Slice;
using td::string;
using td::vector;

string read_all(const char *path) {
  std::ifstream in(path, std::ios::binary);
  if (!in) {
    std::fprintf(stderr, "cannot open %s\n", path);
    std::exit(2);
  }
  return string(std::string((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>()));
}

Slice trim_line(Slice line) {
  while (!line.empty() && (line.back() == '\r' || line.back() == ' ' || line.back() == '\t')) {
    line.remove_suffix(1);
  }
  while (!line.empty() && (line[0] == ' ' || line[0] == '\t')) {
    line.remove_prefix(1);
  }
  return line;
}

vector<Slice> split_tokens(Slice line) {
  vector<Slice> tokens;
  while (!line.empty()) {
    while (!line.empty() && line[0] == ' ') {
      line.remove_prefix(1);
    }
    if (line.empty()) {
      break;
    }
    size_t i = 0;
    while (i < line.size() && line[i] != ' ') {
      i++;
    }
    tokens.push_back(line.substr(0, i));
    line.remove_prefix(i);
  }
  return tokens;
}

td::int64 parse_i64(Slice token) {
  td::int64 value = 0;
  bool negative = false;
  if (!token.empty() && token[0] == '-') {
    negative = true;
    token.remove_prefix(1);
  }
  for (auto c : token) {
    if (c < '0' || c > '9') {
      std::fprintf(stderr, "bad integer\n");
      std::exit(2);
    }
    value = value * 10 + (c - '0');
  }
  return negative ? -value : value;
}

vector<td::int64> parse_i64_csv(Slice token) {
  vector<td::int64> result;
  if (token == "-") {
    return result;
  }
  for (auto part : td::full_split(token, ',')) {
    result.push_back(parse_i64(part));
  }
  return result;
}

string render(td::MessageId message_id) {
  td::StringBuilder sb;
  sb << message_id;
  return sb.as_cslice().str();
}

const char *type_name(td::MessageType type) {
  switch (type) {
    case td::MessageType::None:
      return "None";
    case td::MessageType::Server:
      return "Server";
    case td::MessageType::YetUnsent:
      return "YetUnsent";
    case td::MessageType::Local:
      return "Local";
  }
  return "?";
}

void dump(td::MessageId message_id) {
  const char *yet = "n/a";
  const char *local = "n/a";
  const char *scheduled_server = "n/a";
  if (message_id.is_valid() || message_id.is_scheduled()) {
    yet = message_id.is_yet_unsent() ? "1" : "0";
    local = message_id.is_local() ? "1" : "0";
  }
  if (message_id.is_valid_scheduled()) {
    scheduled_server = message_id.is_scheduled_server() ? "1" : "0";
  }
  const char *any = "n/a";
  if (!message_id.is_scheduled() || message_id.is_valid_scheduled()) {
    any = message_id.is_any_server() ? "1" : "0";
  }
  std::printf(
      "dump raw=%lld print=%s valid=%d vsched=%d vspon=%d type=%s sched=%d server=%d ssrv=%s local=%s yet=%s any=%s\n",
      static_cast<long long>(message_id.get()), render(message_id).c_str(), static_cast<int>(message_id.is_valid()),
      static_cast<int>(message_id.is_valid_scheduled()), static_cast<int>(message_id.is_valid_sponsored()),
      type_name(message_id.get_type()), static_cast<int>(message_id.is_scheduled()),
      static_cast<int>(message_id.is_server()), scheduled_server, local, yet,
      any);
}

}  // namespace

int main(int argc, char **argv) {
  if (argc == 3 && std::strcmp(argv[1], "--check") == 0) {
    Slice check_case(argv[2]);
    if (check_case == "date_ordinary") {
      // get_scheduled_message_date() CHECKs is_valid_scheduled()
      auto value = td::MessageId(static_cast<td::int64>(1000));
      std::printf("before=%s\n", render(value).c_str());
      std::fflush(stdout);
      std::printf("date=%d\n", value.get_scheduled_message_date());
      return 0;
    }
    if (check_case == "ssrv_ordinary") {
      // get_scheduled_server_message_id() CHECKs is_scheduled_server()
      auto value = td::MessageId(static_cast<td::int64>(1000));
      std::printf("before=%s\n", render(value).c_str());
      std::fflush(stdout);
      std::printf("ssrv=%d\n", value.get_scheduled_server_message_id().get());
      return 0;
    }
    if (check_case == "cmp_mixed") {
      // operator< CHECKs that both sides share the layout
      auto ordinary = td::MessageId(static_cast<td::int64>(1000));
      auto scheduled = td::MessageId(td::ScheduledServerMessageId(5), (1 << 30) + 10);
      std::printf("before=%s %s\n", render(ordinary).c_str(), render(scheduled).c_str());
      std::fflush(stdout);
      std::printf("lt=%d\n", static_cast<int>(ordinary < scheduled));
      return 0;
    }
    if (check_case == "next_none") {
      // get_next_message_id(MessageType::None) is UNREACHABLE
      auto value = td::MessageId(static_cast<td::int64>(1000));
      std::printf("before=%s\n", render(value).c_str());
      std::fflush(stdout);
      auto next = value.get_next_message_id(td::MessageType::None);
      std::printf("next=%s\n", render(next).c_str());
      return 0;
    }
    std::fprintf(stderr, "unknown check case\n");
    return 2;
  }

  if (argc != 2) {
    std::fprintf(stderr, "usage: message_id_probe <script.txt>\n");
    return 2;
  }

  auto script = read_all(argv[1]);
  td::MessageId current;
  for (auto raw_line : td::full_split(Slice(script), '\n')) {
    auto line = trim_line(raw_line);
    if (line.empty() || line[0] == '#') {
      continue;
    }
    auto tokens = split_tokens(line);
    auto command = tokens[0];
    if (command == "raw") {
      current = td::MessageId(parse_i64(tokens[1]));
    } else if (command == "srv") {
      current = td::MessageId(td::ServerMessageId(static_cast<td::int32>(parse_i64(tokens[1]))));
    } else if (command == "sched") {
      bool force = tokens.size() > 3 && tokens[3] == "1";
      current = td::MessageId(td::ScheduledServerMessageId(static_cast<td::int32>(parse_i64(tokens[1]))),
                              static_cast<td::int32>(parse_i64(tokens[2])), force);
    } else if (command == "dump") {
      dump(current);
    } else if (command == "prev") {
      if (current.is_scheduled()) {
        std::printf("prev=n/a\n");
      } else {
        std::printf("prev=%lld\n", static_cast<long long>(current.get_prev_server_message_id().get()));
      }
    } else if (command == "nextsrv") {
      if (current.is_scheduled()) {
        std::printf("nextsrv=n/a\n");
      } else {
        std::printf("nextsrv=%lld\n", static_cast<long long>(current.get_next_server_message_id().get()));
      }
    } else if (command == "next") {
      auto type = static_cast<td::MessageType>(parse_i64(tokens[1]));
      if (type == td::MessageType::None || (current.is_scheduled() && !current.is_valid_scheduled())) {
        std::printf("next=n/a\n");
      } else {
        std::printf("next=%s\n", render(current.get_next_message_id(type)).c_str());
      }
    } else if (command == "date") {
      if (current.is_valid_scheduled()) {
        std::printf("date=%d\n", current.get_scheduled_message_date());
      } else {
        std::printf("date=n/a\n");
      }
    } else if (command == "srvget") {
      if (current.is_server()) {
        std::printf("srv=%d\n", current.get_server_message_id().get());
      } else {
        std::printf("srv=n/a\n");
      }
    } else if (command == "ssrv") {
      if (current.is_valid_scheduled() && current.is_scheduled_server()) {
        std::printf("ssrv=%d\n", current.get_scheduled_server_message_id().get());
      } else {
        std::printf("ssrv=n/a\n");
      }
    } else if (command == "cmp") {
      td::MessageId lhs(parse_i64(tokens[1]));
      td::MessageId rhs(parse_i64(tokens[2]));
      if (lhs.is_scheduled() != rhs.is_scheduled()) {
        std::printf("cmp=n/a\n");
      } else {
        std::printf("cmp lt=%d gt=%d le=%d ge=%d eq=%d\n", static_cast<int>(lhs < rhs), static_cast<int>(lhs > rhs),
                    static_cast<int>(lhs <= rhs), static_cast<int>(lhs >= rhs), static_cast<int>(lhs == rhs));
      }
    } else if (command == "ids") {
      auto ids = td::MessageId::get_message_ids(parse_i64_csv(tokens[1]));
      std::printf("ids n=%d", static_cast<int>(ids.size()));
      for (auto id : ids) {
        std::printf(" [%s]", render(id).c_str());
      }
      std::printf("\n");
    } else if (command == "srvids") {
      vector<td::MessageId> ids;
      for (auto value : parse_i64_csv(tokens[1])) {
        ids.push_back(td::MessageId(value));
      }
      auto server_ids = td::MessageId::get_server_message_ids(ids);
      std::printf("srvids n=%d", static_cast<int>(server_ids.size()));
      for (auto id : server_ids) {
        std::printf(" %d", id);
      }
      std::printf("\n");
    } else if (command == "ssrvids") {
      vector<td::MessageId> ids;
      for (auto value : parse_i64_csv(tokens[1])) {
        ids.push_back(td::MessageId(value));
      }
      auto server_ids = td::MessageId::get_scheduled_server_message_ids(ids);
      std::printf("ssrvids n=%d", static_cast<int>(server_ids.size()));
      for (auto id : server_ids) {
        std::printf(" %d", id);
      }
      std::printf("\n");
    } else {
      std::fprintf(stderr, "unknown command %s\n", command.str().c_str());
      return 2;
    }
  }
  return 0;
}