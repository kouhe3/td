// Oracle for `td/telegram/PhotoSizeSource.{h,cpp}`, driven by a script:
//
//   photo_size_source_probe <script.txt>
//   photo_size_source_probe --check <case>    one fixed case per process, only
//                                            to see whether it aborts:
//     filetype_full_legacy   get_file_type(FullLegacy)        UNREACHABLE
//     name_ss_legacy         get_unique_name(StickerSetLegacy) UNREACHABLE
//     unique_thumb_wide      get_unique(thumbnail type 200)   LOG(FATAL)
//     type_default           get_type(default source, offset -1) LOG_CHECK
//
// Specs:
//   t:<file_type>:<thumb_type>                       thumbnail
//   d:<dialog>:<hash>:<0|1>                          dialog photo (small/big)
//   f:<volume>:<local>:<secret>                      full legacy
//   dl:<dialog>:<hash>:<0|1>:<volume>:<local>        dialog photo legacy
//   sl:<set>:<set_hash>:<volume>:<local>             sticker set thumbnail legacy
//   sv:<set>:<set_hash>:<version>                    sticker set thumbnail version
// `dialog` is u<id> (user), c<id> (chat), n<id> (channel), s<id> (secret chat), z (none).
// `l:` is a default-constructed source (variant offset -1).
//
// Build (`PhotoSizeSource.cpp` is in libtdcore.a; `DialogId` pulls the id
// headers, so the full static set is the safe link line):
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I tdactor -I tdnet -I C:/tmp/td-build/tdutils tools/fixtures/photo_size_source_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tdnet/libtdnet.a
//   C:/tmp/td-build/tdtl/libtdtl.a C:/tmp/td-build/tde2e/libtde2e.a
//   C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdutils/libtdutils.a
//   C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/sqlite/libtdsqlite.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi -lcrypt32 -lsecur32
//   -lmswsock -liphlpapi -ldnsapi -o C:/tmp/td-probe/photo_size_source_probe.exe
//
// Measured behaviour this probe encodes (docs/facts.md F254): only eight of the
// ten variants can be built, `get_compare_type` folds thumbnails into `type + 5`
// with `'a'`/`'c'` special cases, the unique form is one byte or 12/13 bytes of
// TL-encoded ids, the unique name prints the thumbnail type in decimal, and the
// chat branch of `get_input_peer` drops the access hash.

#include "td/telegram/PhotoSizeSource.h"

#include "td/telegram/ChannelId.h"
#include "td/telegram/ChatId.h"
#include "td/telegram/DialogId.h"
#include "td/telegram/UserId.h"
#include "td/telegram/files/FileType.h"
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

std::vector<Slice> split_colon(Slice text) {
  std::vector<Slice> parts;
  size_t start = 0;
  for (size_t i = 0; i <= text.size(); i++) {
    if (i == text.size() || text[i] == ':') {
      parts.push_back(text.substr(start, i - start));
      start = i + 1;
    }
  }
  return parts;
}

td::int64 to_i64(Slice token) {
  return td::to_integer<td::int64>(token);
}

td::int32 to_i32(Slice token) {
  return td::to_integer<td::int32>(token);
}

td::DialogId parse_dialog(Slice token) {
  if (token == "z" || token.empty()) {
    return td::DialogId();
  }
  switch (token[0]) {
    case 'u':
      return td::DialogId(td::UserId(to_i64(token.substr(1))));
    case 'c':
      return td::DialogId(td::ChatId(to_i64(token.substr(1))));
    case 'n':
      return td::DialogId(td::ChannelId(to_i64(token.substr(1))));
    case 's':
      return td::DialogId(td::SecretChatId(to_i32(token.substr(1))));
    default:
      std::fprintf(stderr, "bad dialog spec\n");
      std::exit(2);
  }
}

td::PhotoSizeSource parse_source(Slice spec) {
  auto parts = split_colon(spec);
  auto kind = parts[0];
  if (kind == "l" ) {
    return td::PhotoSizeSource();
  }
  if (kind == "t" && parts.size() == 3) {
    return td::PhotoSizeSource::thumbnail(static_cast<td::FileType>(to_i32(parts[1])), to_i32(parts[2]));
  }
  if (kind == "d" && parts.size() == 4) {
    return td::PhotoSizeSource::dialog_photo(parse_dialog(parts[1]), to_i64(parts[2]), parts[3] == "1");
  }
  if (kind == "f" && parts.size() == 4) {
    return td::PhotoSizeSource::full_legacy(to_i64(parts[1]), to_i32(parts[2]), to_i64(parts[3]));
  }
  if (kind == "dl" && parts.size() == 6) {
    return td::PhotoSizeSource::dialog_photo_legacy(parse_dialog(parts[1]), to_i64(parts[2]), parts[3] == "1",
                                                    to_i64(parts[4]), to_i32(parts[5]));
  }
  if (kind == "sl" && parts.size() == 5) {
    return td::PhotoSizeSource::sticker_set_thumbnail_legacy(to_i64(parts[1]), to_i64(parts[2]), to_i64(parts[3]),
                                                             to_i32(parts[4]));
  }
  if (kind == "sv" && parts.size() == 4) {
    return td::PhotoSizeSource::sticker_set_thumbnail(to_i64(parts[1]), to_i64(parts[2]), to_i32(parts[3]));
  }
  std::fprintf(stderr, "bad source spec\n");
  std::exit(2);
}

