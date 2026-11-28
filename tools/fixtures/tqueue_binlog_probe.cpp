// Oracle for td::TQueueBinlog<td::Binlog> (tddb/td/db/TQueue.h) driven by a script.
//
//   tqueue_binlog_probe <script.txt> <binlog_path>
//
// A real `td::Binlog` opened at <binlog_path> with `DbKey::empty()` (plaintext),
// a real `td::TQueue::create()`, and `TQueueBinlog<Binlog>` between them as the
// `StorageCallback`. So every queue mutation turns into a binlog frame:
//
//   push   -> add(2314 + (extra != 0), TQueueLogEvent)      (first time)
//          -> rewrite(log_event_id, ...)                     (rewrite)
//   pop    -> erase(log_event_id)                            (single)
//   pop_batch -> erase_batch(log_event_ids)                  (clear / get sweep)
//
// Script lines (blank lines and '#' comments ignored):
//   push <queue> <hex data> <expires_at> <extra> <hint|->
//   get <queue> <from|-> <forget 0|1> <now> <max>
//   forget <queue> <id>
//   clear <queue> <keep_count>
//   head <queue> | tail <queue> | size <queue>
//   gc <now>
//   close                 closes the binlog (the file is locked while it is open)
//   dumpfile              size + sha256 of the file; only legal after `close`
//   replay <queue>...     re-open the file, replay every live frame into a fresh
//                         queue, report head/tail/size (also only after `close`)
//
// Measured behaviour this probe encodes (docs/facts.md F202/F203):
//   * the frame body is `TQueueLogEvent`: `queue_id:i64 ‖ event_id:i32 ‖
//     expires_at:i32 ‖ data:TL-string ‖ [extra:i64 only when != 0]`, so an event
//     with `extra = 7` is a 28-byte frame and one with `extra = 0` is 20 bytes;
//   * `magic = BINLOG_EVENT_TYPE + (extra != 0)`, i.e. 2314 or 2315 -- and a
//     rewrite re-reads the *current* extra, so an event whose data was cleared
//     and whose extra was 0 becomes a 2314 frame;
//   * an erase frame is `type = -2`, `flags = 1`, no data (32 bytes), and its id
//     is the erased event's log_event_id, never a sequence number;
//   * `erase_batch` takes `next_event_id(0)` (the counter does not move) and then
//     erases one id at a time.
//
// Build from the repo root against a configured C:/tmp/td-build:
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I tddb -I tdutils -I tdactor
//   -I C:/tmp/td-build/tdutils tools/fixtures/tqueue_binlog_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdutils/libtdutils.a
//   C:/tmp/td-build/tdactor/libtdactor.a -Wl,--end-group
//   -lssl -lcrypto -lz -lws2_32 -lpsapi -o C:/tmp/td-probe/tqueue_binlog_probe.exe

#include "td/db/TQueue.h"
#include "td/db/binlog/Binlog.h"
#include "td/db/binlog/BinlogEvent.h"

#include "td/utils/Span.h"
#include "td/utils/crypto.h"
#include "td/utils/port/Stat.h"

#include <cstdio>
#include <cstring>
#include <fstream>
#include <memory>
#include <string>
#include <vector>

namespace {

constexpr size_t VALUE_CAP = 32;
constexpr size_t MAX_RESULT_EVENTS = 4096;

using EventId = td::TQueue::EventId;
using QueueId = td::TQueue::QueueId;

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

bool parse_i32(const std::string &s, int32_t &out) {
  int64_t v;
  if (!parse_i64(s, v) || v < INT32_MIN || v > INT32_MAX) {
    return false;
  }
  out = static_cast<int32_t>(v);
  return true;
}

struct Env {
  std::string path;
  std::shared_ptr<td::Binlog> binlog;
  td::unique_ptr<td::TQueue> queue;
  bool open = false;

  /// Open (or re-open) the binlog and replay its live frames into `queue`.
  bool init(uint64_t *replayed) {
    binlog = std::make_shared<td::Binlog>();
    queue = td::TQueue::create();
    td::TQueueBinlog<td::Binlog> helper;
    helper.set_binlog(binlog);
    uint64_t seen = 0;
    auto status = binlog->init(path, [&](const td::BinlogEvent &event) {
      auto replay_status = helper.replay(event, *queue);
      if (replay_status.is_error()) {
        std::printf("replay err=%s\n", replay_status.message().str().c_str());
      } else {
        seen++;
      }
    });
    if (status.is_error()) {
      std::printf("init err=%s\n", status.message().str().c_str());
      return false;
    }
    auto storage = td::make_unique<td::TQueueBinlog<td::Binlog>>();
    storage->set_binlog(binlog);
    queue->set_callback(std::move(storage));
    open = true;
    if (replayed != nullptr) {
      *replayed = seen;
    }
    return true;
  }

