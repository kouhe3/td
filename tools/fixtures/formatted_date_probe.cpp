// Oracle for td::FormattedDate (td/telegram/FormattedDate.{h,cpp,hpp}) driven by
// a script. No Td, no actor, no clock: the whole surface is flags arithmetic.
//
//   formatted_date_probe <script.txt>
//
// Commands (one per line; blank lines and '#' comments ignored):
//   fmt <date> <format>              get_formatted_date(date, string date_format)
//                                    `-` means the empty format string
//   api <date> <spec>                get_formatted_date(date, td_api type)
//                                    spec = none | rel | abs:<time>:<date>:<dow>
//                                           time/date are n|s|l, dow is 0|1
//   ctr <date> <rel> <st> <lt> <sd> <ld> <dow>    the seven-bool constructor
//   typeobj <date> <flags>           get_date_time_formatting_type_object
//   entity <date> <flags> <off> <len>  get_input_message_entity_formatted_date
//   textdate <date> <flags>          get_input_text_date (text is passed as null)
//   eq <date_a> <flags_a> <date_b> <flags_b>
//   print <date> <flags>             operator<<
//
// Measured behaviour this probe encodes (docs/facts.md F282/F283):
//   * get_date_flags("dD") ORs ShortDate and LongDate (24); the bool constructor's
//     `short_date ? ShortDate : long_date ? LongDate` keeps only 8;
//   * "r"/"R" are Relative only for the whole string -- "rt" is an invalid format,
//     because 'r' is not a per-character flag;
//   * get_date_flags rejects anything outside rRtTdDwW with
//     `Invalid date format used`, and date <= 0 is `Invalid date specified`;
//   * a Relative constructor drops every other flag; get_date_time_formatting_type_object
//     returns nullptr for flags == 0;
//   * messageEntityFormattedDate and textDate take the six flag booleans in the
//     order relative, short_time, long_time, short_date, long_date, day_of_week.
//
// Build from the repo root against a configured C:/tmp/td-build:
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I tdactor -I tdnet -I C:/tmp/td-build/tdutils tools/fixtures/formatted_date_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tdnet/libtdnet.a
//   C:/tmp/td-build/tdtl/libtdtl.a C:/tmp/td-build/tde2e/libtde2e.a
//   C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi -lcrypt32 -lsecur32
//   -lmswsock -liphlpapi -ldnsapi -o C:/tmp/td-probe/formatted_date_probe.exe

#include "td/telegram/FormattedDate.h"

#include "td/telegram/td_api.h"
#include "td/telegram/telegram_api.h"

#include "td/utils/StringBuilder.h"

#include <cerrno>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <string>
#include <vector>

