// Oracle for td::ReactionType (td/telegram/ReactionType.{h,cpp}) driven by a
// script. No Td, no actor, no clock -- the whole class is a string plus two
// decoders and three encoders.
//
//   reaction_type_probe <script.txt>
//   reaction_type_probe --check <case>     one process per case, prints one token
//
// Specs (the same grammar the Rust side uses):
//   -            default-constructed (empty)
//   $            ReactionType::paid()
//   e:<hex>      ReactionType(string(hex)) -- bypasses every guard, so it can
//                build `#zz` and other strings the decoders would refuse
//   c:<id>       through td_api::reactionTypeCustomEmoji(id)
//   tl:empty | tl:emoji:<hex> | tl:custom:<id> | tl:paid
//   td:null | td:emoji:<hex> | td:custom:<id> | td:paid
//
// Commands:
//   print <spec>              is_empty / is_custom_reaction / is_paid_reaction / operator<<
//   hash <spec>               get_hash()
//   cmp <spec_a> <spec_b>     operator< / == / !=
//   input <spec>              get_input_reaction() -> id + payload
//   obj <spec>                get_reaction_type_object() -> null or id + payload
//   upd <spec>                get_update_default_reaction_type() -> null or id
//   vec <0|1> <spec>...       get_reaction_types_object(vec, paid_available)
//   vechash <spec>...         get_reaction_types_hash(vec)
//
// Measured behaviour this probe encodes (docs/facts.md F287/F288):
//   * one string encodes four states: empty, emoji, `#` + base64(8 B LE int64
//     custom_emoji_id), and `$` for paid;
//   * `operator<` special-cases paid to the front; everything else compares the
//     raw string bytes, so the order is `$` < "" < "#..." < emoji;
//   * an empty reaction maps to reactionEmpty on the wire but to nullptr on both
//     td_api exits;
//   * `reactionEmoji` whose emoticon starts with '#' or equals "$" is cleared, and
//     `reactionTypeEmoji` additionally has to pass check_utf8;
//   * get_hash() is the raw document id for a custom emoji and
//     md5(remove_emoji_selectors(s)) big-endian in its first 8 bytes otherwise;
//   * get_custom_emoji_id CHECKs both the base64 decode and the 8-byte length,
//     which is what `--check bad_b64|short_b64` exercises.
//
// Build from the repo root against a configured C:/tmp/td-build:
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I tdactor -I tdnet -I C:/tmp/td-build/tdutils tools/fixtures/reaction_type_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tdnet/libtdnet.a
//   C:/tmp/td-build/tdtl/libtdtl.a C:/tmp/td-build/tde2e/libtde2e.a
//   C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi -lcrypt32 -lsecur32
//   -lmswsock -liphlpapi -ldnsapi -o C:/tmp/td-probe/reaction_type_probe.exe

#include "td/telegram/ReactionType.h"

#include "td/telegram/td_api.h"
#include "td/telegram/telegram_api.h"

#include "td/utils/StringBuilder.h"

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <string>
#include <vector>

