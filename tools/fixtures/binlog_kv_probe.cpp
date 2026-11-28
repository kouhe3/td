// Oracle for td::BinlogKeyValue<td::Binlog> (tddb/td/db/BinlogKeyValue.h) and
// td::SeqKeyValue (tddb/td/db/SeqKeyValue.h).
//
//   binlog_kv_probe <script.txt> <out_binlog>
//
// The probe drives a real `td::BinlogKeyValue<td::Binlog>` opened with
// `init(path, DbKey::empty(), -1, override_magic)` -- plaintext, no Scheduler,
// no actor. The file is destroyed first (Binlog::destroy) unless the script's
// first command is `attach`, which opens the existing file and replays it.
//
// Script lines (blank lines and '#' comments ignored):
//   attach                      first line only: reuse <out_binlog>, do not destroy
//   magic <n>                   re-init with override_magic = n (0 = "no override")
//   set <hex key> <hex value>
//   erase <hex key>
//   erase_batch <hex key>...
//   eraseprefix <hex prefix>
//   get <hex key> | isset <hex key> | getall | prefix <hex prefix> | foreach
//   seq                         the last SeqNo any mutating op returned
//   sync | close | reopen | dumpfile
//   raw <type> <hex data>       append one frame with a fresh td::Binlog while the
//                               KV is closed (a foreign writer)
//   sq_set <hex key> <hex value> | sq_erase <hex key> | sq_erase_batch <hex key>...
//   sq_get <hex key> | sq_isset <hex key> | sq_getall | sq_seq | sq_size
//
// `-` is the empty byte string, for keys, values and prefixes alike. Every
// command prints exactly one narration line (`getall`/`prefix`/`foreach` and the
// `sq_` twins append their entries, sorted by key bytes, on the same line):
//   attach ok=1 size=0
//   magic 17191 ok=1 size=0
//   set key=61 value=62 vlen=1 seq=1
//   reject set key=- reason=empty-key
//   erase key=61 seq=2
//   erasebatch keys=3 seq=3
//   eraseprefix p=70 removed=1
//   get key=61 value=- vlen=0
//   isset key=61 value=0
//   getall n=2 key=61=62 key=63=64
//   prefix p=61 n=1 key=62=62
//   foreach n=2 key=61=62 key=63=64
//   seq 4
//   sync
//   close ok=1
//   reopen ok=1 size=2
//   dumpfile size=76 sha256=<64 hex>
//   raw type=12345 id=3 datalen=12      (or `reject raw reason=open|unaligned`)
//   sq set key=61 value=62 vlen=1 seq=1
//   sq seq 2
// Values in narration are hex-truncated to 32 bytes; `vlen` carries the real
// length, so a 64 KiB value does not flood the comparison (its bytes are pinned
// by the file comparison instead).
//
// `dumpfile` reports the bytes on disk, so it needs the KV closed: an open Binlog
// holds a whole-file write lock (`Binlog::open_binlog`), and on Windows a second
// handle cannot read a single byte of a locked range -- the probe prints
// `reject dumpfile reason=open` rather than silently reporting size 0. `sync` is
// the public flush path (`force_sync` -> `Binlog::sync`) while the KV is open;
// it cannot be observed until a later `dumpfile`.
//
// Measured behaviour this probe encodes (docs/facts.md F191/F192):
//   * `init`'s replay callback (`BinlogKeyValue.h:88-104`) parses *every* event
//     the Binlog hands it -- there is no type/magic filter anywhere in the
//     template. What filters is TdDb's own switch (td/telegram/TdDb.cpp:170-181,
//     `case BinlogPmcMagic/ConfigPmcMagic`) plus `external_init_handle`, which is
//     the path TdDb actually uses (TdDb.cpp:519). A positive foreign type in the
//     file therefore lands in the map with its own event id, and a later `set`
//     of that key is a Rewrite targeting it.
//   * the file's ids are *not* the sequence numbers: for `Binlog` the
//     `add_event(seq_no, …)` specialization drops `seq_no` and writes the frame
//     built by `create_raw(event_id, …)`, where `event_id` is the key's previous
//     event id (`BinlogKeyValue.h:214-234`). Only a first-time `set` writes
//     `id = seq_no`.
//   * `init` begins with `close()`, and `close()` is `*this = BinlogKeyValue()`,
//     so every init *without* an override resets `magic_` to MAGIC
//     (0x2a280000) and drops the map. Only `init(…, override_magic = n)`
//     installs an override, and it survives exactly until the next `reopen`:
//     `magic 17191` then `reopen` then `set` writes 0x2a280000 again (measured
//     in the magic_pmc/magic_config cases); `magic 0` is the same reset.
//   * an erase frame is `type = -2`, `flags = 1`, zero data bytes; a `set` of an
//     empty value is a `type = magic` frame whose body is two TL strings, so it
//     is at least 8 bytes. `get` cannot separate an erased key from an empty
//     value -- `isset` is what does.
//   * `erase_by_prefix` collects ids by walking a `WaitFreeHashMap`
//     (an unordered_map), so the *order* of the frames it writes follows that
//     hash table and is not reproducible without porting the table. The corpus
//     only erases at most one key per `eraseprefix`; multi-match removal order is
//     covered by `erase_batch`, whose order is the script's.
//   * `set` with an empty key is a `CHECK(!key.empty())` abort
//     (`BinlogKeyValue.h:200`), so the probe prints `reject` and both sides must
//     agree instead of dying.
//   * `raw` needs the KV closed: the file is write-locked by the open Binlog.
//
// Build from the repo root against a configured C:/tmp/td-build (-lz is needed
// because td::crc32 is zlib's crc32, like in binlog_write_probe.cpp):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I tdutils -I tdactor
//   -I tddb -I C:/tmp/td-build/tdutils tools/fixtures/binlog_kv_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/tddb/libtddb.a
//   C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group
//   -lssl -lcrypto -lz -lws2_32 -lpsapi -o C:/tmp/td-probe/binlog_kv_probe.exe

