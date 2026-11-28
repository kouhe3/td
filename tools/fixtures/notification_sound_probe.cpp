// Oracle for `td/telegram/NotificationSound.{h,cpp}`, driven by a script:
//
//   notification_sound_probe <script.txt>
//   notification_sound_probe --check store   LogEventStorerCalcLength/Unsafe
//   notification_sound_probe --check parse   LogEventParser
//
// Sound specs: `-` (null), `none`, `local:<title>|<data>`, `ring:<id>`.
// Strings accept `hex-<bytes>` so the corpus can feed control characters.
//
// The two `--check` cases exist because the log-event storers and parser call
// `set_context(G())` in their constructors and `G()` LOG_CHECKs the actor
// context, so they cannot be reached offline: both sides must abort.
//
// Build (NotificationSound.cpp is in libtdcore.a; `LogEvent.h` pulls
// `Global.h` -> `td/net/NetStats.h`, so tdnet and the full static set join):
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I tdactor -I tdnet -I C:/tmp/td-build/tdutils tools/fixtures/notification_sound_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tdnet/libtdnet.a
//   C:/tmp/td-build/tdtl/libtdtl.a C:/tmp/td-build/tde2e/libtde2e.a
//   C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdutils/libtdutils.a
//   C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/sqlite/libtdsqlite.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi -lcrypt32 -lsecur32
//   -lmswsock -liphlpapi -ldnsapi -o C:/tmp/td-probe/notification_sound_probe.exe
//
// Measured behaviour this probe encodes (docs/facts.md F249): a Local sound
// counts as the default, so equivalence never reaches the Local branch; a
// ringtone ID of 0 or -1 becomes NoSound (LOG(ERROR), not a CHECK); and the
// legacy string maps "default"/""/anything-else to null/None/Local(s, s).

#include "td/telegram/NotificationSound.h"
#include "td/telegram/NotificationSoundType.h"
#include "td/telegram/logevent/LogEvent.h"
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
using td::unique_ptr;
using td::NotificationSound;

// Only the oracle can build these three classes, so the probe keeps its own
// shapes and rebuilds them through the public factories where possible.
struct SoundSpec {
  enum class Kind { Null, None, Local, Ringtone } kind = Kind::Null;
  string title;
  string data;
  td::int64 ringtone_id = 0;
};

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

int hex_digit(char c) {
  if (c >= '0' && c <= '9') {
    return c - '0';
  }
  if (c >= 'a' && c <= 'f') {
    return c - 'a' + 10;
  }
  return -1;
}

string unhex(Slice hex) {
  string out;
  if (hex.size() % 2 != 0) {
    std::fprintf(stderr, "bad hex\n");
    std::exit(2);
  }
  for (size_t i = 0; i < hex.size(); i += 2) {
    out.push_back(static_cast<char>((hex_digit(hex[i]) << 4) | hex_digit(hex[i + 1])));
  }
  return out;
}

string parse_text(Slice token) {
  if (token == "-") {
    return string();
  }
  if (token.size() > 4 && token.substr(0, 4) == "hex-") {
    return unhex(token.substr(4));
  }
  return token.str();
}

SoundSpec parse_spec(Slice token) {
  SoundSpec spec;
  if (token == "-") {
    spec.kind = SoundSpec::Kind::Null;
  } else if (token == "none") {
    spec.kind = SoundSpec::Kind::None;
  } else if (token.size() > 6 && token.substr(0, 6) == "local:") {
    auto body = token.substr(6);
    auto bar = body.find('|');
    if (bar == Slice::npos) {
      std::fprintf(stderr, "bad local spec\n");
      std::exit(2);
    }
    spec.kind = SoundSpec::Kind::Local;
    spec.title = parse_text(body.substr(0, bar));
    spec.data = parse_text(body.substr(bar + 1));
  } else if (token.size() > 5 && token.substr(0, 5) == "ring:") {
    spec.kind = SoundSpec::Kind::Ringtone;
    spec.ringtone_id = td::to_integer<td::int64>(token.substr(5));
  } else {
    std::fprintf(stderr, "bad sound spec\n");
    std::exit(2);
  }
  return spec;
}

unique_ptr<NotificationSound> build(const SoundSpec &spec) {
  switch (spec.kind) {
    case SoundSpec::Kind::Null:
      return nullptr;
    case SoundSpec::Kind::None:
      return td::get_legacy_notification_sound(string());
    case SoundSpec::Kind::Local:
      return td::get_notification_sound(
          td::telegram_api::make_object<td::telegram_api::notificationSoundLocal>(spec.title, spec.data).get());
    case SoundSpec::Kind::Ringtone:
      return td::get_notification_sound(false, spec.ringtone_id);
  }
  return nullptr;
}

string render(const unique_ptr<NotificationSound> &sound) {
  td::StringBuilder sb;
  sb << sound;
  return sb.as_cslice().str();
}

const char *type_name(const NotificationSound *sound) {
  if (sound == nullptr) {
    return "null";
  }
  switch (sound->get_type()) {
    case td::NotificationSoundType::None:
      return "none";
    case td::NotificationSoundType::Local:
      return "local";
    case td::NotificationSoundType::Ringtone:
      return "ringtone";
  }
  return "?";
}

