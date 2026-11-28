// Oracle for the td::Binlog write path (tddb/td/db/binlog/Binlog.{h,cpp} + BinlogEvent.cpp).
//
//   binlog_write_probe [--key empty|raw] <script.txt> <out_binlog>
//
// Plain `td::Binlog` is the driver: it is synchronous and needs no
// Scheduler/actor at all (`Binlog::init` -> `load_binlog` -> `fd_.flush_write`),
// unlike `ConcurrentBinlog`, whose every write is an actor message.  The probe
// therefore never touches tdactor.
//
// `--key empty` (the default) uses `DbKey::empty()`, so no AesCtrEncryption
// header event is ever generated and the file stays plaintext. `--key raw`
// opens with `DbKey::raw_key("cucumber")`: a fresh file is then reindexed by
// `Binlog::init` and starts with a plaintext `type = -3` header event carrying a
// random 32-byte key_salt, a random 16-byte iv and
// `key_hash = HMAC_SHA256(PBKDF2_SHA256(key, key_salt, 2, 32), "cucumbers
// everywhere")`; every following frame is AES-256-CTR ciphertext whose keystream
// continues across frames *and* flush boundaries (AesCtrByteFlow emits partial
// blocks immediately).
//
// The out file is destroyed first (Binlog::destroy) so every run starts empty,
// then opened with init().  LOG output goes to stderr; stdout is the narration.
//
// Script lines (blank lines and '#' comments ignored):
//   add <type> <hexdata>        raw payload, stored through create_storer(Slice)
//   addtl <type> <hexdata>      payload stored as a TL `string` (length prefix + zero pad)
//   rewrite <event_id> <type> <hexdata>
//   erase <event_id>
//   erasebatch <id>,<id>,...
//   change_key empty | change_key raw <hexkey>
//   lazy | flush | sync | close | reopen | nextid
//
// Narration (one line per command, identical on the Rust side):
//   add type=7 id=1 datalen=4
//   addtl type=7 id=2 datalen=4 payload=1
//   skip add type=7 len=3 reason=unaligned
//   rewrite seq=3 target=1 type=7 datalen=8
//   erase seq=4 target=2
//   erasebatch seq=5 targets=2
//   change_key raw                          (or `change_key empty`)
//   lazy / flush / sync / close ok=1 / reopen ok=1 / nextid 6
//   replay id=1 type=7 flags=0 datalen=8 data=4142434445464748   (per loaded live event)
//   summary frames=2 size=80 sha256=<64 hex> leftover=0
//   summary encrypted size=124              (first frame is type = -3)
//
// The encrypted summary drops the frame walk and the hash: the salt and iv are
// random per run, so no two encrypted files are ever byte equal. Two runs of the
// same encrypted script agree on the total size, on the first frame's header
// (size/id/type/flags/extra) and on the decrypted events, and differ from the
// first payload byte on -- that is what tools/binlog_write_diff.py judges.
//
// Measured preconditions this probe encodes (see docs/facts.md F185):
//   * `Binlog::add_event` LOG(FATAL)s on `size % 4 != 0` -- with a raw Slice
//     storer that means the payload length must be a multiple of 4, otherwise
//     the process dies at Binlog.cpp:232 with "Trying to add event with bad size".
//     The probe refuses such an `add` instead of dying, so one script can carry
//     the case and both sides can be compared.
//   * `rewrite`/`erase` of an id that is not live makes
//     BinlogEventsProcessor::do_event return "Ignore rewrite log event", which
//     Binlog turns into LOG(FATAL) as well; the corpus only touches live ids.
//   * re-`init`ing the *same* `td::Binlog` object after an encrypted session
//     crashes with an access violation (0xC0000005) inside the second init; the
//     file on disk is still intact afterwards, so nothing is lost. A fresh object
//     per open is what TdDb (a `new Binlog` per open_impl) and test/db.cpp do, and
//     that is what `reopen` does here. The likely carry-over is the read side's
//     `encryption_type_`, which is not reset by `init` -- not measured further,
//     since it is outside the write path.
//
// Build from the repo root against a configured C:/tmp/td-build (-lz is needed
// because td::crc32 is zlib's crc32):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I tdutils -I tdactor
//   -I tddb -I C:/tmp/td-build/tdutils tools/fixtures/binlog_write_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/tddb/libtddb.a
//   C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group
//   -lssl -lcrypto -lz -lws2_32 -lpsapi -o C:/tmp/td-probe/binlog_write_probe.exe

#include "td/db/binlog/Binlog.h"

#include "td/db/DbKey.h"

#include "td/utils/crypto.h"
#include "td/utils/logging.h"
#include "td/utils/Slice.h"
#include "td/utils/Storer.h"
#include "td/utils/tl_helpers.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <memory>
#include <sstream>
#include <string>
#include <vector>

