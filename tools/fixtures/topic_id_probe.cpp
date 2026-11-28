// Oracle for td::ForumTopicId + td::SavedMessagesTopicId (pure value part).
//
//   topic_id_probe <script.txt>
//
//   forum <int32>            ForumTopicId(id): valid / render
//   fids <csv>               get_forum_topic_ids
//   fmsgs <csv>              get_top_msg_ids
//   frommsg <int64>          MessageId -> ForumTopicId -> MessageId round trip
//                            (guarded: `n/a` when the MessageId is not server)
//   frommsg_force <int64>    the same without the guard (abort pair for a
//                            non-server MessageId: it hits
//                            MessageId::get_server_message_id()'s CHECK)
//   saved <int64 dialog>     SavedMessagesTopicId(DialogId): valid / unique /
//                            hidden / render
//   savedeq <a> <b>          equality between two raw dialog ids
//
// Both classes are pure: ForumTopicId is an int32 wrapper that round-trips
// through MessageId's server id, SavedMessagesTopicId is a DialogId wrapper
// with a hidden-author special case (docs/facts.md F221). No `Td`, no actor.
//
// Build (`SavedMessagesTopicId.cpp` lives in libtdcore.a):
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I tdactor -I tddb -I C:/tmp/td-build/tdutils tools/fixtures/topic_id_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi
//   -o C:/tmp/td-probe/topic_id_probe.exe

#include "td/telegram/DialogId.h"
#include "td/telegram/ForumTopicId.h"
#include "td/telegram/MessageId.h"
#include "td/telegram/SavedMessagesTopicId.h"

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

string render_message(td::MessageId message_id) {
  td::StringBuilder sb;
  sb << message_id;
  return sb.as_cslice().str();
}

string render_forum(td::ForumTopicId forum_topic_id) {
  td::StringBuilder sb;
  sb << forum_topic_id;
  return sb.as_cslice().str();
}

string render_saved(td::SavedMessagesTopicId saved_messages_topic_id) {
  td::StringBuilder sb;
  sb << saved_messages_topic_id;
  return sb.as_cslice().str();
}

void dump_forum(td::ForumTopicId forum_topic_id) {
  std::printf("forum raw=%d valid=%d print=%s\n", forum_topic_id.get(),
              static_cast<int>(forum_topic_id.is_valid()), render_forum(forum_topic_id).c_str());
}

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc != 2) {
    std::fprintf(stderr, "usage: topic_id_probe <script.txt>\n");
    return 2;
  }

  auto script = read_all(argv[1]);
  for (auto raw_line : td::full_split(Slice(script), '\n')) {
    auto line = trim_line(raw_line);
    if (line.empty() || line[0] == '#') {
      continue;
    }
    auto tokens = split_tokens(line);
    auto command = tokens[0];
    if (command == "forum") {
      dump_forum(td::ForumTopicId(static_cast<td::int32>(parse_i64(tokens[1]))));
    } else if (command == "fids") {
      vector<td::int32> input;
      for (auto value : parse_i64_csv(tokens[1])) {
        input.push_back(static_cast<td::int32>(value));
      }
      auto ids = td::ForumTopicId::get_forum_topic_ids(input);
      std::printf("fids n=%d", static_cast<int>(ids.size()));
      for (auto id : ids) {
        std::printf(" [%s]", render_forum(id).c_str());
      }
      std::printf("\n");
    } else if (command == "fmsgs") {
      vector<td::ForumTopicId> ids;
      for (auto value : parse_i64_csv(tokens[1])) {
        ids.push_back(td::ForumTopicId(static_cast<td::int32>(value)));
      }
      auto top_msg_ids = td::ForumTopicId::get_top_msg_ids(ids);
      std::printf("fmsgs n=%d", static_cast<int>(top_msg_ids.size()));
      for (auto id : top_msg_ids) {
        std::printf(" %d", id);
      }
      std::printf("\n");
    } else if (command == "frommsg" || command == "frommsg_force") {
      td::MessageId message_id(parse_i64(tokens[1]));
      if (command == "frommsg" && !message_id.is_server()) {
        std::printf("frommsg=n/a\n");
        continue;
      }
      auto forum_topic_id = td::ForumTopicId::from_top_thread_message_id(message_id);
      std::printf("frommsg raw=%lld top=%s print=%s\n", static_cast<long long>(message_id.get()),
                  render_message(forum_topic_id.to_top_thread_message_id()).c_str(),
                  render_forum(forum_topic_id).c_str());
    } else if (command == "saved") {
      td::DialogId dialog_id(parse_i64(tokens[1]));
      td::SavedMessagesTopicId saved(dialog_id);
      std::printf("saved dialog=%lld valid=%d unique=%lld hidden=%d print=%s\n",
                  static_cast<long long>(dialog_id.get()), static_cast<int>(saved.is_valid()),
                  static_cast<long long>(saved.get_unique_id()), static_cast<int>(saved.is_author_hidden()),
                  render_saved(saved).c_str());
    } else if (command == "savedeq") {
      td::SavedMessagesTopicId lhs(td::DialogId(parse_i64(tokens[1])));
      td::SavedMessagesTopicId rhs(td::DialogId(parse_i64(tokens[2])));
      std::printf("savedeq eq=%d ne=%d\n", static_cast<int>(lhs == rhs), static_cast<int>(lhs != rhs));
    } else {
      std::fprintf(stderr, "unknown command %s\n", command.str().c_str());
      return 2;
    }
  }
  return 0;
}