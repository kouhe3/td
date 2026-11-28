// Oracle for td::SqliteKeyValue (tddb/td/db/SqliteKeyValue.h) on top of this
// repository's own SQLCipher 3.31 build, driven by a script.
//
//   sqlite_kv_probe <script.txt> <db_path>
//   sqlite_kv_probe --check <db_path> <case>
//
// The KV is opened with `DbKey::empty()` (plaintext, no PRAGMA key) and the
// table is always "kv". Unless the script's first command is `attach`, the four
// database files are destroyed first, so every case starts from nothing.
//
// Script lines (blank lines and '#' comments ignored):
//   attach                      first line only: reuse <db_path>, do not destroy
//   set <hex k> <hex v>
//   get <hex k>                 prints value=<hex|-> vlen=<n> and crc=<8 hex>
//   crc <hex k>                 same value, printed as a zlib crc32 of all bytes
//   getall                      get_by_prefix("") in scan order
//   prefix <hex p>              get_by_prefix(p): keys arrive with p stripped
//   range <hex from> <hex till> get_by_range(from, till): keys are NOT stripped
//   erase <hex k> | erasebatch <hex k>...
//   eraseprefix <hex p>         counts the matches first, then erase_by_prefix
//   count                       get_all().size() -- CHECKs on an empty key
//   begin | wbegin | commit     SqliteDb's transaction counter
//   close | reopen | drop
//   destroy                     close, then SqliteDb::destroy on four paths
//   footprint                   existence of <db>, -journal, -wal, -shm (0/1)
//
// `-` is the empty byte string. Values are hex-truncated to 32 bytes; `vlen`
// carries the real length, and `crc` pins the full content when it matters.
// Entry lists print `<key>=<value>` in the order the API handed them over --
// that order is part of the criterion, not a presentation detail.
//
// Measured behaviour this probe encodes (docs/facts.md F196/F197):
//   * `get_all()` and `get_by_prefix("")` both run `SELECT k, v FROM kv`, i.e. a
//     full table scan in **rowid** order; `get_by_range` / `get_by_prefix` run
//     the `?1 <= k AND k < ?2` form, which uses the `k BLOB PRIMARY KEY` index
//     and therefore arrives in **key order**. `REPLACE INTO` deletes and
//     re-inserts, so overwriting a key moves it to the end of the rowid order.
//     The same data is thus returned in two different orders depending on which
//     accessor asked for it -- the single most load-bearing fact here.
//   * `get_by_prefix` strips the prefix from the returned keys; `get_by_range`
//     does not (`strip_key_prefix` is the first parameter of the private impl).
//   * `get_by_range("", till)` ignores `till`: an empty `from` selects the
//     all-rows statement. `get_by_range(from, "")` ignores nothing but takes the
//     `?1 <= k` rare form.
//   * `next_prefix` increments the last non-0xFF byte; a prefix of all 0xFF has
//     no successor, so `get_by_prefix` takes the `?1 <= k` rare form and
//     `erase_by_prefix` the matching DELETE. For such a prefix the two forms
//     agree, because no key sorts above 0xFF...FF except those starting with it.
//     `erase_by_prefix("")` therefore deletes the whole table.
//   * `Slice()` carries `""` (not nullptr), so `bind_blob` always binds a
//     zero-length **blob**; a key or value of length 0 is never NULL.
//   * `get_all()` walks the result with `CHECK(!key.empty())`, so one zero-length
//     key in the table aborts the process (0xC0000409). `get_by_prefix("", cb)`
//     does not: the caller's callback decides. That pair is what `--check` judges.
//   * `commit` without a matching `begin` is `Status::Error("No matching begin
//     for commit")`, because the counter lives on the shared RawSqliteDb.
//
// Build from the repo root against a configured C:/tmp/td-build:
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I sqlite -I tddb
//   -I tdutils -I tdactor -I C:/tmp/td-build/tdutils tools/fixtures/sqlite_kv_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/sqlite/libtdsqlite.a
//   C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group
//   -lssl -lcrypto -lz -lws2_32 -lpsapi -o C:/tmp/td-probe/sqlite_kv_probe.exe
//
// Needs C:/msys64/ucrt64/bin on PATH at run time (libstdc++-6.dll, libwinpthread-1.dll).

#include "td/db/DbKey.h"
#include "td/db/SqliteDb.h"
#include "td/db/SqliteKeyValue.h"

#include "td/utils/Slice.h"
#include "td/utils/Status.h"
#include "td/utils/crypto.h"
#include "td/utils/port/Stat.h"

#include <cstdio>
#include <cstring>
#include <fstream>
#include <string>
#include <vector>

