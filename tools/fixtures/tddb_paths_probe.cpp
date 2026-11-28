// Oracle for td::TdDb's path helpers: `as_db_key` (TdDb.cpp:671-676) and
// `check_parameters` (TdDb.cpp:621-669).
//
//   tddb_paths_probe <script.txt>
//
// Both are reached through the real `td::TdDb` definitions, i.e. this is the
// actual TDLib code and not a transcription of it:
//   * `as_db_key` is public (TdDb.h:154);
//   * `check_parameters` is *private* (TdDb.h:192), so the probe widens access
//     with `#define private public` around the TdDb.h include. TdDb.h's own
//     includes are pre-included first and its include guard is therefore already
//     set, so the macro can only affect TdDb.h's access specifiers; the function
//     body that runs is td's compiled one, unmodified.
//
// `check_parameters` is a pure filesystem+string function (mkpath, realpath,
// TD_DIR_SLASH, two `if` implication steps). It touches no actor, no Scheduler
// and no logging interface, so this probe never initialises TDLib. It does
// create directories on disk -- mkpath is the point.
//
// Script lines (blank lines and '#' comments ignored, fields separated by one
// TAB; paths must not contain a TAB or a newline):
//
//   key <hex|->                 feed the bytes to TdDb::as_db_key
//   checkparams <db_dir> <files_dir> <is_test_dc> <use_file> <use_chat> <use_msg>
//                               fill TdDb::Parameters, call check_parameters,
//                               then print every field it can have changed
//
// Narration, one line per command:
//   key in=<hex|-> kempty=0 kraw=1 kpwd=0 dlen=8 data=637563756d626572
//   checkparams db_in=[…] files_in=[…] testdc=0 file=1 chat=1 msg=1 -> ok=1 code=0 \
//       db=[C:\…\] files=[C:\…\] testdc=0 file=1 chat=1 msg=1 msg=[OK]
//   checkparams … -> ok=0 code=400 db=[…] files=[…] … msg=[[Error : 400 : Can't init …]]
//
// `DbKey` has exactly three observable facts (DbKey.h): which of
// empty/raw_key/password it is and its `data()` bytes; the 32-byte AES key is
// derived later, by BinlogKeyValue/AesCtrEncryption, and is not reachable from
// a DbKey. `kempty` is therefore 0 for every `as_db_key` result — the empty
// *input* still becomes the raw key "cucumber".
//
// Paths come out of `realpath` absolute and with the build's TD_DIR_SLASH
// ('\\' on Windows), so both sides of the lane must run with the same cwd and be
// compared literally.
//
// Three measured facts this probe encodes:
//   * mkpath splits on TD_DIR_SLASH only, so a corpus path written with '/'
//     separators produces exactly *one* CreateDirectory call (for the whole
//     path, minus the trailing separator `prepare_dir` appends). Nested
//     creation therefore only happens for '\\' paths; a '/' path whose parent
//     is missing fails with ERROR_PATH_NOT_FOUND (3) even though Windows itself
//     accepts mixed separators.
//   * `mkdir` treats ERROR_ALREADY_EXISTS as success, and Windows rejects
//     CreateDirectory on a path that is an existing *regular file* with that
//     same code, so a file passes as a "directory": check_parameters accepts
//     `<…>/marker.txt` and returns it with a trailing separator.
//   * `realpath` on Windows is GetFullPathNameW, i.e. lexical: it collapses
//     `.`/`..`, folds '/' to '\\' and never touches the filesystem or resolves
//     symlinks. A single trailing separator is *preserved* (`C:\a\` -> `C:\a\`,
//     `C:\a` -> `C:\a`) and a doubled one is collapsed, so the two
//     `if (res.back() != TD_DIR_SLASH) res += TD_DIR_SLASH;` appends in td are
//     no-ops here -- they are ported anyway, since only the measurement says so.
//
// Build from the repo root against a configured C:/tmp/td-build. `check_parameters`
// lives in td/telegram/TdDb.cpp, which is compiled into libtdcore.a, so the link
// set is larger than the other lanes' (-lz for crc32, -lcrypt32 for the
// certificate store, -lmswsock for AcceptEx, all pulled in by tdcore/tdnet):
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I tdutils -I tdactor -I tddb
//   -I C:/tmp/td-build/tdutils tools/fixtures/tddb_paths_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/tdnet/libtdnet.a C:/tmp/td-build/tddb/libtddb.a
//   C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/tde2e/libtde2e.a
//   C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tdutils/libtdutils.a
//   C:/tmp/td-build/sqlite/libtdsqlite.a -Wl,--end-group
//   -lssl -lcrypto -lz -lcrypt32 -lmswsock -lws2_32 -lpsapi
//   -o C:/tmp/td-probe/tddb_paths_probe.exe

#include "td/db/binlog/BinlogEvent.h"
#include "td/db/binlog/BinlogInterface.h"
#include "td/db/DbKey.h"
#include "td/db/KeyValueSyncInterface.h"
#include "td/utils/common.h"
#include "td/utils/Promise.h"
#include "td/utils/Slice.h"
#include "td/utils/Status.h"

#include <functional>
#include <memory>

#define private public
#include "td/telegram/TdDb.h"
#undef private

#include <cstdio>
#include <fstream>
#include <string>
#include <vector>