namespace {

constexpr size_t HEADER_SIZE = 4 + 8 + 4 + 4 + 8;
constexpr size_t TAIL_SIZE = 4;
constexpr size_t MIN_SIZE = HEADER_SIZE + TAIL_SIZE;
constexpr td::int32 SERVICE_AES_CTR_ENCRYPTION = -3;

// `DefaultStorer<T>` needs an object with a `store(StorerT &)` member, so the
// payload is wrapped in one thin struct; the layout itself is still produced by
// td::store() -> TlStorerUnsafe::store_string (length prefix + zero padding).
struct TlString {
  const std::string &value;
  template <class StorerT>
  void store(StorerT &storer) const {
    td::store(value, storer);
  }
};

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
  if (text == "-") {  // "-" is the empty payload, so a zero-length add stays a token
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

uint32_t load_u32(const std::string &data, size_t offset) {
  uint32_t value = 0;
  std::memcpy(&value, data.data() + offset, 4);
  return value;
}

int32_t load_i32(const std::string &data, size_t offset) {
  return static_cast<int32_t>(load_u32(data, offset));
}

void print_replay(const td::BinlogEvent &event) {
  auto data = event.get_data();
  std::printf("replay id=%llu type=%d flags=%d datalen=%zu data=%s\n", static_cast<unsigned long long>(event.id_),
              event.type_, event.flags_, data.size(), to_hex(data, 32).c_str());
}

void print_summary(const std::string &path) {
  auto data = read_file(path);
  // An encrypted binlog ends where its first frame ends: everything after it is
  // AES-CTR ciphertext, and the salt/iv are random, so the walk and the hash
  // would both be meaningless.
  if (data.size() >= MIN_SIZE) {
    uint32_t size = load_u32(data, 0);
    if (size >= MIN_SIZE && size <= data.size() && load_i32(data, 12) == SERVICE_AES_CTR_ENCRYPTION) {
      std::printf("summary encrypted size=%zu\n", data.size());
      return;
    }
  }
  size_t frames = 0;
  size_t offset = 0;
  while (offset + 4 <= data.size()) {
    uint32_t size = load_u32(data, offset);
    if (size < MIN_SIZE || offset + size > data.size()) {
      break;
    }
    frames++;
    offset += size;
  }
  std::string digest(32, '\0');
  td::sha256(data, digest);
  std::printf("summary frames=%zu size=%zu sha256=%s leftover=%zu\n", frames, data.size(),
              to_hex(digest, digest.size()).c_str(), data.size() - offset);
}

int fail(const char *what, const std::string &line) {
  std::fprintf(stderr, "binlog_write_probe: %s in line %s\n", what, line.c_str());
  return 2;
}

}  // namespace

int main(int argc, char *argv[]) {
  if (argc != 3 && argc != 5) {
    std::fprintf(stderr, "Usage: binlog_write_probe [--key empty|raw] <script.txt> <out_binlog>\n");
    return 2;
  }
  // Unbuffered: LOG(FATAL)/access violations kill the process without flushing
  // stdio, and a lost narration line makes a crash much harder to place.
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  SET_VERBOSITY_LEVEL(VERBOSITY_NAME(ERROR));

  int arg = 1;
  td::DbKey current_key = td::DbKey::empty();
  if (argc == 5) {
    std::string flag = argv[arg];
    std::string mode = argv[arg + 1];
    if (flag != "--key" || (mode != "empty" && mode != "raw")) {
      std::fprintf(stderr, "Usage: binlog_write_probe [--key empty|raw] <script.txt> <out_binlog>\n");
      return 2;
    }
    if (mode == "raw") {
      current_key = td::DbKey::raw_key("cucumber");
    }
    arg += 2;
  }
  std::string script_path = argv[arg];
  std::string out_path = argv[arg + 1];

  td::Binlog::destroy(out_path).ignore();
  // A fresh object per open: re-init'ing the same instance after an encrypted
  // session crashes (the stale `encryption_type_` drives the reader through the
  // AES byte flow over a plaintext header). TdDb and test/db.cpp reopen through a
  // new object too, and the Rust side just reloads the file.
  std::unique_ptr<td::Binlog> binlog;
  auto open = [&]() {
    binlog.reset();
    binlog = std::make_unique<td::Binlog>();
    return binlog->init(out_path, [](const td::BinlogEvent &event) { print_replay(event); }, current_key);
  };
  if (open().is_error()) {
    std::fprintf(stderr, "binlog_write_probe: init failed\n");
    return 2;
  }

  std::ifstream script(script_path);
  if (!script) {
    std::fprintf(stderr, "binlog_write_probe: cannot read %s\n", script_path.c_str());
    return 2;
  }

  std::string line;
  while (std::getline(script, line)) {
    while (!line.empty() && (line.back() == '\r' || line.back() == ' ' || line.back() == '\t')) {
      line.pop_back();
    }
    if (line.empty() || line[0] == '#') {
      continue;
    }
    std::istringstream words(line);
    std::string command;
    words >> command;

    if (command == "add" || command == "addtl") {
      std::string type_text;
      std::string hex;
      words >> type_text >> hex;
      auto type = static_cast<td::int32>(std::strtoll(type_text.c_str(), nullptr, 10));
      std::string payload;
      if (words.fail() || !parse_hex(hex, &payload)) {
        return fail("bad add", line);
      }
      if (command == "add") {
        if (payload.size() % 4 != 0) {
          std::printf("skip add type=%d len=%zu reason=unaligned\n", type, payload.size());
          continue;
        }
        auto storer = td::create_storer(td::Slice(payload));
        auto data_size = storer.size();
        auto id = binlog->add(type, storer);
        std::printf("add type=%d id=%llu datalen=%zu\n", type, static_cast<unsigned long long>(id), data_size);
      } else {
        TlString wrapped{payload};
        auto storer = td::create_default_storer(wrapped);
        auto data_size = storer.size();
        auto id = binlog->add(type, storer);
        std::printf("addtl type=%d id=%llu datalen=%zu payload=%zu\n", type, static_cast<unsigned long long>(id),
                    data_size, payload.size());
      }
      continue;
    }

    if (command == "rewrite") {
      std::string target_text;
      std::string type_text;
      std::string hex;
      words >> target_text >> type_text >> hex;
      auto target = std::strtoull(target_text.c_str(), nullptr, 10);
      auto type = static_cast<td::int32>(std::strtoll(type_text.c_str(), nullptr, 10));
      std::string payload;
      if (words.fail() || !parse_hex(hex, &payload)) {
        return fail("bad rewrite", line);
      }
      if (payload.size() % 4 != 0) {
        std::printf("skip rewrite target=%llu type=%d len=%zu reason=unaligned\n",
                    static_cast<unsigned long long>(target), type, payload.size());
        continue;
      }
      auto storer = td::create_storer(td::Slice(payload));
      auto data_size = storer.size();
      auto seq = binlog->rewrite(target, type, storer);
      std::printf("rewrite seq=%llu target=%llu type=%d datalen=%zu\n", static_cast<unsigned long long>(seq),
                  static_cast<unsigned long long>(target), type, data_size);
      continue;
    }

    if (command == "erase") {
      std::string target_text;
      words >> target_text;
      if (words.fail()) {
        return fail("bad erase", line);
      }
      auto target = std::strtoull(target_text.c_str(), nullptr, 10);
      auto seq = binlog->erase(target);
      std::printf("erase seq=%llu target=%llu\n", static_cast<unsigned long long>(seq),
                  static_cast<unsigned long long>(target));
      continue;
    }

    if (command == "erasebatch") {
      std::string list;
      words >> list;
      if (words.fail()) {
        return fail("bad erasebatch", line);
      }
      std::vector<td::uint64> ids;
      size_t start = 0;
      while (start <= list.size()) {
        auto end = list.find(',', start);
        if (end == std::string::npos) {
          end = list.size();
        }
        ids.push_back(std::strtoull(list.substr(start, end - start).c_str(), nullptr, 10));
        start = end + 1;
      }
      auto seq = binlog->erase_batch(ids);
      std::printf("erasebatch seq=%llu targets=%zu\n", static_cast<unsigned long long>(seq), ids.size());
      continue;
    }

    if (command == "change_key") {
      std::string mode;
      words >> mode;
      if (mode == "empty") {
        current_key = td::DbKey::empty();
      } else if (mode == "raw") {
        std::string hex;
        words >> hex;
        std::string key_bytes;
        if (words.fail() || !parse_hex(hex, &key_bytes)) {
          return fail("bad change_key", line);
        }
        current_key = td::DbKey::raw_key(key_bytes);
      } else {
        return fail("bad change_key mode", line);
      }
      // Binlog::change_key -> do_reindex: the file is regenerated from the live
      // set with a fresh header (new salt and iv) when a key is in use.
      binlog->change_key(current_key);
      std::printf("change_key %s\n", mode.c_str());
      continue;
    }

    if (command == "lazy") {
      binlog->lazy_flush();
      std::printf("lazy\n");
      continue;
    }
    if (command == "flush") {
      binlog->flush("probe");
      std::printf("flush\n");
      continue;
    }
    if (command == "sync") {
      binlog->sync("probe");
      std::printf("sync\n");
      continue;
    }
    if (command == "close") {
      auto status = binlog->close();
      std::printf("close ok=%d\n", status.is_ok() ? 1 : 0);
      continue;
    }
    if (command == "reopen") {
      auto status = open();
      std::printf("reopen ok=%d\n", status.is_ok() ? 1 : 0);
      continue;
    }
    if (command == "nextid") {
      std::printf("nextid %llu\n", static_cast<unsigned long long>(binlog->next_event_id()));
      continue;
    }

    return fail("unknown command", line);
  }

  print_summary(out_path);
  return 0;
}