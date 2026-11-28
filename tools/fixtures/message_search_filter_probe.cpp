// Oracle for `td/telegram/MessageSearchFilter.{h,cpp}`, driven by a script:
//
//   message_search_filter_probe <script.txt>
//   message_search_filter_probe --check <case>   one fixed case per process:
//     print_size              operator<<(Size)                 UNREACHABLE
//     index_empty             message_search_filter_index(Empty)  CHECK
//     callindex_none          call_message_search_filter_index(Photo)  CHECK
//     input_unread_mention / input_failed_to_send /
//     input_unread_reaction / input_unread_poll_vote           UNREACHABLE
//
// Filter specs are the enum values as integers (1 = Animation .. 20 = UnreadPollVote).
//
// Build:
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I tdactor -I tdnet -I C:/tmp/td-build/tdutils tools/fixtures/message_search_filter_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tdnet/libtdnet.a
//   C:/tmp/td-build/tdtl/libtdtl.a C:/tmp/td-build/tde2e/libtde2e.a
//   C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdutils/libtdutils.a
//   C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/sqlite/libtdsqlite.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi -lcrypt32 -lsecur32
//   -lmswsock -liphlpapi -ldnsapi -o C:/tmp/td-probe/message_search_filter_probe.exe
//
// Measured behaviour this probe encodes (docs/facts.md F259): `Size` is 21, so
// there are 20 filters and the mask tops out at `1 << 19`; `Empty` is the only
// mask-0 filter; four filters have no TL input form; and td_api has no
// Call/MissedCall class, so those two are reachable only from the enum side.

#include "td/telegram/MessageSearchFilter.h"
#include "td/telegram/td_api.h"
#include "td/telegram/telegram_api.h"

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