namespace {

using td::FormattedDate;

bool parse_i32(const std::string &s, int32_t &out) {
  if (s.empty()) {
    return false;
  }
  errno = 0;
  char *end = nullptr;
  long long v = std::strtoll(s.c_str(), &end, 10);
  if (errno != 0 || end != s.c_str() + s.size()) {
    return false;
  }
  out = static_cast<int32_t>(v);
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

std::string printed(const FormattedDate &date) {
  td::StringBuilder sb;
  sb << date;
  return sb.as_cslice().str();
}

const char *precision_name(const td::td_api::object_ptr<td::td_api::DateTimePartPrecision> &precision) {
  if (precision == nullptr) {
    return "null";
  }
  switch (precision->get_id()) {
    case td::td_api::dateTimePartPrecisionNone::ID:
      return "none";
    case td::td_api::dateTimePartPrecisionShort::ID:
      return "short";
    case td::td_api::dateTimePartPrecisionLong::ID:
      return "long";
    default:
      return "unknown";
  }
}

td::td_api::object_ptr<td::td_api::DateTimePartPrecision> make_precision(char c) {
  switch (c) {
    case 'n':
      return td::td_api::make_object<td::td_api::dateTimePartPrecisionNone>();
    case 's':
      return td::td_api::make_object<td::td_api::dateTimePartPrecisionShort>();
    case 'l':
      return td::td_api::make_object<td::td_api::dateTimePartPrecisionLong>();
    default:
      return nullptr;
  }
}

/// `none` | `rel` | `abs:<time>:<date>:<dow>`; returns false for a malformed spec.
bool make_type(const std::string &spec, td::td_api::object_ptr<td::td_api::DateTimeFormattingType> &out) {
  if (spec == "none") {
    out = nullptr;
    return true;
  }
  if (spec == "rel") {
    out = td::td_api::make_object<td::td_api::dateTimeFormattingTypeRelative>();
    return true;
  }
  // abs:<time>:<date>:<dow> is exactly 9 characters.
  if (spec.size() != 9 || spec.compare(0, 4, "abs:") != 0 || spec[5] != ':' || spec[7] != ':' ||
      (spec[8] != '0' && spec[8] != '1')) {
    return false;
  }
  auto time = make_precision(spec[4]);
  auto date = make_precision(spec[6]);
  if (time == nullptr || date == nullptr) {
    return false;
  }
  out = td::td_api::make_object<td::td_api::dateTimeFormattingTypeAbsolute>(std::move(time), std::move(date),
                                                                            spec[8] == '1');
  return true;
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
    if (cmd == "fmt") {
      int32_t date;
      if (tok.size() != 3 || !parse_i32(tok[1], date)) {
        std::printf("reject fmt argc=%zu\n", tok.size());
        continue;
      }
      const std::string format = tok[2] == "-" ? std::string() : tok[2];
      auto result = FormattedDate::get_formatted_date(date, format);
      if (result.is_error()) {
        std::printf("fmt %d %s err=%s\n", date, tok[2].c_str(), result.error().message().str().c_str());
        continue;
      }
      auto value = result.move_as_ok();
      std::printf("fmt %d %s valid=%d %s\n", date, tok[2].c_str(), value.is_valid() ? 1 : 0, printed(value).c_str());
    } else if (cmd == "api") {
      int32_t date;
      if (tok.size() != 3 || !parse_i32(tok[1], date)) {
        std::printf("reject api argc=%zu\n", tok.size());
        continue;
      }
      td::td_api::object_ptr<td::td_api::DateTimeFormattingType> type;
      if (!make_type(tok[2], type)) {
        std::printf("reject api spec=%s\n", tok[2].c_str());
        continue;
      }
      auto result = FormattedDate::get_formatted_date(date, type);
      if (result.is_error()) {
        std::printf("api %d %s err=%s\n", date, tok[2].c_str(), result.error().message().str().c_str());
        continue;
      }
      auto value = result.move_as_ok();
      std::printf("api %d %s %s\n", date, tok[2].c_str(), printed(value).c_str());
    } else if (cmd == "ctr") {
      int32_t date;
      int32_t flags[6];
      if (tok.size() != 8 || !parse_i32(tok[1], date) || !parse_i32(tok[2], flags[0]) ||
          !parse_i32(tok[3], flags[1]) || !parse_i32(tok[4], flags[2]) || !parse_i32(tok[5], flags[3]) ||
          !parse_i32(tok[6], flags[4]) || !parse_i32(tok[7], flags[5])) {
        std::printf("reject ctr argc=%zu\n", tok.size());
        continue;
      }
      FormattedDate value(date, flags[0] != 0, flags[1] != 0, flags[2] != 0, flags[3] != 0, flags[4] != 0,
                          flags[5] != 0);
      std::printf("ctr %d %d%d%d%d%d%d valid=%d %s\n", date, flags[0], flags[1], flags[2], flags[3], flags[4],
                  flags[5], value.is_valid() ? 1 : 0, printed(value).c_str());
    } else if (cmd == "typeobj") {
      int32_t date;
      int32_t flags;
      if (tok.size() != 3 || !parse_i32(tok[1], date) || !parse_i32(tok[2], flags)) {
        std::printf("reject typeobj argc=%zu\n", tok.size());
        continue;
      }
      FormattedDate value(date, flags);
      auto object = value.get_date_time_formatting_type_object();
      if (object == nullptr) {
        std::printf("typeobj %d %d null\n", date, flags);
      } else if (object->get_id() == td::td_api::dateTimeFormattingTypeRelative::ID) {
        std::printf("typeobj %d %d rel\n", date, flags);
      } else {
        auto absolute = static_cast<const td::td_api::dateTimeFormattingTypeAbsolute *>(object.get());
        std::printf("typeobj %d %d abs:%s:%s:%d\n", date, flags, precision_name(absolute->time_precision_),
                    precision_name(absolute->date_precision_), absolute->show_day_of_week_ ? 1 : 0);
      }
    } else if (cmd == "entity") {
      int32_t date;
      int32_t flags;
      int32_t offset;
      int32_t length;
      if (tok.size() != 5 || !parse_i32(tok[1], date) || !parse_i32(tok[2], flags) || !parse_i32(tok[3], offset) ||
          !parse_i32(tok[4], length)) {
        std::printf("reject entity argc=%zu\n", tok.size());
        continue;
      }
      FormattedDate value(date, flags);
      auto object = value.get_input_message_entity_formatted_date(offset, length);
      std::printf("entity %d %d %d %d rel=%d st=%d lt=%d sd=%d ld=%d dow=%d off=%d len=%d date=%d\n", date, flags,
                  offset, length, object->relative_ ? 1 : 0, object->short_time_ ? 1 : 0,
                  object->long_time_ ? 1 : 0, object->short_date_ ? 1 : 0, object->long_date_ ? 1 : 0,
                  object->day_of_week_ ? 1 : 0, object->offset_, object->length_, object->date_);
    } else if (cmd == "textdate") {
      int32_t date;
      int32_t flags;
      if (tok.size() != 3 || !parse_i32(tok[1], date) || !parse_i32(tok[2], flags)) {
        std::printf("reject textdate argc=%zu\n", tok.size());
        continue;
      }
      FormattedDate value(date, flags);
      td::telegram_api::object_ptr<td::telegram_api::RichText> text;
      auto object = value.get_input_text_date(std::move(text));
      std::printf("textdate %d %d rel=%d st=%d lt=%d sd=%d ld=%d dow=%d text=%d date=%d\n", date, flags,
                  object->relative_ ? 1 : 0, object->short_time_ ? 1 : 0, object->long_time_ ? 1 : 0,
                  object->short_date_ ? 1 : 0, object->long_date_ ? 1 : 0, object->day_of_week_ ? 1 : 0,
                  object->text_ == nullptr ? 0 : 1, object->date_);
    } else if (cmd == "eq") {
      int32_t date_a;
      int32_t flags_a;
      int32_t date_b;
      int32_t flags_b;
      if (tok.size() != 5 || !parse_i32(tok[1], date_a) || !parse_i32(tok[2], flags_a) ||
          !parse_i32(tok[3], date_b) || !parse_i32(tok[4], flags_b)) {
        std::printf("reject eq argc=%zu\n", tok.size());
        continue;
      }
      FormattedDate a(date_a, flags_a);
      FormattedDate b(date_b, flags_b);
      std::printf("eq %d %d %d %d eq=%d ne=%d\n", date_a, flags_a, date_b, flags_b, a == b ? 1 : 0, a != b ? 1 : 0);
    } else if (cmd == "print") {
      int32_t date;
      int32_t flags;
      if (tok.size() != 3 || !parse_i32(tok[1], date) || !parse_i32(tok[2], flags)) {
        std::printf("reject print argc=%zu\n", tok.size());
        continue;
      }
      FormattedDate value(date, flags);
      std::printf("print %d %d %s\n", date, flags, printed(value).c_str());
    } else {
      std::printf("reject unknown=%s\n", cmd.c_str());
    }
  }
  return 0;
}

}  // namespace

int main(int argc, char **argv) {
  if (argc != 2) {
    std::fprintf(stderr, "usage: formatted_date_probe <script.txt>\n");
    return 2;
  }
  return run_script(argv[1]);
}