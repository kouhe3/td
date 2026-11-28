// Oracle for the FileManager-free half of `td/telegram/DialogPhoto.{h,cpp}` plus
// `need_update_dialog_photo_minithumbnail` from `td/telegram/PhotoSize.cpp`.
//
//   dialog_photo_probe <script.txt>
//
// The other half of the header (get_profile_photo, get_dialog_photo,
// as_dialog_photo, as_profile_photo, is_same_dialog_photo, get_profile_photo_object,
// get_chat_photo_info_object, get_chat_photo_object) all take a live
// `FileManager *`. It cannot be faked: `FileManager` is `final : public Actor`
// (files/FileManager.h:397) and its constructor calls `G()->use_file_database()`
// (FileManager.cpp:1312), which LOG_CHECKs without a `Td`. That half is
// `blocked` in docs/facts.md F270; this probe only covers the rest.
//
// Script commands (blank lines and '#' comments ignored):
//   pid <none|empty|photo=<id>>       get_profile_photo_id
//   fake <photo> <dialog> <personal>  as_fake_dialog_photo
//   ids <photo> <dialog> <personal>   dialog_photo_get_file_ids
//   need <dp> <dp>                    need_update_dialog_photo
//   needp <pp> <pp>                   need_update_profile_photo
//   mneed <hex> <hex>                 need_update_dialog_photo_minithumbnail
//   print <dp> / printp <pp>          operator<<
//   eq <dp> <dp> / eqp <pp> <pp>      operator==
//   conv <tphoto> <personal>          convert_photo_to_profile_photo
//
// Specs are comma-separated key=value pairs:
//   dp: s=<small id>, b=<big id>, m=<hex>, a=<0|1>, p=<0|1>
//   pp: dp plus id=<profile photo id>
//   photo: id=<int64>, m=<hex>, sz=a3;c4;z0 (type char + file id), an=<count>
//   tphoto: id=<int64>, dc=<int>, sz=a;c;d (a/c are photoSize, d is photoSizeEmpty), v=<video count>
//
// Build:
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I tdactor -I tdnet -I C:/tmp/td-build/tdutils tools/fixtures/dialog_photo_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tdnet/libtdnet.a
//   C:/tmp/td-build/tdtl/libtdtl.a C:/tmp/td-build/tde2e/libtde2e.a
//   C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdutils/libtdutils.a
//   C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/sqlite/libtdsqlite.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi -lcrypt32 -lsecur32
//   -lmswsock -liphlpapi -ldnsapi -o C:/tmp/td-probe/dialog_photo_probe.exe
//
// Measured behaviour this probe encodes (docs/facts.md F270): need_update_dialog_photo
// ignores `minithumbnail` while operator== compares it; as_fake_dialog_photo returns
// an empty DialogPhoto as soon as either the 'a' or the 'c' file id is invalid; and
// need_update_dialog_photo_minithumbnail compares decoded sizes (leading 0x01 gives
// max(w, h)), not bytes.

#include "td/telegram/DialogPhoto.h"
#include "td/telegram/DialogId.h"
#include "td/telegram/Photo.h"
#include "td/telegram/PhotoSize.h"
#include "td/telegram/PhotoSizeType.h"
#include "td/telegram/UserId.h"
#include "td/telegram/files/FileManager.h"
#include "td/telegram/telegram_api.h"

#include "td/utils/Slice.h"
#include "td/utils/StringBuilder.h"
#include "td/utils/misc.h"

#include <cstdio>
#include <type_traits>
#include <cstring>
#include <fstream>
#include <string>
#include <vector>

using td::Slice;

// 这半边的回流条件：`FileManager` 一旦不再是 `final`，替身就有可能出现，
// F270 里那 8 个函数的 blocked 结论必须重判。
static_assert(std::is_final_v<td::FileManager>,
              "FileManager is no longer final: the blocked half of F270 may have become testable");