string read_all(const char *path) {
  std::ifstream in(path, std::ios::binary);
  if (!in) {
    std::fprintf(stderr, "cannot open %s\n", path);
    std::exit(2);
  }
  return string(std::string((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>()));
}

std::vector<Slice> split_tokens(Slice line) {
  std::vector<Slice> tokens;
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

Slice trim_line(Slice line) {
  while (!line.empty() && (line.back() == '\r' || line.back() == ' ' || line.back() == '\t')) {
    line.remove_suffix(1);
  }
  while (!line.empty() && (line[0] == ' ' || line[0] == '\t')) {
    line.remove_prefix(1);
  }
  return line;
}

td::MessageSearchFilter parse_filter(Slice token) {
  return static_cast<td::MessageSearchFilter>(td::to_integer<td::int32>(token));
}

string render(td::MessageSearchFilter filter) {
  td::StringBuilder sb;
  sb << filter;
  return sb.as_cslice().str();
}

td::td_api::object_ptr<td::td_api::SearchMessagesFilter> make_td_api_filter(Slice name) {
  using namespace td::td_api;
  if (name == "empty") return make_object<searchMessagesFilterEmpty>();
  if (name == "animation") return make_object<searchMessagesFilterAnimation>();
  if (name == "audio") return make_object<searchMessagesFilterAudio>();
  if (name == "document") return make_object<searchMessagesFilterDocument>();
  if (name == "photo") return make_object<searchMessagesFilterPhoto>();
  if (name == "poll") return make_object<searchMessagesFilterPoll>();
  if (name == "video") return make_object<searchMessagesFilterVideo>();
  if (name == "voicenote") return make_object<searchMessagesFilterVoiceNote>();
  if (name == "photoandvideo") return make_object<searchMessagesFilterPhotoAndVideo>();
  if (name == "url") return make_object<searchMessagesFilterUrl>();
  if (name == "chatphoto") return make_object<searchMessagesFilterChatPhoto>();
  if (name == "videonote") return make_object<searchMessagesFilterVideoNote>();
  if (name == "voiceandvideonote") return make_object<searchMessagesFilterVoiceAndVideoNote>();
  if (name == "mention") return make_object<searchMessagesFilterMention>();
  if (name == "unreadmention") return make_object<searchMessagesFilterUnreadMention>();
  if (name == "unreadreaction") return make_object<searchMessagesFilterUnreadReaction>();
  if (name == "unreadpollvote") return make_object<searchMessagesFilterUnreadPollVote>();
  if (name == "failedtosend") return make_object<searchMessagesFilterFailedToSend>();
  if (name == "pinned") return make_object<searchMessagesFilterPinned>();
  std::fprintf(stderr, "bad td_api filter name\n");
  std::exit(2);
}

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc == 3 && std::strcmp(argv[1], "--check") == 0) {
    Slice check_case(argv[2]);
    std::printf("before\n");
    std::fflush(stdout);
    if (check_case == "print_size") {
      std::printf("after=%s\n", render(td::MessageSearchFilter::Size).c_str());
    } else if (check_case == "index_empty") {
      std::printf("after=%d\n", td::message_search_filter_index(td::MessageSearchFilter::Empty));
    } else if (check_case == "callindex_none") {
      std::printf("after=%d\n", td::call_message_search_filter_index(td::MessageSearchFilter::Photo));
    } else if (check_case == "input_unread_mention") {
      std::printf("after id=%d\n", td::get_input_messages_filter(td::MessageSearchFilter::UnreadMention)->get_id());
    } else if (check_case == "input_failed_to_send") {
      std::printf("after id=%d\n", td::get_input_messages_filter(td::MessageSearchFilter::FailedToSend)->get_id());
    } else if (check_case == "input_unread_reaction") {
      std::printf("after id=%d\n", td::get_input_messages_filter(td::MessageSearchFilter::UnreadReaction)->get_id());
    } else if (check_case == "input_unread_poll_vote") {
      std::printf("after id=%d\n", td::get_input_messages_filter(td::MessageSearchFilter::UnreadPollVote)->get_id());
    } else {
      std::fprintf(stderr, "unknown check case\n");
      return 2;
    }
    std::fflush(stdout);
    return 0;
  }
  if (argc != 2) {
    std::fprintf(stderr, "usage: message_search_filter_probe <script.txt>\n");
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
    if (command == "count") {
      std::printf("count=%zu\n", td::message_search_filter_count());
    } else if (command == "print") {
      auto filter = parse_filter(tokens[1]);
      std::printf("print=%s\n", render(filter).c_str());
    } else if (command == "index") {
      auto filter = parse_filter(tokens[1]);
      std::printf("index=%d\n", td::message_search_filter_index(filter));
    } else if (command == "mask") {
      auto filter = parse_filter(tokens[1]);
      std::printf("mask=%d\n", td::message_search_filter_index_mask(filter));
    } else if (command == "callindex") {
      auto filter = parse_filter(tokens[1]);
      std::printf("callindex=%d\n", td::call_message_search_filter_index(filter));
    } else if (command == "input") {
      auto filter = parse_filter(tokens[1]);
      auto object = td::get_input_messages_filter(filter);
      if (object == nullptr) {
        std::printf("input=null\n");
      } else if (object->get_id() == td::telegram_api::inputMessagesFilterPhoneCalls::ID) {
        auto *calls = static_cast<td::telegram_api::inputMessagesFilterPhoneCalls *>(object.get());
        std::printf("input=calls silent=%d\n", static_cast<int>(calls->missed_));
      } else {
        std::printf("input=id=%d\n", object->get_id());
      }
    } else if (command == "tdapi") {
      auto object = make_td_api_filter(tokens[1]);
      auto filter = td::get_message_search_filter(object);
      std::printf("tdapi=%s\n", render(filter).c_str());
    } else if (command == "tdapinull") {
      td::tl_object_ptr<td::td_api::SearchMessagesFilter> object;
      auto filter = td::get_message_search_filter(object);
      std::printf("tdapinull=%s\n", render(filter).c_str());
    } else {
      std::fprintf(stderr, "unknown command %s\n", command.str().c_str());
      return 2;
    }
  }
  return 0;
}