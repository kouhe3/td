// Oracle for td/telegram/net/DcId: the value-level surface around
// is_valid / create / from_value plus the derived predicates, comparisons and
// the stream printer.
//
//   dc_id_probe <script.txt>
//
// Script lines (`#` starts a comment):
//   is_valid <int32>              -> `is_valid in=… static=0|1`
//   create <int32>                -> `create in=… <dump>`
//   from_value <int32>            -> `from_value in=… <dump>`
//   internal <int32>              -> `internal in=… <dump>`   (only 1..1000)
//   external <int32>              -> `external in=… <dump>`   (only 1..1000)
//   cmp <a> <b>                   -> `cmp a=… b=… eq=… lt=… gt=…`  (from_value)
//   cmp_external <a> <b>          -> same, both built with external()
//   cmp_mixed <a> <b>             -> a = external(a), b = from_value(b)
//   raw <int32>                   -> `raw in=… id=…`  (from_value, exact only)
//
// <dump> = `value=… empty=… main=… exact=… internal=… external=… eq_invalid=… eq_empty=… str=…`
// where str is `operator<<`, including its branch order
// (invalid -> empty -> is_empty -> main -> raw/external).
//
// internal() / external() / get_raw_id() CHECK on their invalid inputs; that is
// a process abort, not a value, so the script only feeds them valid ids.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/dc_id_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group \
//       -lssl -lcrypto -lws2_32 -lpsapi -o dc_id_probe.exe

#include "td/telegram/net/DcId.h"

#include <cstdio>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>

namespace {

std::string render(td::DcId dc_id) {
  td::StringBuilder sb;
  sb << dc_id;
  return sb.as_cslice().str();
}

void dump(const std::string &command, int value, td::DcId dc_id) {
  std::printf(
      "%s in=%d value=%d empty=%d main=%d exact=%d internal=%d external=%d eq_invalid=%d eq_empty=%d str=%s\n",
      command.c_str(), value, static_cast<int>(dc_id.get_value()), static_cast<int>(dc_id.is_empty()),
      static_cast<int>(dc_id.is_main()), static_cast<int>(dc_id.is_exact()), static_cast<int>(dc_id.is_internal()),
      static_cast<int>(dc_id.is_external()), static_cast<int>(dc_id == td::DcId::invalid()),
      static_cast<int>(dc_id == td::DcId()), render(dc_id).c_str());
}

}  // namespace

int main(int argc, char **argv) {
  if (argc != 2) {
    std::fprintf(stderr, "usage: dc_id_probe <script.txt>\n");
    return 2;
  }
  std::ifstream input(argv[1], std::ios::binary);
  if (!input) {
    std::fprintf(stderr, "cannot read %s\n", argv[1]);
    return 2;
  }
  std::string text((std::istreambuf_iterator<char>(input)), std::istreambuf_iterator<char>());
  std::istringstream lines(text);
  std::string line;
  while (std::getline(lines, line)) {
    std::istringstream tokens(line);
    std::string command;
    if (!(tokens >> command) || command[0] == '#') {
      continue;
    }
    int a = 0;
    int b = 0;
    if (command == "is_valid") {
      tokens >> a;
      std::printf("is_valid in=%d static=%d\n", a, static_cast<int>(td::DcId::is_valid(a)));
    } else if (command == "create") {
      tokens >> a;
      dump(command, a, td::DcId::create(a));
    } else if (command == "from_value") {
      tokens >> a;
      dump(command, a, td::DcId::from_value(a));
    } else if (command == "internal") {
      tokens >> a;
      dump(command, a, td::DcId::internal(a));
    } else if (command == "external") {
      tokens >> a;
      dump(command, a, td::DcId::external(a));
    } else if (command == "cmp" || command == "cmp_external" || command == "cmp_mixed") {
      tokens >> a >> b;
      td::DcId left;
      td::DcId right;
      if (command == "cmp") {
        left = td::DcId::from_value(a);
        right = td::DcId::from_value(b);
      } else if (command == "cmp_external") {
        left = td::DcId::external(a);
        right = td::DcId::external(b);
      } else {
        left = td::DcId::external(a);
        right = td::DcId::from_value(b);
      }
      std::printf("%s a=%d b=%d eq=%d lt=%d gt=%d\n", command.c_str(), a, b, static_cast<int>(left == right),
                  static_cast<int>(left < right), static_cast<int>(right < left));
    } else if (command == "raw") {
      tokens >> a;
      std::printf("raw in=%d id=%d\n", a, static_cast<int>(td::DcId::from_value(a).get_raw_id()));
    } else {
      std::printf("unknown %s\n", command.c_str());
    }
  }
  return 0;
}