#include "td/db/BinlogKeyValue.h"
#include "td/db/SeqKeyValue.h"
#include "td/db/binlog/Binlog.h"
#include "td/db/DbKey.h"

#include "td/utils/common.h"
#include "td/utils/crypto.h"
#include "td/utils/logging.h"
#include "td/utils/port/Stat.h"
#include "td/utils/Slice.h"
#include "td/utils/Storer.h"

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <map>
#include <sstream>
#include <string>
#include <vector>

namespace {

constexpr size_t HEX_PREVIEW = 32;

std::string to_hex(td::Slice data, size_t limit) {
  static const char *digits = "0123456789abcdef";
  std::string out;
  auto shown = data.size() < limit ? data.size() : limit;
  out.reserve(shown * 2 + 2);
  for (size_t i = 0; i < shown; i++) {
    auto byte = static_cast<unsigned char>(data[i]);
    out.push_back(digits[byte >> 4]);
    out.push_back(digits[byte & 15]);
  }
  if (shown < data.size()) {
    out += "..";
  }
  if (out.empty()) {
    out = "-";
  }
  return out;
}

bool parse_hex(const std::string &text, std::string *out) {
  if (text == "-") {  // "-" is the empty byte string, so a zero-length token stays a token
    out->clear();
    return true;
  }
  if (text.size() % 2 != 0) {
    return false;
  }
  out->clear();
  out->reserve(text.size() / 2);
  for (size_t i = 0; i < text.size(); i += 2) {
    int value = 0;
    for (size_t j = 0; j < 2; j++) {
      char c = text[i + j];
      int digit = 0;
      if (c >= '0' && c <= '9') {
        digit = c - '0';
      } else if (c >= 'a' && c <= 'f') {
        digit = c - 'a' + 10;
      } else if (c >= 'A' && c <= 'F') {
        digit = c - 'A' + 10;
      } else {
        return false;
      }
      value = value * 16 + digit;
    }
    out->push_back(static_cast<char>(value));
  }
  return true;
}

std::string read_file(const std::string &path) {
  std::ifstream in(path, std::ios::binary);
  std::ostringstream buffer;
  buffer << in.rdbuf();
  return buffer.str();
}

std::string sha256_hex(td::Slice data) {
  std::string digest(32, '\0');
  td::sha256(data, digest);
  return to_hex(digest, digest.size());
}

std::string key_text(const std::string &key) {
  return to_hex(key, key.size());
}

int fail(const char *what, const std::string &line) {
  std::fprintf(stderr, "binlog_kv_probe: %s in line %s\n", what, line.c_str());
  return 2;
}

struct Probe {
  std::string path;
  td::BinlogKeyValue<td::Binlog> kv;
  td::SeqKeyValue memory;
  td::uint64 last_seq = 0;
  bool is_open = false;