string hex(Slice data) {
  return td::hex_encode(data);
}

void print_source(Slice spec) {
  auto source = parse_source(spec);
  td::StringBuilder sb;
  sb << source;
  std::printf("print type=%d text=%s\n", static_cast<int>(source.get_type("probe")), sb.as_cslice().str().c_str());
}

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc == 3 && std::strcmp(argv[1], "--check") == 0) {
    Slice check_case(argv[2]);
    std::printf("before\n");
    std::fflush(stdout);
    if (check_case == "filetype_full_legacy") {
      std::printf("after filetype=%d\n", static_cast<int>(parse_source(Slice("f:1:2:3")).get_file_type("probe")));
    } else if (check_case == "name_ss_legacy") {
      std::printf("after=%s\n", parse_source(Slice("sl:1:2:3:4")).get_unique_name(1, "probe").c_str());
    } else if (check_case == "unique_thumb_wide") {
      std::printf("after=%s\n", hex(parse_source(Slice("t:0:200")).get_unique("probe")).c_str());
    } else if (check_case == "type_default") {
      std::printf("after type=%d\n", static_cast<int>(td::PhotoSizeSource().get_type("probe")));
    } else {
      std::fprintf(stderr, "unknown check case\n");
      return 2;
    }
    std::fflush(stdout);
    return 0;
  }
  if (argc != 2) {
    std::fprintf(stderr, "usage: photo_size_source_probe <script.txt>\n");
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
    if (command == "print") {
      print_source(tokens[1]);
    } else if (command == "filetype") {
      auto source = parse_source(tokens[1]);
      auto file_type = source.get_file_type("probe");
      std::printf("filetype=%d\n", static_cast<int>(file_type));
    } else if (command == "unique") {
      auto source = parse_source(tokens[1]);
      auto unique = source.get_unique("probe");
      std::printf("unique len=%zu hex=%s\n", unique.size(), hex(unique).c_str());
    } else if (command == "less") {
      auto lhs = parse_source(tokens[1]);
      auto rhs = parse_source(tokens[2]);
      std::printf("less=%d\n", static_cast<int>(td::PhotoSizeSource::unique_less(lhs, rhs)));
    } else if (command == "ueq") {
      auto lhs = parse_source(tokens[1]);
      auto rhs = parse_source(tokens[2]);
      std::printf("ueq=%d\n", static_cast<int>(td::PhotoSizeSource::unique_equal(lhs, rhs)));
    } else if (command == "name") {
      auto source = parse_source(tokens[2]);
      auto name = source.get_unique_name(to_i64(tokens[1]), "probe");
      std::printf("name=%s\n", name.c_str());
    } else if (command == "eq") {
      auto lhs = parse_source(tokens[1]);
      auto rhs = parse_source(tokens[2]);
      std::printf("eq eq=%d ne=%d\n", static_cast<int>(lhs == rhs), static_cast<int>(lhs != rhs));
    } else if (command == "peer") {
      auto source = parse_source(tokens[1]);
      auto peer = source.dialog_photo().get_input_peer();
      if (peer == nullptr) {
        std::printf("peer=null\n");
      } else {
        switch (peer->get_id()) {
          case td::telegram_api::inputPeerUser::ID: {
            auto *user = static_cast<td::telegram_api::inputPeerUser *>(peer.get());
            std::printf("peer=user id=%lld hash=%lld\n", static_cast<long long>(user->user_id_),
                        static_cast<long long>(user->access_hash_));
            break;
          }
          case td::telegram_api::inputPeerChat::ID: {
            auto *chat = static_cast<td::telegram_api::inputPeerChat *>(peer.get());
            std::printf("peer=chat id=%lld\n", static_cast<long long>(chat->chat_id_));
            break;
          }
          case td::telegram_api::inputPeerChannel::ID: {
            auto *channel = static_cast<td::telegram_api::inputPeerChannel *>(peer.get());
            std::printf("peer=channel id=%lld hash=%lld\n", static_cast<long long>(channel->channel_id_),
                        static_cast<long long>(channel->access_hash_));
            break;
          }
          case td::telegram_api::inputPeerEmpty::ID:
            std::printf("peer=empty\n");
            break;
          default:
            std::printf("peer=other\n");
        }
      }
    } else if (command == "sset") {
      auto source = parse_source(tokens[1]);
      auto set = source.sticker_set_thumbnail().get_input_sticker_set();
      auto *by_id = static_cast<td::telegram_api::inputStickerSetID *>(set.get());
      std::printf("sset id=%lld hash=%lld\n", static_cast<long long>(by_id->id_),
                  static_cast<long long>(by_id->access_hash_));
    } else {
      std::fprintf(stderr, "unknown command %s\n", command.str().c_str());
      return 2;
    }
  }
  return 0;
}