namespace {

constexpr size_t VALUE_CAP = 32;

std::string hex_all(td::Slice s) {
  static const char *d = "0123456789abcdef";
  if (s.empty()) {
    return "-";
  }
  std::string r;
  r.reserve(s.size() * 2);
  for (auto c : s) {
    auto u = static_cast<unsigned char>(c);
    r += d[u >> 4];
    r += d[u & 15];
  }
  return r;
}

std::string hex_head(td::Slice s) {
  return s.size() <= VALUE_CAP ? hex_all(s) : hex_all(s.substr(0, VALUE_CAP));
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

std::string next_prefix(td::Slice prefix) {
  std::string next = prefix.str();
  size_t pos = next.size();
  while (pos) {
    pos--;
    auto value = static_cast<unsigned char>(next[pos]);
    value++;
    next[pos] = static_cast<char>(value);
    if (value != 0) {
      return next;
    }
  }
  return std::string{};
}

struct State {
  td::SqliteKeyValue kv;
  std::string path;
  bool is_open = false;
};

bool open_kv(State &s, bool allow_create) {
  auto r_db = td::SqliteDb::open_with_key(s.path, allow_create, td::DbKey::empty());
  if (r_db.is_error()) {
    std::printf("reject open reason=%s\n", r_db.error().message().str().c_str());
    return false;
  }
  auto status = s.kv.init_with_connection(r_db.move_as_ok(), "kv");
  if (status.is_error()) {
    std::printf("reject init reason=%s\n", status.message().str().c_str());
    return false;
  }
  s.is_open = true;
  return true;
}

using Rows = std::vector<std::pair<std::string, std::string>>;

Rows collect(td::SqliteKeyValue &kv, td::Slice prefix) {
  Rows rows;
  kv.get_by_prefix(prefix, [&rows](td::Slice key, td::Slice value) {
    rows.emplace_back(key.str(), value.str());
    return true;
  });
  return rows;
}

void print_rows(td::Slice tag, const Rows &rows) {
  std::printf("%s n=%zu", tag.str().c_str(), rows.size());
  for (auto &row : rows) {
    std::printf(" %s=%s", hex_all(row.first).c_str(), hex_head(row.second).c_str());
  }
  std::printf("\n");
}

const char *yes(bool flag) {
  return flag ? "1" : "0";
}

bool exists(const std::string &path) {
  return !td::stat(path).is_error();
}

int run_script(const char *script_path, const char *db_path) {
  std::ifstream in(script_path);
  if (!in) {
    std::fprintf(stderr, "cannot read %s\n", script_path);
    return 2;
  }
  td::CSlice path(db_path);
  State s;
  s.path = db_path;

  bool attached = false;
  bool first = true;
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

    if (first && cmd == "attach") {
      attached = true;
      if (!open_kv(s, false)) {
        return 3;
      }
      std::printf("attach ok=1 n=%zu\n", collect(s.kv, "").size());
      first = false;
      continue;
    }
    if (first) {
      td::SqliteDb::destroy(path).ignore();
      if (!open_kv(s, true)) {
        return 3;
      }
      first = false;
    }
    (void)attached;

    std::string key;
    std::string value;
    if (cmd == "set") {
      if (tok.size() != 3 || !from_hex(tok[1], key) || !from_hex(tok[2], value)) {
        std::printf("reject set argc=%zu\n", tok.size());
        continue;
      }
      s.kv.set(key, value);
      std::printf("set key=%s vlen=%zu\n", hex_all(key).c_str(), value.size());
    } else if (cmd == "get" || cmd == "crc") {
      if (tok.size() != 2 || !from_hex(tok[1], key)) {
        std::printf("reject %s argc=%zu\n", cmd.c_str(), tok.size());
        continue;
      }
      auto got = s.kv.get(key);
      if (cmd == "get") {
        std::printf("get key=%s value=%s vlen=%zu\n", hex_all(key).c_str(), hex_head(got).c_str(), got.size());
      } else {
        std::printf("crc key=%s vlen=%zu crc=%08x\n", hex_all(key).c_str(), got.size(),
                    static_cast<unsigned>(td::crc32(got)));
      }
    } else if (cmd == "getall") {
      print_rows("getall", collect(s.kv, ""));
    } else if (cmd == "prefix") {
      if (tok.size() != 2 || !from_hex(tok[1], key)) {
        std::printf("reject prefix argc=%zu\n", tok.size());
        continue;
      }
      auto rows = collect(s.kv, key);
      std::printf("prefix p=%s", hex_all(key).c_str());
      std::printf(" n=%zu", rows.size());
      for (auto &row : rows) {
        std::printf(" %s=%s", hex_all(row.first).c_str(), hex_head(row.second).c_str());
      }
      std::printf("\n");
    } else if (cmd == "range") {
      std::string from;
      std::string till;
      if (tok.size() != 3 || !from_hex(tok[1], from) || !from_hex(tok[2], till)) {
        std::printf("reject range argc=%zu\n", tok.size());
        continue;
      }
      Rows rows;
      s.kv.get_by_range(from, till, [&rows](td::Slice k, td::Slice v) {
        rows.emplace_back(k.str(), v.str());
        return true;
      });
      std::printf("range from=%s till=%s n=%zu", hex_all(from).c_str(), hex_all(till).c_str(), rows.size());
      for (auto &row : rows) {
        std::printf(" %s=%s", hex_all(row.first).c_str(), hex_head(row.second).c_str());
      }
      std::printf("\n");
    } else if (cmd == "erase") {
      if (tok.size() != 2 || !from_hex(tok[1], key)) {
        std::printf("reject erase argc=%zu\n", tok.size());
        continue;
      }
      s.kv.erase(key);
      std::printf("erase key=%s\n", hex_all(key).c_str());
    } else if (cmd == "erasebatch") {
      std::vector<std::string> keys;
      bool ok = true;
      for (size_t i = 1; i < tok.size(); i++) {
        std::string k;
        if (!from_hex(tok[i], k)) {
          ok = false;
          break;
        }
        keys.push_back(std::move(k));
      }
      if (!ok) {
        std::printf("reject erasebatch\n");
        continue;
      }
      s.kv.erase_batch(std::move(keys));
      std::printf("erasebatch keys=%zu\n", tok.size() - 1);
    } else if (cmd == "eraseprefix") {
      if (tok.size() != 2 || !from_hex(tok[1], key)) {
        std::printf("reject eraseprefix argc=%zu\n", tok.size());
        continue;
      }
      auto removed = collect(s.kv, key).size();
      s.kv.erase_by_prefix(key);
      std::printf("eraseprefix p=%s removed=%zu\n", hex_all(key).c_str(), removed);
    } else if (cmd == "count") {
      std::printf("count n=%zu\n", s.kv.get_all().size());
    } else if (cmd == "begin" || cmd == "wbegin" || cmd == "commit") {
      td::Status status;
      if (cmd == "begin") {
        status = s.kv.begin_read_transaction();
      } else if (cmd == "wbegin") {
        status = s.kv.begin_write_transaction();
      } else {
        status = s.kv.commit_transaction();
      }
      if (status.is_ok()) {
        std::printf("%s ok=1\n", cmd.c_str());
      } else {
        std::printf("%s ok=0 err=%s\n", cmd.c_str(), status.message().str().c_str());
      }
    } else if (cmd == "close") {
      s.kv.close();
      s.is_open = false;
      std::printf("close ok=1 empty=%s\n", yes(s.kv.empty()));
    } else if (cmd == "reopen") {
      s.kv.close();
      s.is_open = false;
      if (!open_kv(s, true)) {
        return 3;
      }
      std::printf("reopen ok=1 n=%zu\n", collect(s.kv, "").size());
    } else if (cmd == "drop") {
      auto status = s.kv.drop();
      std::printf("drop ok=%s empty=%s\n", yes(status.is_ok()), yes(s.kv.empty()));
    } else if (cmd == "destroy") {
      s.kv.close();
      s.is_open = false;
      auto status = td::SqliteDb::destroy(path);
      std::printf("destroy ok=%s\n", yes(status.is_ok()));
    } else if (cmd == "footprint") {
      std::printf("footprint f=%s j=%s w=%s s=%s\n", exists(s.path) ? "1" : "0",
                  exists(s.path + "-journal") ? "1" : "0", exists(s.path + "-wal") ? "1" : "0",
                  exists(s.path + "-shm") ? "1" : "0");
    } else {
      std::printf("reject unknown=%s\n", cmd.c_str());
    }
  }
  if (s.is_open) {
    s.kv.close();
  }
  return 0;
}

int run_check(const char *db_path, const char *case_name) {
  td::CSlice path(db_path);
  td::SqliteDb::destroy(path).ignore();
  State s;
  s.path = db_path;
  if (!open_kv(s, true)) {
    return 3;
  }
  s.kv.set("", "x");
  if (std::strcmp(case_name, "empty_getall") == 0) {
    std::printf("before n=%zu\n", s.kv.get_all().size());
    std::printf("after\n");
  } else if (std::strcmp(case_name, "empty_prefix") == 0) {
    std::printf("before n=%zu\n", collect(s.kv, "").size());
    std::printf("after\n");
  } else {
    std::fprintf(stderr, "unknown check case %s\n", case_name);
    return 2;
  }
  s.kv.close();
  return 0;
}

}  // namespace

int main(int argc, char **argv) {
  if (argc == 4 && std::strcmp(argv[1], "--check") == 0) {
    return run_check(argv[2], argv[3]);
  }
  if (argc != 3) {
    std::fprintf(stderr, "usage: sqlite_kv_probe <script.txt> <db_path>\n"
                         "       sqlite_kv_probe --check <db_path> <case>\n");
    return 2;
  }
  return run_script(argv[1], argv[2]);
}