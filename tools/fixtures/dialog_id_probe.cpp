// Oracle for td::DialogId (td/telegram/DialogId.{h,cpp}) driven by a script.
//
//   dialog_id_probe <script.txt>
//   dialog_id_probe --check <case>      one fixed case per process, only to see
//                                       whether the implementation aborts
//
// `DialogId` packs a dialog of any type into one int64: positive ids are users,
// small negative ids are basic groups, `-10^12 - channel_id` is a channel and
// `-2*10^12 + secret_chat_id` is a secret chat, with a monoforum channel range
// on top. It is a pure value type -- no `Td`, no actor.
//
// Script lines (blank lines and '#' comments ignored):
//   raw <int64> | user <id> | chat <id> | channel <id> | secret <id>
//   dump                       raw / rendering / valid / type
//   uid | cid | chid | sid     the four CHECK-guarded getters (n/a when wrong)
//   ids <csv>                  get_dialog_ids(vector<int64>)
//   usrs|chats|chans|secs <csv> the four typed get_dialog_ids overloads
//   rmsecret <csv>             remove_secret_chat_dialog_ids
//   cmp <a> <b>                equality between two raw ids
//
// Build (`DialogId.cpp` lives in libtdcore.a):
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I C:/tmp/td-build/tdutils tools/fixtures/dialog_id_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi
//   -o C:/tmp/td-probe/dialog_id_probe.exe
//
// Measured behaviour this probe encodes (docs/facts.md F212): the ranges are
// `[-999999999999, 0)` basic groups, `[-1997852516352, -10^12)` channels,
// `[-2002147483648, -2*10^12)` secret chats and `[-4*10^12, ...)` the rest of
// the monoforum channels; users are `(0, 2^40-1]`.

#include "td/telegram/DialogId.h"

#include "td/telegram/ChannelId.h"
#include "td/telegram/ChatId.h"
#include "td/telegram/SecretChatId.h"
#include "td/telegram/UserId.h"

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

string render(td::DialogId dialog_id) {
  td::StringBuilder sb;
  sb << dialog_id;
  return sb.as_cslice().str();
}

const char *type_name(td::DialogType type) {
  switch (type) {
    case td::DialogType::None:
      return "None";
    case td::DialogType::User:
      return "User";
    case td::DialogType::Chat:
      return "Chat";
    case td::DialogType::Channel:
      return "Channel";
    case td::DialogType::SecretChat:
      return "SecretChat";
  }
  return "?";
}

void dump(td::DialogId dialog_id) {
  std::printf("dump raw=%lld print=%s valid=%d type=%s\n", static_cast<long long>(dialog_id.get()),
              render(dialog_id).c_str(), static_cast<int>(dialog_id.is_valid()), type_name(dialog_id.get_type()));
}

}  // namespace

