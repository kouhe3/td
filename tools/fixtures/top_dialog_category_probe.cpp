// Oracle for `td/telegram/TopDialogCategory.{h,cpp}` and
// `td/telegram/NotificationGroupType.{h,cpp}`, driven by a script:
//
//   top_dialog_category_probe <script.txt>
//   top_dialog_category_probe --check <case>   one fixed case per process:
//     tl_null            get_top_dialog_category(wire nullptr)        CHECK
//     name_size          get_top_dialog_category_name(Size)           UNREACHABLE
//     input_size         get_input_top_peer_category(Size)            UNREACHABLE
//     ngt_null           get_notification_group_type(thd_api nullptr) CHECK
//     ngt_db_oor         is_database_notification_group_type(7)       UNREACHABLE
//     ngt_partial_oor    is_partial_notification_group_type(7)        UNREACHABLE
//     ngt_obj_oor        get_notification_group_type_object(7)        UNREACHABLE
//     ngt_print_oor      operator<<(7)                                UNREACHABLE
//
// Commands: `cat <td_api name>` / `catnull` / `cattl <wire name>` /
// `name <0..9>` / `input <0..9>` / `ngt <0..3>` / `ngtfrom <0..3>`.
//
// Build:
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I tdactor -I tdnet -I C:/tmp/td-build/tdutils tools/fixtures/top_dialog_category_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tdnet/libtdnet.a
//   C:/tmp/td-build/tdtl/libtdtl.a C:/tmp/td-build/tde2e/libtde2e.a
//   C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdutils/libtdutils.a
//   C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/sqlite/libtdsqlite.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi -lcrypt32 -lsecur32
//   -lmswsock -liphlpapi -ldnsapi -o C:/tmp/td-probe/top_dialog_category_probe.exe
//
// Measured behaviour this probe encodes (docs/facts.md F264): the td_api side
// collapses `topChatCategoryForwardChats` onto `ForwardUsers`, so `ForwardChats`
// is reachable only from the wire side; `Size` is 10 and both `name(Size)` and
// `input(Size)` are UNREACHABLE; a null td_api category is `Size` while a null
// wire category is a CHECK; the two NotificationGroupType predicates differ
// only on SecretChat (database yes, partial no).

#include "td/telegram/NotificationGroupType.h"
#include "td/telegram/TopDialogCategory.h"
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

using td::Slice;