namespace {

td::string read_all(const char *path) {
  std::ifstream in(path, std::ios::binary);
  if (!in) {
    std::fprintf(stderr, "cannot open %s\n", path);
    std::exit(2);
  }
  return td::string(std::string((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>()));
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

td::string hex_encode(Slice data) {
  if (data.empty()) {
    return "-";
  }
  static const char *digits = "0123456789abcdef";
  td::string result;
  for (auto c : data) {
    result.push_back(digits[(static_cast<unsigned char>(c) >> 4) & 15]);
    result.push_back(digits[static_cast<unsigned char>(c) & 15]);
  }
  return result;
}

struct Field {
  Slice key;
  Slice value;
};

std::vector<Field> parse_fields(Slice spec) {
  std::vector<Field> fields;
  for (auto item : td::full_split(spec, ',')) {
    if (item.empty()) {
      continue;
    }
    auto eq = item.find('=');
    if (eq == Slice::npos) {
      fields.push_back({item, Slice()});
    } else {
      fields.push_back({item.substr(0, eq), item.substr(eq + 1)});
    }
  }
  return fields;
}

Slice field_value(const std::vector<Field> &fields, Slice key) {
  for (auto &field : fields) {
    if (field.key == key) {
      return field.value;
    }
  }
  return Slice();
}

td::int64 field_int(const std::vector<Field> &fields, Slice key, td::int64 fallback) {
  auto value = field_value(fields, key);
  return value.empty() ? fallback : td::to_integer<td::int64>(value);
}

td::string field_hex(const std::vector<Field> &fields, Slice key) {
  auto value = field_value(fields, key);
  return value.empty() ? td::string() : td::hex_decode(value).move_as_ok();
}

td::DialogPhoto parse_dialog_photo(Slice spec) {
  auto fields = parse_fields(spec);
  td::DialogPhoto photo;
  photo.small_file_id = td::FileId(static_cast<td::int32>(field_int(fields, "s", 0)), 0);
  photo.big_file_id = td::FileId(static_cast<td::int32>(field_int(fields, "b", 0)), 0);
  photo.minithumbnail = field_hex(fields, "m");
  photo.has_animation = field_int(fields, "a", 0) != 0;
  photo.is_personal = field_int(fields, "p", 0) != 0;
  return photo;
}

td::ProfilePhoto parse_profile_photo(Slice spec) {
  auto fields = parse_fields(spec);
  td::ProfilePhoto photo;
  static_cast<td::DialogPhoto &>(photo) = parse_dialog_photo(spec);
  photo.id = field_int(fields, "id", 0);
  return photo;
}

td::Photo parse_photo(Slice spec) {
  auto fields = parse_fields(spec);
  td::Photo photo;
  photo.id = td::MovableValue<td::int64, -2>(field_int(fields, "id", -2));
  photo.minithumbnail = field_hex(fields, "m");
  auto sizes = field_value(fields, "sz");
  for (auto item : td::full_split(sizes, ';')) {
    if (item.size() < 2) {
      continue;
    }
    td::PhotoSize size;
    size.type = td::PhotoSizeType(static_cast<td::int32>(item[0]));
    size.file_id = td::FileId(static_cast<td::int32>(td::to_integer<td::int32>(item.substr(1))), 0);
    photo.photos.push_back(size);
  }
  auto animations = field_int(fields, "an", 0);
  for (td::int64 i = 0; i < animations; i++) {
    photo.animations.push_back(td::AnimationSize());
  }
  return photo;
}

td::telegram_api::object_ptr<td::telegram_api::photo> parse_tphoto(Slice spec) {
  auto fields = parse_fields(spec);
  td::vector<td::telegram_api::object_ptr<td::telegram_api::PhotoSize>> sizes;
  for (auto item : td::full_split(field_value(fields, "sz"), ';')) {
    if (item.empty()) {
      continue;
    }
    if (item[0] == 'a' || item[0] == 'c') {
      sizes.push_back(td::make_tl_object<td::telegram_api::photoSize>(td::string(1, item[0]), 1, 1, 1));
    } else {
      sizes.push_back(td::make_tl_object<td::telegram_api::photoSizeEmpty>(td::string(1, item[0])));
    }
  }
  td::vector<td::telegram_api::object_ptr<td::telegram_api::VideoSize>> videos;
  auto video_count = field_int(fields, "v", 0);
  for (td::int64 i = 0; i < video_count; i++) {
    videos.push_back(td::make_tl_object<td::telegram_api::videoSize>(0, td::string("u"), 1, 1, 1, 0.0));
  }
  return td::make_tl_object<td::telegram_api::photo>(0 /*flags*/, false /*has_stickers*/,
                                                     field_int(fields, "id", 0), 0 /*access_hash*/,
                                                     td::BufferSlice(), 0 /*date*/, std::move(sizes),
                                                     std::move(videos), static_cast<td::int32>(field_int(fields, "dc", 0)));
}

td::string render_dialog_photo(const td::DialogPhoto &photo) {
  td::StringBuilder sb;
  sb << photo;
  return sb.as_cslice().str();
}

td::string render_profile_photo(const td::ProfilePhoto &photo) {
  td::StringBuilder sb;
  sb << photo;
  return sb.as_cslice().str();
}

td::int64 parse_dialog_id(Slice token) {
  return td::to_integer<td::int64>(token);
}

td::DialogId make_dialog_id(td::int64 user_id) {
  return td::DialogId(td::UserId(user_id));
}

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc != 2) {
    std::fprintf(stderr, "usage: dialog_photo_probe <script.txt>\n");
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
    if (command == "pid") {
      auto spec = tokens[1];
      td::telegram_api::object_ptr<td::telegram_api::UserProfilePhoto> profile_photo;
      if (spec == "empty") {
        profile_photo = td::make_tl_object<td::telegram_api::userProfilePhotoEmpty>();
      } else if (spec != "none") {
        auto fields = parse_fields(spec);
        profile_photo = td::make_tl_object<td::telegram_api::userProfilePhoto>(
            0, false, false, field_int(fields, "photo", 0), td::BufferSlice(), 0);
      }
      std::printf("pid=%lld\n", static_cast<long long>(td::get_profile_photo_id(profile_photo)));
    } else if (command == "fake") {
      auto photo = parse_photo(tokens[1]);
      auto dialog_id = make_dialog_id(parse_dialog_id(tokens[2]));
      auto is_personal = td::to_integer<td::int32>(tokens[3]) != 0;
      auto result = td::as_fake_dialog_photo(photo, dialog_id, is_personal);
      std::printf("fake=s=%d b=%d mini=%s an=%d p=%d\n", result.small_file_id.get(), result.big_file_id.get(),
                  hex_encode(result.minithumbnail).c_str(), static_cast<int>(result.has_animation),
                  static_cast<int>(result.is_personal));
    } else if (command == "ids") {
      auto photo = parse_photo(tokens[1]);
      auto dialog_id = make_dialog_id(parse_dialog_id(tokens[2]));
      auto is_personal = td::to_integer<td::int32>(tokens[3]) != 0;
      auto result = td::as_fake_dialog_photo(photo, dialog_id, is_personal);
      auto ids = td::dialog_photo_get_file_ids(result);
      std::printf("ids=%zu", ids.size());
      for (auto &id : ids) {
        std::printf(":%d", id.get());
      }
      std::printf("\n");
    } else if (command == "need") {
      auto from = parse_dialog_photo(tokens[1]);
      auto to = parse_dialog_photo(tokens[2]);
      std::printf("need=%d\n", static_cast<int>(td::need_update_dialog_photo(from, to)));
    } else if (command == "needp") {
      auto from = parse_profile_photo(tokens[1]);
      auto to = parse_profile_photo(tokens[2]);
      std::printf("needp=%d\n", static_cast<int>(td::need_update_profile_photo(from, to)));
    } else if (command == "mneed") {
      auto decode = [](Slice token) { return token == "-" ? td::string() : td::hex_decode(token).move_as_ok(); };
      auto from = decode(tokens[1]);
      auto to = decode(tokens[2]);
      std::printf("mneed=%d\n", static_cast<int>(td::need_update_dialog_photo_minithumbnail(from, to)));
    } else if (command == "print") {
      std::printf("print=%s\n", render_dialog_photo(parse_dialog_photo(tokens[1])).c_str());
    } else if (command == "printp") {
      std::printf("printp=%s\n", render_profile_photo(parse_profile_photo(tokens[1])).c_str());
    } else if (command == "eq") {
      auto lhs = parse_dialog_photo(tokens[1]);
      auto rhs = parse_dialog_photo(tokens[2]);
      std::printf("eq=%d ne=%d\n", static_cast<int>(lhs == rhs), static_cast<int>(lhs != rhs));
    } else if (command == "eqp") {
      auto lhs = parse_profile_photo(tokens[1]);
      auto rhs = parse_profile_photo(tokens[2]);
      std::printf("eqp=%d nep=%d\n", static_cast<int>(lhs == rhs), static_cast<int>(lhs != rhs));
    } else if (command == "conv") {
      auto photo = parse_tphoto(tokens[1]);
      auto is_personal = td::to_integer<td::int32>(tokens[2]) != 0;
      auto result = td::convert_photo_to_profile_photo(photo, is_personal);
      if (result == nullptr) {
        std::printf("conv=null\n");
      } else {
        std::printf("conv=flags=%d video=%d personal=%d photo=%lld stripped=%zu dc=%d\n", result->flags_,
                    static_cast<int>(result->has_video_), static_cast<int>(result->personal_),
                    static_cast<long long>(result->photo_id_), result->stripped_thumb_.size(), result->dc_id_);
      }
    } else {
      std::fprintf(stderr, "unknown command %s\n", command.str().c_str());
      return 2;
    }
  }
  return 0;
}