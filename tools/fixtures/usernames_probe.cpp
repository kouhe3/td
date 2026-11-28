// Oracle for td::Usernames (td/telegram/Usernames.{h,cpp}) driven by a script.
//
//   usernames_probe <script.txt>
//   usernames_probe --check <case>      one fixed case per process, only to see
//                                       whether the implementation aborts
//
// `Usernames` is the pure value type behind the `td_api::usernames` object. It
// takes a `first_username` plus a list of `telegram_api::username` records and
// keeps two ordered arrays (`active`, `disabled`) plus the position of the
// editable username and three side tables (other editable / deletable /
// expired). Nothing here needs `Td`, an actor or a scheduler: the probe builds
// the records itself and drives the real implementation.
//
// Script lines (blank lines and '#' comments ignored):
//   new <first|-> <name:act:edit:del:exp,name:...,...|->   reset the object
//   print                     `state=<operator<< output>`
//   printhex                  `statehex=<hex of the operator<< bytes>`
//   acc                       empty / has first / first / has editable / editable
//   canadd <name>             can_add_secondary
//   candel <name>             can_delete_secondary
//   cantoggle <for_bot> <name>
//   canreorder <csv|->        can_reorder_to
//   change <name|->           change_editable_username (then `state`)
//   toggle <for_bot> <name> <0|1>
//   addsec <name> | delsec <name> | deact | reorder <csv|->
//
// A `<name>` is either plain text or `hex-<hex bytes>`; the hex form exists so
// the corpus can feed invalid UTF-8 and watch `check_validness` reset the whole
// object (`printhex` then shows the bytes).
//
// Build (td/telegram/Usernames.cpp lives in libtdcore.a; td_api/telegram_api
// generated classes are header-only for what we use, but `Usernames.cpp` also
// references `td_api::usernames`' generated store, so libtdapi.a joins the
// group):
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I tdutils -I tdactor
//   -I C:/tmp/td-build/tdutils tools/fixtures/usernames_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/libtdutils.a C:/tmp/td-build/libtdactor.a -Wl,--end-group
//   -lssl -lcrypto -lz -lws2_32 -lpsapi -o C:/tmp/td-probe/usernames_probe.exe
//
// Measured behaviour this probe encodes (docs/facts.md F205): the constructor
// filters empty and duplicate names first, `deletable` clears `editable`, only
// the first editable survives (the rest land in `other_editable`), and a name
// with `active=0` goes to `disabled` with `editable_username_pos_` indexed into
// that array.

#include "td/telegram/Usernames.h"

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

// "<name>" or "hex:<hex bytes>"; "-" is the empty string.
string parse_name(Slice token) {
  if (token == "-") {
    return string();
  }
  if (token.size() > 4 && token.substr(0, 4) == "hex-") {
    auto hex = token.substr(4);
    string result;
    if (hex.size() % 2 != 0) {
      std::fprintf(stderr, "bad hex\n");
      std::exit(2);
    }
    for (size_t i = 0; i < hex.size(); i += 2) {
      auto hi = hex_digit(hex[i]);
      auto lo = hex_digit(hex[i + 1]);
      if (hi < 0 || lo < 0) {
        std::fprintf(stderr, "bad hex digit\n");
        std::exit(2);
      }
      result.push_back(static_cast<char>((hi << 4) | lo));
    }
    return result;
  }
  return token.str();
}

bool parse_flag(Slice token) {
  return token == "1";
}

vector<string> parse_csv(Slice token) {
  vector<string> result;
  if (token == "-") {
    return result;
  }
  for (auto part : td::full_split(token, ',')) {
    result.push_back(parse_name(part));
  }
  return result;
}

vector<td::telegram_api::object_ptr<td::telegram_api::username>> parse_items(Slice token) {
  vector<td::telegram_api::object_ptr<td::telegram_api::username>> result;
  if (token == "-") {
    return result;
  }
  for (auto item : td::full_split(token, ',')) {
    auto parts = td::full_split(item, ':');
    if (parts.size() != 5) {
      std::fprintf(stderr, "bad item\n");
      std::exit(2);
    }
    auto name = parse_name(parts[0]);
    result.push_back(td::telegram_api::make_object<td::telegram_api::username>(
        0, parse_flag(parts[2]), parse_flag(parts[1]), parse_flag(parts[3]), parse_flag(parts[4]), name));
  }
  return result;
}