namespace {

td::string hex_encode(td::Slice data) {
  static const char *digits = "0123456789abcdef";
  td::string res;
  res.reserve(data.size() * 2);
  for (auto c : data) {
    res.push_back(digits[(static_cast<unsigned char>(c) >> 4) & 15]);
    res.push_back(digits[static_cast<unsigned char>(c) & 15]);
  }
  return res;
}

int hex_digit(char c) {
  if (c >= '0' && c <= '9') {
    return c - '0';
  }
  if (c >= 'a' && c <= 'f') {
    return c - 'a' + 10;
  }
  if (c >= 'A' && c <= 'F') {
    return c - 'A' + 10;
  }
  return -1;
}

bool hex_decode(td::Slice text, td::string &res) {
  res.clear();
  if (text.size() % 2 != 0) {
    return false;
  }
  res.reserve(text.size() / 2);
  for (size_t i = 0; i < text.size(); i += 2) {
    int hi = hex_digit(text[i]);
    int lo = hex_digit(text[i + 1]);
    if (hi < 0 || lo < 0) {
      return false;
    }
    res.push_back(static_cast<char>((hi << 4) | lo));
  }
  return true;
}

bool token(const td::string &token, bool &res) {
  if (token == "0") {
    res = false;
    return true;
  }
  if (token == "1") {
    res = true;
    return true;
  }
  return false;
}

std::vector<td::string> split(const td::string &line, char sep) {
  std::vector<td::string> res;
  td::string current;
  for (auto c : line) {
    if (c == sep) {
      res.push_back(current);
      current.clear();
    } else {
      current.push_back(c);
    }
  }
  res.push_back(current);
  return res;
}

int fail(int line_no, const td::string &message) {
  std::fprintf(stderr, "tddb_paths_probe: %s in line %d\n", message.c_str(), line_no);
  return 1;
}

void print_key(td::Slice in_hex, const td::DbKey &key) {
  auto data = key.data();
  std::printf("key in=%s kempty=%d kraw=%d kpwd=%d dlen=%zu data=%s\n", in_hex.str().c_str(), key.is_empty() ? 1 : 0,
              key.is_raw_key() ? 1 : 0, key.is_password() ? 1 : 0, data.size(),
              data.empty() ? "-" : hex_encode(data).c_str());
}

void print_parameters(const td::TdDb::Parameters &parameters, td::Slice db_in, td::Slice files_in, bool in_test_dc,
                      bool in_file, bool in_chat, bool in_msg, const td::Status &status) {
  std::printf(
      "checkparams db_in=[%s] files_in=[%s] testdc=%d file=%d chat=%d msg=%d -> ok=%d code=%d db=[%s] files=[%s] "
      "testdc=%d file=%d chat=%d msg=%d msg=[%s]\n",
      db_in.str().c_str(), files_in.str().c_str(), in_test_dc ? 1 : 0, in_file ? 1 : 0, in_chat ? 1 : 0,
      in_msg ? 1 : 0, status.is_ok() ? 1 : 0, status.code(), parameters.database_directory_.c_str(),
      parameters.files_directory_.c_str(), parameters.is_test_dc_ ? 1 : 0, parameters.use_file_database_ ? 1 : 0,
      parameters.use_chat_info_database_ ? 1 : 0, parameters.use_message_database_ ? 1 : 0,
      status.message().str().c_str());
}

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc != 2) {
    std::fprintf(stderr, "Usage: tddb_paths_probe <script.txt>\n");
    return 2;
  }

  std::ifstream in(argv[1], std::ios::binary);
  if (!in) {
    std::fprintf(stderr, "tddb_paths_probe: cannot read %s\n", argv[1]);
    return 2;
  }

  td::string line;
  int line_no = 0;
  while (std::getline(in, line)) {
    line_no++;
    while (!line.empty() && (line.back() == '\r' || line.back() == '\n')) {
      line.pop_back();
    }
    if (line.empty() || line[0] == '#') {
      continue;
    }

    auto space = line.find_first_of(" \t");
    if (space == td::string::npos) {
      return fail(line_no, "missing arguments");
    }
    auto command = line.substr(0, space);
    auto rest = line.substr(space + 1);

    if (command == "key") {
      td::string raw;
      if (rest != "-" && !hex_decode(rest, raw)) {
        return fail(line_no, "bad hex in key");
      }
      auto in_hex = raw.empty() ? td::string("-") : hex_encode(raw);
      print_key(in_hex, td::TdDb::as_db_key(std::move(raw)));
      continue;
    }

    if (command == "checkparams") {
      auto fields = split(rest, '\t');
      if (fields.size() != 6) {
        return fail(line_no, "checkparams needs 6 tab-separated fields");
      }
      td::TdDb::Parameters parameters;
      parameters.database_directory_ = fields[0];
      parameters.files_directory_ = fields[1];
      bool in_test_dc = false;
      bool in_file = false;
      bool in_chat = false;
      bool in_msg = false;
      if (!token(fields[2], in_test_dc) || !token(fields[3], in_file) || !token(fields[4], in_chat) ||
          !token(fields[5], in_msg)) {
        return fail(line_no, "flags must be 0 or 1");
      }
      parameters.is_test_dc_ = in_test_dc;
      parameters.use_file_database_ = in_file;
      parameters.use_chat_info_database_ = in_chat;
      parameters.use_message_database_ = in_msg;

      auto status = td::TdDb::check_parameters(parameters);
      print_parameters(parameters, fields[0], fields[1], in_test_dc, in_file, in_chat, in_msg, status);
      continue;
    }

    return fail(line_no, "unknown command");
  }

  return 0;
}