// `%s` would stop at an embedded NUL, and the corpus feeds one on purpose.
void print_sound(const char *prefix, const unique_ptr<NotificationSound> &sound) {
  std::printf("%s type=%s text=", prefix, type_name(sound.get()));
  auto text = render(sound);
  std::fwrite(text.data(), 1, text.size(), stdout);
  std::fputc('\n', stdout);
}

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc == 3 && std::strcmp(argv[1], "--check") == 0) {
    Slice check_case(argv[2]);
    auto sound = build(parse_spec(Slice("ring:7")));
    if (check_case == "store") {
      std::printf("before type=%s\n", type_name(sound.get()));
      std::fflush(stdout);
      td::LogEventStorerCalcLength calc;
      td::store_notification_sound(sound.get(), calc);
      std::printf("after len=%zu\n", calc.get_length());
      std::fflush(stdout);
      return 0;
    }
    if (check_case == "parse") {
      std::printf("before\n");
      std::fflush(stdout);
      unique_ptr<NotificationSound> parsed;
      td::LogEventParser parser(Slice("\x00\x00\x00\x00"));
      td::parse_notification_sound(parsed, parser);
      std::printf("after type=%s\n", type_name(parsed.get()));
      std::fflush(stdout);
      return 0;
    }
    std::fprintf(stderr, "unknown check case\n");
    return 2;
  }
  if (argc != 2) {
    std::fprintf(stderr, "usage: notification_sound_probe <script.txt>\n");
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
      auto sound = build(parse_spec(tokens[1]));
      print_sound("print", sound);
    } else if (command == "def") {
      auto lhs = build(parse_spec(tokens[1]));
      auto rhs = build(parse_spec(tokens[2]));
      std::printf("def lhs=%d rhs=%d\n", static_cast<int>(td::is_notification_sound_default(lhs)),
                  static_cast<int>(td::is_notification_sound_default(rhs)));
    } else if (command == "eq") {
      auto lhs = build(parse_spec(tokens[1]));
      auto rhs = build(parse_spec(tokens[2]));
      std::printf("eq=%d\n", static_cast<int>(td::are_equivalent_notification_sounds(lhs, rhs)));
    } else if (command == "diff") {
      auto lhs = build(parse_spec(tokens[1]));
      auto rhs = build(parse_spec(tokens[2]));
      std::printf("diff=%d\n", static_cast<int>(td::are_different_equivalent_notification_sounds(lhs, rhs)));
    } else if (command == "rid") {
      auto sound = build(parse_spec(tokens[1]));
      std::printf("rid=%lld\n", static_cast<long long>(td::get_notification_sound_ringtone_id(sound)));
    } else if (command == "legacy") {
      auto sound = td::get_legacy_notification_sound(parse_text(tokens[1]));
      print_sound("legacy", sound);
    } else if (command == "boolsound") {
      auto sound = td::get_notification_sound(tokens[1] == "1", td::to_integer<td::int64>(tokens[2]));
      print_sound("boolsound", sound);
    } else if (command == "tl") {
      td::telegram_api::object_ptr<td::telegram_api::NotificationSound> object;
      auto kind = tokens[1];
      if (kind == "null") {
        object = nullptr;
      } else if (kind == "default") {
        object = td::telegram_api::make_object<td::telegram_api::notificationSoundDefault>();
      } else if (kind == "none") {
        object = td::telegram_api::make_object<td::telegram_api::notificationSoundNone>();
      } else if (kind == "local") {
        object = td::telegram_api::make_object<td::telegram_api::notificationSoundLocal>(
            parse_text(tokens[2]), parse_text(tokens[3]));
      } else if (kind == "ring") {
        object = td::telegram_api::make_object<td::telegram_api::notificationSoundRingtone>(
            td::to_integer<td::int64>(tokens[2]));
      } else {
        std::fprintf(stderr, "bad tl kind\n");
        return 2;
      }
      auto sound = td::get_notification_sound(object.get());
      print_sound("tl", sound);
    } else if (command == "input") {
      auto sound = build(parse_spec(tokens[1]));
      auto object = td::get_input_notification_sound(sound, tokens[2] == "1");
      if (object == nullptr) {
        std::printf("input=null\n");
      } else {
        switch (object->get_id()) {
          case td::telegram_api::notificationSoundDefault::ID:
            std::printf("input=default\n");
            break;
          case td::telegram_api::notificationSoundNone::ID:
            std::printf("input=none\n");
            break;
          case td::telegram_api::notificationSoundLocal::ID: {
            auto *local = static_cast<td::telegram_api::notificationSoundLocal *>(object.get());
            std::printf("input=local title=%s data=%s\n", td::hex_encode(local->title_).c_str(),
                        td::hex_encode(local->data_).c_str());
            break;
          }
          case td::telegram_api::notificationSoundRingtone::ID: {
            auto *ring = static_cast<td::telegram_api::notificationSoundRingtone *>(object.get());
            std::printf("input=ring id=%lld\n", static_cast<long long>(ring->id_));
            break;
          }
          default:
            std::printf("input=other\n");
        }
      }
    } else if (command == "dup") {
      auto sound = build(parse_spec(tokens[1]));
      auto copy = td::dup_notification_sound(sound);
      print_sound("dup", copy);
    } else {
      std::fprintf(stderr, "unknown command %s\n", command.str().c_str());
      return 2;
    }
  }
  return 0;
}