int main(int argc, char **argv) {
  if (argc == 3 && std::strcmp(argv[1], "--check") == 0) {
    Slice check_case(argv[2]);
    auto print_and_die = [](td::DialogId dialog_id, auto getter) {
      std::printf("before=%s\n", render(dialog_id).c_str());
      std::fflush(stdout);
      std::printf("value=%lld\n", static_cast<long long>(getter(dialog_id)));
      return 0;
    };
    if (check_case == "uid_on_chat") {
      return print_and_die(td::DialogId(static_cast<td::int64>(-5)),
                           [](td::DialogId d) { return d.get_user_id().get(); });
    }
    if (check_case == "cid_on_channel") {
      return print_and_die(td::DialogId(static_cast<td::int64>(-1000000000005ll)),
                           [](td::DialogId d) { return d.get_chat_id().get(); });
    }
    if (check_case == "chid_on_user") {
      return print_and_die(td::DialogId(static_cast<td::int64>(42)),
                           [](td::DialogId d) { return d.get_channel_id().get(); });
    }
    if (check_case == "sid_on_chat") {
      return print_and_die(td::DialogId(static_cast<td::int64>(-5)),
                           [](td::DialogId d) { return static_cast<td::int64>(d.get_secret_chat_id().get()); });
    }
    std::fprintf(stderr, "unknown check case\n");
    return 2;
  }

  if (argc != 2) {
    std::fprintf(stderr, "usage: dialog_id_probe <script.txt>\n");
    return 2;
  }

  auto script = read_all(argv[1]);
  td::DialogId current;
  for (auto raw_line : td::full_split(Slice(script), '\n')) {
    auto line = trim_line(raw_line);
    if (line.empty() || line[0] == '#') {
      continue;
    }
    auto tokens = split_tokens(line);
    auto command = tokens[0];
    if (command == "raw") {
      current = td::DialogId(parse_i64(tokens[1]));
    } else if (command == "user") {
      current = td::DialogId(td::UserId(parse_i64(tokens[1])));
    } else if (command == "chat") {
      current = td::DialogId(td::ChatId(parse_i64(tokens[1])));
    } else if (command == "channel") {
      current = td::DialogId(td::ChannelId(parse_i64(tokens[1])));
    } else if (command == "secret") {
      current = td::DialogId(td::SecretChatId(static_cast<td::int32>(parse_i64(tokens[1]))));
    } else if (command == "dump") {
      dump(current);
    } else if (command == "uid") {
      if (current.get_type() == td::DialogType::User) {
        std::printf("uid=%lld\n", static_cast<long long>(current.get_user_id().get()));
      } else {
        std::printf("uid=n/a\n");
      }
    } else if (command == "cid") {
      if (current.get_type() == td::DialogType::Chat) {
        std::printf("cid=%lld\n", static_cast<long long>(current.get_chat_id().get()));
      } else {
        std::printf("cid=n/a\n");
      }
    } else if (command == "chid") {
      if (current.get_type() == td::DialogType::Channel) {
        std::printf("chid=%lld\n", static_cast<long long>(current.get_channel_id().get()));
      } else {
        std::printf("chid=n/a\n");
      }
    } else if (command == "sid") {
      if (current.get_type() == td::DialogType::SecretChat) {
        std::printf("sid=%d\n", current.get_secret_chat_id().get());
      } else {
        std::printf("sid=n/a\n");
      }
    } else if (command == "ids") {
      auto ids = td::DialogId::get_dialog_ids(parse_i64_csv(tokens[1]));
      std::printf("ids n=%d", static_cast<int>(ids.size()));
      for (auto id : ids) {
        std::printf(" [%s]", render(id).c_str());
      }
      std::printf("\n");
    } else if (command == "usrs" || command == "chats" || command == "chans" || command == "secs") {
      vector<td::DialogId> ids;
      for (auto value : parse_i64_csv(tokens[1])) {
        if (command == "usrs") {
          ids.push_back(td::DialogId(td::UserId(value)));
        } else if (command == "chats") {
          ids.push_back(td::DialogId(td::ChatId(value)));
        } else if (command == "chans") {
          ids.push_back(td::DialogId(td::ChannelId(value)));
        } else {
          ids.push_back(td::DialogId(td::SecretChatId(static_cast<td::int32>(value))));
        }
      }
      std::printf("%s n=%d", command.str().c_str(), static_cast<int>(ids.size()));
      for (auto id : ids) {
        std::printf(" [%s]", render(id).c_str());
      }
      std::printf("\n");
    } else if (command == "rmsecret") {
      vector<td::DialogId> ids;
      for (auto value : parse_i64_csv(tokens[1])) {
        ids.push_back(td::DialogId(value));
      }
      auto kept = td::DialogId::remove_secret_chat_dialog_ids(ids);
      std::printf("rmsecret n=%d", static_cast<int>(kept.size()));
      for (auto id : kept) {
        std::printf(" [%s]", render(id).c_str());
      }
      std::printf("\n");
    } else if (command == "cmp") {
      td::DialogId lhs(parse_i64(tokens[1]));
      td::DialogId rhs(parse_i64(tokens[2]));
      std::printf("cmp eq=%d ne=%d\n", static_cast<int>(lhs == rhs), static_cast<int>(lhs != rhs));
    } else {
      std::fprintf(stderr, "unknown command %s\n", command.str().c_str());
      return 2;
    }
  }
  return 0;
}