  bool close() {
    if (!open) {
      return true;
    }
    auto status = binlog->close();
    open = false;
    if (status.is_error()) {
      std::printf("close err=%s\n", status.message().str().c_str());
      return false;
    }
    return true;
  }
};

void print_events(td::MutableSpan<td::TQueue::Event> &span) {
  for (auto &event : span) {
    std::printf(" id=%d exp=%d extra=%lld data=%s vlen=%zu", event.id.value(), event.expires_at,
                static_cast<long long>(event.extra), hex_head(event.data).c_str(), event.data.size());
  }
}

int run_script(const char *script_path, const char *binlog_path) {
  std::ifstream in(script_path);
  if (!in) {
    std::fprintf(stderr, "cannot read %s\n", script_path);
    return 2;
  }
  std::vector<std::string> script_lines;
  {
    std::string raw;
    while (std::getline(in, raw)) {
      if (!raw.empty() && raw.back() == '\r') {
        raw.pop_back();
      }
      script_lines.push_back(raw);
    }
  }
  bool attach = false;
  for (auto &raw : script_lines) {
    auto tok = split(raw);
    if (tok.empty() || tok[0][0] == '#') {
      continue;
    }
    attach = tok[0] == "attach";
    break;
  }

  Env env;
  env.path = binlog_path;
  if (!attach) {
    td::Binlog::destroy(td::CSlice(binlog_path)).ignore();
  }
  uint64_t seen = 0;
  if (!env.init(&seen)) {
    return 3;
  }
  if (attach) {
    std::printf("attach frames=%llu\n", static_cast<unsigned long long>(seen));
  }

  std::vector<td::TQueue::Event> buffer(MAX_RESULT_EVENTS);
  for (auto &line : script_lines) {
    auto tok = split(line);
    if (tok.empty() || tok[0][0] == '#') {
      continue;
    }
    const std::string &cmd = tok[0];

    if (cmd == "push") {
      int64_t queue_id;
      std::string data;
      int32_t expires_at;
      int64_t extra;
      int32_t hint_value = 0;
      bool has_hint = tok.size() == 6 && tok[5] != "-";
      if (tok.size() != 6 || !parse_i64(tok[1], queue_id) || !from_hex(tok[2], data) ||
          !parse_i32(tok[3], expires_at) || !parse_i64(tok[4], extra) ||
          (has_hint && !parse_i32(tok[5], hint_value))) {
        std::printf("reject push argc=%zu\n", tok.size());
        continue;
      }
      EventId hint;
      if (has_hint) {
        auto r_hint = EventId::from_int32(hint_value);
        if (r_hint.is_error()) {
          std::printf("push err=%s\n", r_hint.error().message().str().c_str());
          continue;
        }
        hint = r_hint.move_as_ok();
      }
      auto result = env.queue->push(queue_id, data, expires_at, extra, hint);
      if (result.is_error()) {
        std::printf("push err=%s\n", result.error().message().str().c_str());
      } else {
        std::printf("push q=%lld id=%d data=%s vlen=%zu\n", static_cast<long long>(queue_id),
                    result.ok().value(), hex_head(data).c_str(), data.size());
      }
    } else if (cmd == "get") {
      int64_t queue_id;
      int32_t from_value = 0;
      int32_t forget;
      int32_t now;
      int32_t max_n;
      bool has_from = tok.size() == 6 && tok[2] != "-";
      if (tok.size() != 6 || !parse_i64(tok[1], queue_id) || (has_from && !parse_i32(tok[2], from_value)) ||
          !parse_i32(tok[3], forget) || !parse_i32(tok[4], now) || !parse_i32(tok[5], max_n) || max_n < 0) {
        std::printf("reject get argc=%zu\n", tok.size());
        continue;
      }
      EventId from_id;
      if (has_from) {
        auto r_from = EventId::from_int32(from_value);
        if (r_from.is_error()) {
          std::printf("get err=%s\n", r_from.error().message().str().c_str());
          continue;
        }
        from_id = r_from.move_as_ok();
      }
      td::MutableSpan<td::TQueue::Event> span(buffer.data(), static_cast<size_t>(max_n));
      auto result = env.queue->get(queue_id, from_id, forget != 0, now, span);
      if (result.is_error()) {
        std::printf("get err=%s\n", result.error().message().str().c_str());
      } else {
        std::printf("get q=%lld ret=%zu n=%zu", static_cast<long long>(queue_id), result.ok(), span.size());
        print_events(span);
        std::printf("\n");
      }
    } else if (cmd == "forget") {
      int64_t queue_id;
      int32_t id_value;
      if (tok.size() != 3 || !parse_i64(tok[1], queue_id) || !parse_i32(tok[2], id_value)) {
        std::printf("reject forget argc=%zu\n", tok.size());
        continue;
      }
      auto r_id = EventId::from_int32(id_value);
      if (r_id.is_error()) {
        std::printf("forget err=%s\n", r_id.error().message().str().c_str());
        continue;
      }
      env.queue->forget(queue_id, r_id.move_as_ok());
      std::printf("forget q=%lld id=%d\n", static_cast<long long>(queue_id), id_value);
    } else if (cmd == "clear") {
      int64_t queue_id;
      int64_t keep;
      if (tok.size() != 3 || !parse_i64(tok[1], queue_id) || !parse_i64(tok[2], keep) || keep < 0) {
        std::printf("reject clear argc=%zu\n", tok.size());
        continue;
      }
      auto deleted = env.queue->clear(queue_id, static_cast<size_t>(keep));
      std::printf("clear q=%lld keep=%lld returned=%zu del=", static_cast<long long>(queue_id),
                  static_cast<long long>(keep), deleted.size());
      bool first = true;
      for (auto &entry : deleted) {
        std::printf("%s%d", first ? "" : ",", entry.first.value());
        first = false;
      }
      if (first) {
        std::printf("-");
      }
      std::printf(" head=%d tail=%d size=%zu\n", env.queue->get_head(queue_id).value(),
                  env.queue->get_tail(queue_id).value(), env.queue->get_size(queue_id));
    } else if (cmd == "head" || cmd == "tail" || cmd == "size") {
      int64_t queue_id;
      if (tok.size() != 2 || !parse_i64(tok[1], queue_id)) {
        std::printf("reject %s argc=%zu\n", cmd.c_str(), tok.size());
        continue;
      }
      if (cmd == "head") {
        std::printf("head q=%lld id=%d\n", static_cast<long long>(queue_id), env.queue->get_head(queue_id).value());
      } else if (cmd == "tail") {
        std::printf("tail q=%lld id=%d\n", static_cast<long long>(queue_id), env.queue->get_tail(queue_id).value());
      } else {
        std::printf("size q=%lld n=%zu\n", static_cast<long long>(queue_id), env.queue->get_size(queue_id));
      }
    } else if (cmd == "gc") {
      int32_t now;
      if (tok.size() != 2 || !parse_i32(tok[1], now)) {
        std::printf("reject gc argc=%zu\n", tok.size());
        continue;
      }
      auto result = env.queue->run_gc(now);
      std::printf("gc now=%d deleted=%lld completed=%d\n", now, static_cast<long long>(result.first),
                  static_cast<int>(result.second));
    } else if (cmd == "close") {
      bool ok = env.close();
      std::printf("close ok=%d\n", static_cast<int>(ok));
    } else if (cmd == "dumpfile") {
      if (env.open) {
        std::printf("reject dumpfile reason=open\n");
        continue;
      }
      std::ifstream file(binlog_path, std::ios::binary);
      if (!file) {
        std::printf("reject dumpfile reason=missing\n");
        continue;
      }
      std::string bytes((std::istreambuf_iterator<char>(file)), std::istreambuf_iterator<char>());
      auto digest = td::sha256(bytes);
      std::printf("dumpfile size=%zu sha256=%s\n", bytes.size(), hex_all(digest).c_str());
    } else if (cmd == "replay") {
      if (env.open) {
        std::printf("reject replay reason=open\n");
        continue;
      }
      if (tok.size() < 2) {
        std::printf("reject replay argc=%zu\n", tok.size());
        continue;
      }
      auto fresh = td::TQueue::create();
      auto binlog = std::make_shared<td::Binlog>();
      td::TQueueBinlog<td::Binlog> helper;
      helper.set_binlog(binlog);
      uint64_t seen = 0;
      auto status = binlog->init(binlog_path, [&](const td::BinlogEvent &event) {
        auto replay_status = helper.replay(event, *fresh);
        if (replay_status.is_error()) {
          std::printf("replay err=%s\n", replay_status.message().str().c_str());
        } else {
          seen++;
        }
      });
      if (status.is_error()) {
        std::printf("replay err=%s\n", status.message().str().c_str());
        continue;
      }
      for (size_t i = 1; i < tok.size(); i++) {
        int64_t queue_id = 0;
        if (!parse_i64(tok[i], queue_id)) {
          std::printf("reject replay queue=%s\n", tok[i].c_str());
          continue;
        }
        std::printf("replay q=%lld head=%d tail=%d size=%zu\n", static_cast<long long>(queue_id),
                    fresh->get_head(queue_id).value(), fresh->get_tail(queue_id).value(),
                    fresh->get_size(queue_id));
      }
      std::printf("replay frames=%llu\n", static_cast<unsigned long long>(seen));
      binlog->close().ignore();
    } else if (cmd == "attach") {
      // handled in the prologue, before the binlog was opened
      continue;
    } else {
      std::printf("reject unknown=%s\n", cmd.c_str());
    }
  }
  if (env.open) {
    env.close();
  }
  return 0;
}

}  // namespace

int main(int argc, char **argv) {
  if (argc != 3) {
    std::fprintf(stderr, "usage: tqueue_binlog_probe <script.txt> <binlog_path>\n");
    return 2;
  }
  return run_script(argv[1], argv[2]);
}