string render(const td::Usernames &usernames) {
  td::StringBuilder sb;
  sb << usernames;
  return sb.as_cslice().str();
}

void print_state(const td::Usernames &usernames, bool hex) {
  auto rendered = render(usernames);
  if (hex) {
    std::printf("statehex=%s\n", td::hex_encode(rendered).c_str());
  } else {
    std::printf("state=%s\n", rendered.c_str());
  }
}

void print_accessors(const td::Usernames &usernames) {
  auto first = usernames.get_first_username();
  auto editable = usernames.get_editable_username();
  std::printf("acc empty=%d hasfirst=%d first=%s haseditable=%d editable=%s\n",
              static_cast<int>(usernames.is_empty()), static_cast<int>(usernames.has_first_username()),
              first.empty() ? "-" : td::hex_encode(first).c_str(), static_cast<int>(usernames.has_editable_username()),
              editable.empty() ? "-" : td::hex_encode(editable).c_str());
}

}  // namespace

int main(int argc, char **argv) {
  if (argc == 3 && std::strcmp(argv[1], "--check") == 0) {
    Slice check_case(argv[2]);
    if (check_case == "reorder_missing") {
      td::Usernames usernames(string(), parse_items(Slice("a:1:1:0:0,b:1:0:0:0")));
      std::printf("before=%s\n", render(usernames).c_str());
      std::fflush(stdout);
      // The editable username disappears from the new order: can_reorder_to()
      // rejects it, a direct reorder_to() hits CHECK(has_editable_username()).
      auto result = usernames.reorder_to({"b"});
      std::printf("after=%s\n", render(result).c_str());
      return 0;
    }
    std::fprintf(stderr, "unknown check case\n");
    return 2;
  }

  if (argc != 2) {
    std::fprintf(stderr, "usage: usernames_probe <script.txt>\n");
    return 2;
  }

  auto script = read_all(argv[1]);
  td::Usernames current;
  bool hex_mode = false;
  for (auto raw_line : td::full_split(Slice(script), '\n')) {
    auto line = trim_line(raw_line);
    if (line.empty() || line[0] == '#') {
      continue;
    }
    auto tokens = split_tokens(line);
    auto command = tokens[0];
    if (command == "new") {
      current = td::Usernames(parse_name(tokens[1]), parse_items(tokens[2]));
      print_state(current, hex_mode);
    } else if (command == "print") {
      print_state(current, false);
    } else if (command == "printhex") {
      print_state(current, true);
    } else if (command == "hexmode") {
      hex_mode = parse_flag(tokens[1]);
    } else if (command == "acc") {
      print_accessors(current);
    } else if (command == "canadd") {
      std::printf("canadd=%d\n", static_cast<int>(current.can_add_secondary(parse_name(tokens[1]))));
    } else if (command == "candel") {
      std::printf("candel=%d\n", static_cast<int>(current.can_delete_secondary(parse_name(tokens[1]))));
    } else if (command == "cantoggle") {
      std::printf("cantoggle=%d\n",
                  static_cast<int>(current.can_toggle(parse_flag(tokens[1]), parse_name(tokens[2]))));
    } else if (command == "canreorder") {
      std::printf("canreorder=%d\n", static_cast<int>(current.can_reorder_to(parse_csv(tokens[1]))));
    } else if (command == "change") {
      current = current.change_editable_username(parse_name(tokens[1]));
      print_state(current, hex_mode);
    } else if (command == "toggle") {
      bool for_bot = parse_flag(tokens[1]);
      auto name = parse_name(tokens[2]);
      bool is_active = parse_flag(tokens[3]);
      if (!current.can_toggle(for_bot, name)) {
        std::printf("toggle=skip\n");
        continue;
      }
      current = current.toggle(for_bot, name, is_active);
      print_state(current, hex_mode);
    } else if (command == "addsec") {
      current = current.add_secondary(parse_name(tokens[1]));
      print_state(current, hex_mode);
    } else if (command == "delsec") {
      current = current.delete_secondary(parse_name(tokens[1]));
      print_state(current, hex_mode);
    } else if (command == "deact") {
      current = current.deactivate_all();
      print_state(current, hex_mode);
    } else if (command == "reorder") {
      current = current.reorder_to(parse_csv(tokens[1]));
      print_state(current, hex_mode);
    } else {
      std::fprintf(stderr, "unknown command %s\n", command.str().c_str());
      return 2;
    }
  }
  return 0;
}