  size_t size() {
    return kv.get_all().size();
  }

  bool init(td::int32 override_magic) {
    auto status = kv.init(path, td::DbKey::empty(), -1, override_magic);
    is_open = status.is_ok();
    return is_open;
  }

  void print_pairs(const std::map<std::string, std::string> &pairs) {
    for (const auto &it : pairs) {
      std::printf(" %s=%s", key_text(it.first).c_str(), to_hex(it.second, HEX_PREVIEW).c_str());
    }
  }
};

}  // namespace

int main(int argc, char *argv[]) {
  if (argc != 3) {
    std::fprintf(stderr, "Usage: binlog_kv_probe <script.txt> <out_binlog>\n");
    return 2;
  }
  // Unbuffered: a CHECK/LOG(FATAL) kills the process without flushing stdio, and
  // a lost narration line makes a crash much harder to place.
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  SET_VERBOSITY_LEVEL(VERBOSITY_NAME(ERROR));

  std::ifstream script(argv[1]);
  if (!script) {
    std::fprintf(stderr, "binlog_kv_probe: cannot read %s\n", argv[1]);
    return 2;
  }
  std::vector<std::string> lines;
  {
    std::string line;
    while (std::getline(script, line)) {
      while (!line.empty() && (line.back() == '\r' || line.back() == ' ' || line.back() == '\t')) {
        line.pop_back();
      }
      size_t first = line.find_first_not_of(" \t");
      if (first == std::string::npos) {
        continue;
      }
      line = line.substr(first);
      if (line[0] == '#') {
        continue;
      }
      lines.push_back(line);
    }
  }

  Probe probe;
  probe.path = argv[2];
  bool attach = !lines.empty() && lines.front() == "attach";
  if (attach) {
    auto r_stat = td::stat(probe.path);
    probe.init(0);
    std::printf("attach ok=%d size=%zu\n", r_stat.is_ok() ? 1 : 0, probe.size());
    lines.erase(lines.begin());
  } else {
    td::Binlog::destroy(probe.path).ignore();
    if (!probe.init(0)) {
      std::fprintf(stderr, "binlog_kv_probe: init failed\n");
      return 2;
    }
  }

  for (const auto &line : lines) {
    std::istringstream words(line);
    std::string command;
    words >> command;

    if (command == "magic") {
      std::string value_text;
      words >> value_text;
      if (words.fail()) {
        return fail("bad magic", line);
      }
      auto magic = static_cast<td::int32>(std::strtoll(value_text.c_str(), nullptr, 10));
      auto ok = probe.init(magic);
      std::printf("magic %d ok=%d size=%zu\n", magic, ok ? 1 : 0, probe.size());
      continue;
    }

    if (command == "set") {
      std::string key_hex;
      std::string value_hex;
      words >> key_hex >> value_hex;
      std::string key;
      std::string value;
      if (words.fail() || !parse_hex(key_hex, &key) || !parse_hex(value_hex, &value)) {
        return fail("bad set", line);
      }
      if (key.empty()) {
        std::printf("reject set key=- reason=empty-key\n");
        continue;
      }
      if (!probe.is_open) {
        std::printf("reject set reason=closed\n");
        continue;
      }
      auto seq = probe.kv.set(key, value);
      probe.last_seq = seq;
      std::printf("set key=%s value=%s vlen=%zu seq=%llu\n", key_text(key).c_str(),
                  to_hex(value, HEX_PREVIEW).c_str(), value.size(), static_cast<unsigned long long>(seq));
      continue;
    }

    if (command == "erase") {
      std::string key_hex;
      words >> key_hex;
      std::string key;
      if (words.fail() || !parse_hex(key_hex, &key)) {
        return fail("bad erase", line);
      }
      if (!probe.is_open) {
        std::printf("reject erase reason=closed\n");
        continue;
      }
      auto seq = probe.kv.erase(key);
      probe.last_seq = seq;
      std::printf("erase key=%s seq=%llu\n", key_text(key).c_str(), static_cast<unsigned long long>(seq));
      continue;
    }

    if (command == "erase_batch") {
      std::vector<std::string> keys;
      std::string token;
      while (words >> token) {
        std::string key;
        if (!parse_hex(token, &key)) {
          return fail("bad erase_batch hex", line);
        }
        keys.push_back(key);
      }
      if (keys.empty()) {
        return fail("bad erase_batch", line);
      }
      if (!probe.is_open) {
        std::printf("reject erasebatch reason=closed\n");
        continue;
      }
      auto seq = probe.kv.erase_batch(keys);
      probe.last_seq = seq;
      std::printf("erasebatch keys=%zu seq=%llu\n", keys.size(), static_cast<unsigned long long>(seq));
      continue;
    }

    if (command == "eraseprefix") {
      std::string prefix_hex;
      words >> prefix_hex;
      std::string prefix;
      if (words.fail() || !parse_hex(prefix_hex, &prefix)) {
        return fail("bad eraseprefix", line);
      }
      if (!probe.is_open) {
        std::printf("reject eraseprefix reason=closed\n");
        continue;
      }
      auto before = probe.size();
      probe.kv.erase_by_prefix(prefix);
      auto after = probe.size();
      std::printf("eraseprefix p=%s removed=%zu\n", to_hex(prefix, prefix.size()).c_str(), before - after);
      continue;
    }

    if (command == "get") {
      std::string key_hex;
      words >> key_hex;
      std::string key;
      if (words.fail() || !parse_hex(key_hex, &key)) {
        return fail("bad get", line);
      }
      if (!probe.is_open) {
        std::printf("reject get reason=closed\n");
        continue;
      }
      auto value = probe.kv.get(key);
      std::printf("get key=%s value=%s vlen=%zu\n", key_text(key).c_str(), to_hex(value, HEX_PREVIEW).c_str(),
                  value.size());
      continue;
    }

    if (command == "isset") {
      std::string key_hex;
      words >> key_hex;
      std::string key;
      if (words.fail() || !parse_hex(key_hex, &key)) {
        return fail("bad isset", line);
      }
      if (!probe.is_open) {
        std::printf("reject isset reason=closed\n");
        continue;
      }
      std::printf("isset key=%s value=%d\n", key_text(key).c_str(), probe.kv.isset(key) ? 1 : 0);
      continue;
    }

    if (command == "getall" || command == "foreach" || command == "prefix") {
      std::string prefix;
      if (command == "prefix") {
        std::string prefix_hex;
        words >> prefix_hex;
        if (words.fail() || !parse_hex(prefix_hex, &prefix)) {
          return fail("bad prefix", line);
        }
      }
      if (!probe.is_open) {
        std::printf("reject %s reason=closed\n", command.c_str());
        continue;
      }
      std::map<std::string, std::string> sorted;
      if (command == "getall") {
        for (const auto &it : probe.kv.get_all()) {
          sorted.emplace(it.first, it.second);
        }
        std::printf("getall n=%zu", sorted.size());
      } else if (command == "prefix") {
        for (const auto &it : probe.kv.prefix_get(prefix)) {
          sorted.emplace(it.first, it.second);
        }
        std::printf("prefix p=%s n=%zu", to_hex(prefix, prefix.size()).c_str(), sorted.size());
      } else {
        probe.kv.for_each([&sorted](td::Slice key, td::Slice value) { sorted.emplace(key.str(), value.str()); });
        std::printf("foreach n=%zu", sorted.size());
      }
      probe.print_pairs(sorted);
      std::printf("\n");
      continue;
    }

    if (command == "seq") {
      std::printf("seq %llu\n", static_cast<unsigned long long>(probe.last_seq));
      continue;
    }

    if (command == "sync") {
      if (!probe.is_open) {
        std::printf("reject sync reason=closed\n");
        continue;
      }
      probe.kv.force_sync(td::Auto(), "probe_sync");
      std::printf("sync\n");
      continue;
    }

    if (command == "close") {
      probe.kv.close();
      probe.is_open = false;
      std::printf("close ok=1\n");
      continue;
    }

    if (command == "reopen") {
      auto ok = probe.init(0);
      std::printf("reopen ok=%d size=%zu\n", ok ? 1 : 0, probe.size());
      continue;
    }

    if (command == "dumpfile") {
      if (probe.is_open) {
        // An open Binlog holds a whole-file write lock (Binlog::open_binlog), so
        // a second reader cannot see a single byte of it on Windows.
        std::printf("reject dumpfile reason=open\n");
        continue;
      }
      auto data = read_file(probe.path);
      std::printf("dumpfile size=%zu sha256=%s\n", data.size(), sha256_hex(data).c_str());
      continue;
    }

    if (command == "raw") {
      std::string type_text;
      std::string data_hex;
      words >> type_text >> data_hex;
      auto type = static_cast<td::int32>(std::strtoll(type_text.c_str(), nullptr, 10));
      std::string data;
      if (words.fail() || !parse_hex(data_hex, &data)) {
        return fail("bad raw", line);
      }
      if (probe.is_open) {
        std::printf("reject raw reason=open\n");
        continue;
      }
      if (data.size() % 4 != 0) {
        std::printf("reject raw reason=unaligned\n");
        continue;
      }
      // A foreign writer: the KV's own file lock is released, so a second
      // td::Binlog can append one frame and close again.
      td::Binlog binlog;
      auto status = binlog.init(probe.path, [](const td::BinlogEvent &) {}, td::DbKey::empty());
      if (status.is_error()) {
        return fail("raw init failed", line);
      }
      auto id = binlog.next_event_id();
      binlog.add_raw_event(td::BinlogEvent::create_raw(id, type, 0, td::create_storer(td::Slice(data))),
                           td::BinlogDebugInfo{__FILE__, __LINE__});
      binlog.sync("probe_raw");
      auto close_status = binlog.close();
      std::printf("raw type=%d id=%llu datalen=%zu ok=%d\n", type, static_cast<unsigned long long>(id), data.size(),
                  close_status.is_ok() ? 1 : 0);
      continue;
    }

    if (command == "sq_set") {
      std::string key_hex;
      std::string value_hex;
      words >> key_hex >> value_hex;
      std::string key;
      std::string value;
      if (words.fail() || !parse_hex(key_hex, &key) || !parse_hex(value_hex, &value)) {
        return fail("bad sq_set", line);
      }
      if (key.empty()) {
        std::printf("reject sq set key=- reason=empty-key\n");
        continue;
      }
      auto seq = probe.memory.set(key, value);
      std::printf("sq set key=%s value=%s vlen=%zu seq=%llu\n", key_text(key).c_str(),
                  to_hex(value, HEX_PREVIEW).c_str(), value.size(), static_cast<unsigned long long>(seq));
      continue;
    }

    if (command == "sq_erase") {
      std::string key_hex;
      words >> key_hex;
      std::string key;
      if (words.fail() || !parse_hex(key_hex, &key)) {
        return fail("bad sq_erase", line);
      }
      auto seq = probe.memory.erase(key);
      std::printf("sq erase key=%s seq=%llu\n", key_text(key).c_str(), static_cast<unsigned long long>(seq));
      continue;
    }

    if (command == "sq_erase_batch") {
      std::vector<std::string> keys;
      std::string token;
      while (words >> token) {
        std::string key;
        if (!parse_hex(token, &key)) {
          return fail("bad sq_erase_batch hex", line);
        }
        keys.push_back(key);
      }
      if (keys.empty()) {
        return fail("bad sq_erase_batch", line);
      }
      auto seq = probe.memory.erase_batch(keys);
      std::printf("sq erasebatch keys=%zu seq=%llu\n", keys.size(), static_cast<unsigned long long>(seq));
      continue;
    }

    if (command == "sq_get") {
      std::string key_hex;
      words >> key_hex;
      std::string key;
      if (words.fail() || !parse_hex(key_hex, &key)) {
        return fail("bad sq_get", line);
      }
      auto value = probe.memory.get(key);
      std::printf("sq get key=%s value=%s vlen=%zu\n", key_text(key).c_str(), to_hex(value, HEX_PREVIEW).c_str(),
                  value.size());
      continue;
    }

    if (command == "sq_isset") {
      std::string key_hex;
      words >> key_hex;
      std::string key;
      if (words.fail() || !parse_hex(key_hex, &key)) {
        return fail("bad sq_isset", line);
      }
      std::printf("sq isset key=%s value=%d\n", key_text(key).c_str(), probe.memory.isset(key) ? 1 : 0);
      continue;
    }

    if (command == "sq_getall") {
      std::map<std::string, std::string> sorted;
      for (const auto &it : probe.memory.get_all()) {
        sorted.emplace(it.first, it.second);
      }
      std::printf("sq getall n=%zu", sorted.size());
      probe.print_pairs(sorted);
      std::printf("\n");
      continue;
    }

    if (command == "sq_seq") {
      std::printf("sq seq %llu\n", static_cast<unsigned long long>(probe.memory.seq_no()));
      continue;
    }

    if (command == "sq_size") {
      std::printf("sq size %zu\n", probe.memory.size());
      continue;
    }

    return fail("unknown command", line);
  }

  return 0;
}