namespace {

using Category = td::TopDialogCategory;
using GroupType = td::NotificationGroupType;

std::string read_all(const char *path) {
  std::ifstream in(path, std::ios::binary);
  if (!in) {
    std::fprintf(stderr, "cannot open %s\n", path);
    std::exit(2);
  }
  return std::string((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
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

Category parse_category(Slice token) {
  return static_cast<Category>(td::to_integer<td::int32>(token));
}

GroupType parse_group_type(Slice token) {
  return static_cast<GroupType>(td::to_integer<td::int32>(token));
}

td::string render_category(Category category) {
  return td::get_top_dialog_category_name(category).str();
}

td::string render_group_type(GroupType type) {
  td::StringBuilder sb;
  sb << type;
  return sb.as_cslice().str();
}

td::td_api::object_ptr<td::td_api::TopChatCategory> make_td_api_category(Slice name) {
  using namespace td::td_api;
  if (name == "users") return make_object<topChatCategoryUsers>();
  if (name == "bots") return make_object<topChatCategoryBots>();
  if (name == "botsinline") return make_object<topChatCategoryInlineBots>();
  if (name == "groups") return make_object<topChatCategoryGroups>();
  if (name == "channels") return make_object<topChatCategoryChannels>();
  if (name == "calls") return make_object<topChatCategoryCalls>();
  if (name == "forwardchats") return make_object<topChatCategoryForwardChats>();
  if (name == "webappbots") return make_object<topChatCategoryWebAppBots>();
  if (name == "guestbots") return make_object<topChatCategoryGuestBots>();
  std::fprintf(stderr, "bad td_api category name\n");
  std::exit(2);
}

td::telegram_api::object_ptr<td::telegram_api::TopPeerCategory> make_wire_category(Slice name) {
  using namespace td::telegram_api;
  if (name == "correspondents") return td::make_tl_object<topPeerCategoryCorrespondents>();
  if (name == "botspm") return td::make_tl_object<topPeerCategoryBotsPM>();
  if (name == "botsinline") return td::make_tl_object<topPeerCategoryBotsInline>();
  if (name == "groups") return td::make_tl_object<topPeerCategoryGroups>();
  if (name == "channels") return td::make_tl_object<topPeerCategoryChannels>();
  if (name == "phonecalls") return td::make_tl_object<topPeerCategoryPhoneCalls>();
  if (name == "forwardusers") return td::make_tl_object<topPeerCategoryForwardUsers>();
  if (name == "forwardchats") return td::make_tl_object<topPeerCategoryForwardChats>();
  if (name == "botsapp") return td::make_tl_object<topPeerCategoryBotsApp>();
  if (name == "botsguest") return td::make_tl_object<topPeerCategoryBotsGuestChat>();
  std::fprintf(stderr, "bad wire category name\n");
  std::exit(2);
}

td::td_api::object_ptr<td::td_api::NotificationGroupType> make_td_api_group_type(GroupType type) {
  return td::get_notification_group_type_object(type);
}

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc == 3 && std::strcmp(argv[1], "--check") == 0) {
    Slice check_case(argv[2]);
    std::printf("before\n");
    std::fflush(stdout);
    if (check_case == "tl_null") {
      std::printf("after=%s\n",
                  render_category(td::get_top_dialog_category(td::telegram_api::object_ptr<td::telegram_api::TopPeerCategory>())).c_str());
    } else if (check_case == "name_size") {
      std::printf("after=%s\n", render_category(Category::Size).c_str());
    } else if (check_case == "input_size") {
      std::printf("after=%d\n", td::get_input_top_peer_category(Category::Size)->get_id());
    } else if (check_case == "ngt_null") {
      std::printf("after=%d\n",
                  static_cast<int>(td::get_notification_group_type(td::td_api::object_ptr<td::td_api::NotificationGroupType>())));
    } else if (check_case == "ngt_db_oor") {
      std::printf("after=%d\n", static_cast<int>(td::is_database_notification_group_type(static_cast<GroupType>(7))));
    } else if (check_case == "ngt_partial_oor") {
      std::printf("after=%d\n", static_cast<int>(td::is_partial_notification_group_type(static_cast<GroupType>(7))));
    } else if (check_case == "ngt_obj_oor") {
      std::printf("after=%d\n", td::get_notification_group_type_object(static_cast<GroupType>(7))->get_id());
    } else if (check_case == "ngt_print_oor") {
      std::printf("after=%s\n", render_group_type(static_cast<GroupType>(7)).c_str());
    } else {
      std::fprintf(stderr, "unknown check case\n");
      return 2;
    }
    std::fflush(stdout);
    return 0;
  }
  if (argc != 2) {
    std::fprintf(stderr, "usage: top_dialog_category_probe <script.txt>\n");
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
    if (command == "cat") {
      auto object = make_td_api_category(tokens[1]);
      auto category = td::get_top_dialog_category(object);
      std::printf("cat=%.*s id=%d enum=%d name=%s\n", static_cast<int>(tokens[1].size()), tokens[1].begin(),
                  object->get_id(), static_cast<int>(category), render_category(category).c_str());
    } else if (command == "catnull") {
      td::td_api::object_ptr<td::td_api::TopChatCategory> object;
      std::printf("catnull=enum=%d\n", static_cast<int>(td::get_top_dialog_category(object)));
    } else if (command == "cattl") {
      auto object = make_wire_category(tokens[1]);
      auto category = td::get_top_dialog_category(object);
      std::printf("cattl=%.*s enum=%d name=%s\n", static_cast<int>(tokens[1].size()), tokens[1].begin(),
                  static_cast<int>(category), render_category(category).c_str());
    } else if (command == "name") {
      auto category = parse_category(tokens[1]);
      std::printf("name=%d print=%s\n", static_cast<int>(category), render_category(category).c_str());
    } else if (command == "input") {
      auto category = parse_category(tokens[1]);
      std::printf("input=%d id=%d\n", static_cast<int>(category), td::get_input_top_peer_category(category)->get_id());
    } else if (command == "ngt") {
      auto type = parse_group_type(tokens[1]);
      std::printf("ngt=%d db=%d partial=%d obj=%d print=%s\n", static_cast<int>(type),
                  static_cast<int>(td::is_database_notification_group_type(type)),
                  static_cast<int>(td::is_partial_notification_group_type(type)),
                  make_td_api_group_type(type)->get_id(), render_group_type(type).c_str());
    } else if (command == "ngtfrom") {
      auto type = parse_group_type(tokens[1]);
      auto object = make_td_api_group_type(type);
      std::printf("ngtfrom=%d obj=%d back=%d\n", static_cast<int>(type), object->get_id(),
                  static_cast<int>(td::get_notification_group_type(object)));
    } else {
      std::fprintf(stderr, "unknown command %s\n", command.str().c_str());
      return 2;
    }
  }
  return 0;
}