namespace {

using td::ReactionType;

bool parse_i64(const std::string &s, int64_t &out) {
  if (s.empty()) {
    return false;
  }
  errno = 0;
  char *end = nullptr;
  long long v = std::strtoll(s.c_str(), &end, 10);
  if (errno != 0 || end != s.c_str() + s.size()) {
    return false;
  }
  out = v;
  return true;
}

std::vector<std::string> split(const std::string &line) {
  std::vector<std::string> out;
  size_t pos = 0;
  while (pos < line.size()) {
    while (pos < line.size() && (line[pos] == ' ' || line[pos] == '\t')) {
      pos++;
    }
    if (pos == line.size()) {
      break;
    }
    size_t start = pos;
    while (pos < line.size() && line[pos] != ' ' && line[pos] != '\t') {
      pos++;
    }
    out.push_back(line.substr(start, pos - start));
  }
  return out;
}

bool from_hex(const std::string &h, std::string &out) {
  out.clear();
  if (h == "-") {
    return true;
  }
  if (h.size() % 2 != 0) {
    return false;
  }
  for (size_t i = 0; i < h.size(); i += 2) {
    auto nib = [](char c) -> int {
      if (c >= '0' && c <= '9') {
        return c - '0';
      }
      if (c >= 'a' && c <= 'f') {
        return c - 'a' + 10;
      }
      return -1;
    };
    int hi = nib(h[i]);
    int lo = nib(h[i + 1]);
    if (hi < 0 || lo < 0) {
      return false;
    }
    out.push_back(static_cast<char>((hi << 4) | lo));
  }
  return true;
}

std::string to_hex(const std::string &s) {
  static const char *digits = "0123456789abcdef";
  if (s.empty()) {
    return "-";
  }
  std::string out;
  out.reserve(s.size() * 2);
  for (unsigned char c : s) {
    out += digits[c >> 4];
    out += digits[c & 15];
  }
  return out;
}

td::string printed(const ReactionType &r) {
  td::StringBuilder sb;
  sb << r;
  return sb.as_cslice().str();
}

/// Parses a spec into a ReactionType; false means "malformed spec".
bool make(const std::string &spec, ReactionType &out) {
  if (spec == "-") {
    out = ReactionType();
    return true;
  }
  if (spec == "$") {
    out = ReactionType::paid();
    return true;
  }
  if (spec.size() > 2 && spec.compare(0, 2, "e:") == 0) {
    std::string bytes;
    if (!from_hex(spec.substr(2), bytes)) {
      return false;
    }
    out = ReactionType(std::move(bytes));
    return true;
  }
  if (spec.size() > 2 && spec.compare(0, 2, "c:") == 0) {
    int64_t id;
    if (!parse_i64(spec.substr(2), id)) {
      return false;
    }
    auto object = td::td_api::make_object<td::td_api::reactionTypeCustomEmoji>(id);
    out = ReactionType(td::td_api::object_ptr<td::td_api::ReactionType>(std::move(object)));
    return true;
  }
  if (spec == "tl:empty") {
    auto object = td::telegram_api::make_object<td::telegram_api::reactionEmpty>();
    out = ReactionType(td::telegram_api::object_ptr<td::telegram_api::Reaction>(std::move(object)));
    return true;
  }
  if (spec == "tl:paid") {
    auto object = td::telegram_api::make_object<td::telegram_api::reactionPaid>();
    out = ReactionType(td::telegram_api::object_ptr<td::telegram_api::Reaction>(std::move(object)));
    return true;
  }
  if (spec.size() > 9 && spec.compare(0, 9, "tl:emoji:") == 0) {
    std::string bytes;
    if (!from_hex(spec.substr(9), bytes)) {
      return false;
    }
    auto object = td::telegram_api::make_object<td::telegram_api::reactionEmoji>(bytes);
    out = ReactionType(td::telegram_api::object_ptr<td::telegram_api::Reaction>(std::move(object)));
    return true;
  }
  if (spec.size() > 10 && spec.compare(0, 10, "tl:custom:") == 0) {
    int64_t id;
    if (!parse_i64(spec.substr(10), id)) {
      return false;
    }
    auto object = td::telegram_api::make_object<td::telegram_api::reactionCustomEmoji>(id);
    out = ReactionType(td::telegram_api::object_ptr<td::telegram_api::Reaction>(std::move(object)));
    return true;
  }
  if (spec == "td:null") {
    td::td_api::object_ptr<td::td_api::ReactionType> object;
    out = ReactionType(object);
    return true;
  }
  if (spec == "td:paid") {
    auto object = td::td_api::make_object<td::td_api::reactionTypePaid>();
    out = ReactionType(td::td_api::object_ptr<td::td_api::ReactionType>(std::move(object)));
    return true;
  }
  if (spec.size() > 9 && spec.compare(0, 9, "td:emoji:") == 0) {
    std::string bytes;
    if (!from_hex(spec.substr(9), bytes)) {
      return false;
    }
    auto object = td::td_api::make_object<td::td_api::reactionTypeEmoji>(bytes);
    out = ReactionType(td::td_api::object_ptr<td::td_api::ReactionType>(std::move(object)));
    return true;
  }
  if (spec.size() > 10 && spec.compare(0, 10, "td:custom:") == 0) {
    int64_t id;
    if (!parse_i64(spec.substr(10), id)) {
      return false;
    }
    auto object = td::td_api::make_object<td::td_api::reactionTypeCustomEmoji>(id);
    out = ReactionType(td::td_api::object_ptr<td::td_api::ReactionType>(std::move(object)));
    return true;
  }
  return false;
}

const char *reaction_name(int id) {
  switch (id) {
    case td::telegram_api::reactionEmpty::ID:
      return "empty";
    case td::telegram_api::reactionEmoji::ID:
      return "emoji";
    case td::telegram_api::reactionCustomEmoji::ID:
      return "custom";
    case td::telegram_api::reactionPaid::ID:
      return "paid";
    default:
      return "unknown";
  }
}

const char *td_reaction_name(int id) {
  switch (id) {
    case td::td_api::reactionTypeEmoji::ID:
      return "emoji";
    case td::td_api::reactionTypeCustomEmoji::ID:
      return "custom";
    case td::td_api::reactionTypePaid::ID:
      return "paid";
    default:
      return "unknown";
  }
}

std::string input_line(const ReactionType &r) {
  auto object = r.get_input_reaction();
  if (object == nullptr) {
    return "id=-1 unknown";
  }
  std::string out = "id=" + std::to_string(object->get_id()) + " " + reaction_name(object->get_id());
  switch (object->get_id()) {
    case td::telegram_api::reactionEmoji::ID:
      out += " emoji=" + to_hex(static_cast<const td::telegram_api::reactionEmoji *>(object.get())->emoticon_);
      break;
    case td::telegram_api::reactionCustomEmoji::ID:
      out += " doc=" +
             std::to_string(static_cast<const td::telegram_api::reactionCustomEmoji *>(object.get())->document_id_);
      break;
    default:
      break;
  }
  return out;
}

std::string object_line(const ReactionType &r) {
  auto object = r.get_reaction_type_object();
  if (object == nullptr) {
    return "null";
  }
  std::string out = std::to_string(object->get_id()) + " " + td_reaction_name(object->get_id());
  switch (object->get_id()) {
    case td::td_api::reactionTypeEmoji::ID:
      out += " emoji=" + to_hex(static_cast<const td::td_api::reactionTypeEmoji *>(object.get())->emoji_);
      break;
    case td::td_api::reactionTypeCustomEmoji::ID:
      out += " doc=" +
             std::to_string(static_cast<const td::td_api::reactionTypeCustomEmoji *>(object.get())->custom_emoji_id_);
      break;
    default:
      break;
  }
  return out;
}

std::string vector_object_line(const std::vector<ReactionType> &types, bool paid_available) {
  auto objects = ReactionType::get_reaction_types_object(types, paid_available);
  std::string out = "[";
  bool first = true;
  for (auto &object : objects) {
    if (object == nullptr) {
      out += (first ? "" : ",") + std::string("null");
    } else {
      out += first ? "" : ",";
      out += td_reaction_name(object->get_id());
      if (object->get_id() == td::td_api::reactionTypeEmoji::ID) {
        out += ":" + to_hex(static_cast<const td::td_api::reactionTypeEmoji *>(object.get())->emoji_);
      } else if (object->get_id() == td::td_api::reactionTypeCustomEmoji::ID) {
        out += ":" +
               std::to_string(static_cast<const td::td_api::reactionTypeCustomEmoji *>(object.get())->custom_emoji_id_);
      }
    }
    first = false;
  }
  return out + "]";
}

int check_case(const std::string &name) {
  if (name == "bad_b64") {
    ReactionType r(std::move(std::string("#zz")));
    std::printf("hash=%llu\n", static_cast<unsigned long long>(r.get_hash()));
    return 0;
  }
  if (name == "short_b64") {
    ReactionType r(std::move(std::string("#abc")));
    std::printf("hash=%llu\n", static_cast<unsigned long long>(r.get_hash()));
    return 0;
  }
  if (name == "ok_custom") {
    auto object = td::td_api::make_object<td::td_api::reactionTypeCustomEmoji>(int64_t{1});
    ReactionType r(td::td_api::object_ptr<td::td_api::ReactionType>(std::move(object)));
    std::printf("hash=%llu\n", static_cast<unsigned long long>(r.get_hash()));
    return 0;
  }
  std::fprintf(stderr, "unknown check case %s\n", name.c_str());
  return 2;
}

int run_script(const char *script_path) {
  std::ifstream in(script_path);
  if (!in) {
    std::fprintf(stderr, "cannot read %s\n", script_path);
    return 2;
  }
  std::string line;
  while (std::getline(in, line)) {
    if (!line.empty() && line.back() == '\r') {
      line.pop_back();
    }
    auto tok = split(line);
    if (tok.empty() || tok[0][0] == '#') {
      continue;
    }
    const std::string &cmd = tok[0];
    if (cmd == "print" || cmd == "hash" || cmd == "input" || cmd == "obj" || cmd == "upd") {
      ReactionType value;
      if (tok.size() != 2 || !make(tok[1], value)) {
        std::printf("reject %s argc=%zu\n", cmd.c_str(), tok.size());
        continue;
      }
      if (cmd == "print") {
        std::printf("print %s str=%s empty=%d custom=%d paid=%d %s\n", tok[1].c_str(),
                    to_hex(value.get_string()).c_str(), value.is_empty() ? 1 : 0, value.is_custom_reaction() ? 1 : 0,
                    value.is_paid_reaction() ? 1 : 0, printed(value).c_str());
      } else if (cmd == "hash") {
        std::printf("hash %s %llu\n", tok[1].c_str(), static_cast<unsigned long long>(value.get_hash()));
      } else if (cmd == "input") {
        std::printf("input %s %s\n", tok[1].c_str(), input_line(value).c_str());
      } else if (cmd == "obj") {
        std::printf("obj %s %s\n", tok[1].c_str(), object_line(value).c_str());
      } else {
        auto update = value.get_update_default_reaction_type();
        if (update == nullptr) {
          std::printf("upd %s null\n", tok[1].c_str());
        } else {
          std::printf("upd %s %s\n", tok[1].c_str(),
                      update->reaction_type_ == nullptr
                          ? "null-reaction"
                          : td_reaction_name(update->reaction_type_->get_id()));
        }
      }
    } else if (cmd == "cmp") {
      ReactionType a;
      ReactionType b;
      if (tok.size() != 3 || !make(tok[1], a) || !make(tok[2], b)) {
        std::printf("reject cmp argc=%zu\n", tok.size());
        continue;
      }
      std::printf("cmp %s %s lt=%d eq=%d ne=%d\n", tok[1].c_str(), tok[2].c_str(), a < b ? 1 : 0, a == b ? 1 : 0,
                  a != b ? 1 : 0);
    } else if (cmd == "vec") {
      if (tok.size() < 2 || (tok[1] != "0" && tok[1] != "1") || tok.size() < 3) {
        std::printf("reject vec argc=%zu\n", tok.size());
        continue;
      }
      std::vector<ReactionType> types;
      bool ok = true;
      for (size_t i = 2; i < tok.size(); i++) {
        ReactionType value;
        if (!make(tok[i], value)) {
          ok = false;
          break;
        }
        types.push_back(std::move(value));
      }
      if (!ok) {
        std::printf("reject vec spec\n");
        continue;
      }
      std::string args_text;
      for (size_t i = 1; i < tok.size(); i++) {
        if (i > 1) {
          args_text += ' ';
        }
        args_text += tok[i];
      }
      std::printf("vec %s %s\n", args_text.c_str(), vector_object_line(types, tok[1] == "1").c_str());
    } else if (cmd == "vechash") {
      if (tok.size() < 2) {
        std::printf("reject vechash argc=%zu\n", tok.size());
        continue;
      }
      std::vector<ReactionType> types;
      bool ok = true;
      for (size_t i = 1; i < tok.size(); i++) {
        ReactionType value;
        if (!make(tok[i], value)) {
          ok = false;
          break;
        }
        types.push_back(std::move(value));
      }
      if (!ok) {
        std::printf("reject vechash spec\n");
        continue;
      }
      std::string args_text;
      for (size_t i = 1; i < tok.size(); i++) {
        if (i > 1) {
          args_text += ' ';
        }
        args_text += tok[i];
      }
      std::printf("vechash %s %lld\n", args_text.c_str(),
                  static_cast<long long>(get_reaction_types_hash(types)));
    } else {
      std::printf("reject unknown=%s\n", cmd.c_str());
    }
  }
  return 0;
}

}  // namespace

int main(int argc, char **argv) {
  if (argc == 3 && std::string(argv[1]) == "--check") {
    return check_case(argv[2]);
  }
  if (argc != 2) {
    std::fprintf(stderr, "usage: reaction_type_probe <script.txt> | --check <case>\n");
    return 2;
  }
  return run_script(argv[1]);
}