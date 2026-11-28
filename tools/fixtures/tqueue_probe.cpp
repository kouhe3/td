// Oracle for td::TQueue (tddb/td/db/TQueue.h) driven by a script.
//
//   tqueue_probe <script.txt>
//   tqueue_probe --check <case>
//
// The queue is `TQueue::create()` (i.e. the real `TQueueImpl`) with a
// `TQueueMemoryStorage` as its `StorageCallback`, so no actor, no binlog and no
// network are involved. `replay` builds a *second* queue and pushes every stored
// event through it with `TQueueMemoryStorage::replay`, which is the same path
// TDLib uses at open time.
//
// Script lines (blank lines and '#' comments ignored):
//   push <queue> <hex data> <expires_at> <extra> <hint|->
//   get <queue> <from|-> <forget 0|1> <now> <max>
//   forget <queue> <id>
//   clear <queue> <keep_count>
//   head <queue> | tail <queue> | size <queue>
//   gc <now>
//   replay <queue>...
//   id <int32> | idnext <int32> | idadv <int32> <offset> | idprint <int32>
//
// `-` is the empty byte string (an empty hint, or an empty `from_id`). Values
// print as hex truncated to 32 bytes with `vlen` carrying the real length.
//
// Measured behaviour this probe encodes (docs/facts.md F199/F200):
//   * `get_size` never counts the **last** event when its data is empty
//     (`q.events.size() - (rbegin()->second.data.empty() ? 1 : 0)`), so a
//     "reserved slot" event does not count; `get` returns that same size, not
//     the number of events it handed back.
//   * `clear(q, 0)` returns only `size - 1` deleted events: the last one keeps
//     its slot and its `log_event_id`, its data is cleared and the storage is
//     told to rewrite it (`callback_->push`).
//   * `pop` on the last event (whose `next()` is `tail_id`) also rewrites rather
//     than removes, so an emptied slot survives until the queue is swept.
//   * `push` only takes `hint_new_id` when the tail is still empty; afterwards
//     the tail is the authority. Without a hint the first tail is
//     `Random::fast(2000001, MAX_ID / 2)`, which is why the corpus always gives
//     one.
//   * `from_id > tail + 10` is "Specified from_id is in the future" and
//     `from_id < head - MAX_QUEUE_EVENTS` is "Specified from_id is in the past".
//   * `EventId::from_int32(0)` succeeds -- 0 is a valid id but `empty()` -- while
//     `MAX_ID = 2000000000` and above are rejected with "Invalid ID".
//
// Build from the repo root against a configured C:/tmp/td-build:
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I tddb -I tdutils -I tdactor
//   -I C:/tmp/td-build/tdutils tools/fixtures/tqueue_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdutils/libtdutils.a
//   C:/tmp/td-build/tdactor/libtdactor.a -Wl,--end-group
//   -lssl -lcrypto -lz -lws2_32 -lpsapi -o C:/tmp/td-probe/tqueue_probe.exe
//
// Needs C:/msys64/ucrt64/bin on PATH at run time (libstdc++-6.dll, libwinpthread-1.dll).

#include "td/db/TQueue.h"

#include "td/utils/Span.h"
#include "td/utils/StringBuilder.h"

#include <cstdio>
#include <cstdlib>
#include <cerrno>
#include <cstring>
#include <fstream>
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

std::string id_text(EventId id) {
  td::StringBuilder sb;
  sb << id;
  return sb.as_cslice().str();
}

void print_id_result(const char *tag, td::Result<EventId> result) {
  if (result.is_error()) {
    std::printf("%s ok=0 err=%s\n", tag, result.error().message().str().c_str());
    return;
  }
  auto id = result.move_as_ok();
  std::printf("%s ok=1 value=%d valid=%d empty=%d\n", tag, id.value(), static_cast<int>(id.is_valid()),
              static_cast<int>(id.empty()));
}

struct Env {
  td::unique_ptr<td::TQueue> queue;
  td::TQueueMemoryStorage *storage = nullptr;

  bool init() {
    auto memory = td::make_unique<td::TQueueMemoryStorage>();
    storage = memory.get();
    queue = td::TQueue::create();
    queue->set_callback(std::move(memory));
    return true;
  }
};

void print_events(td::MutableSpan<td::TQueue::Event> &span) {
  for (auto &event : span) {
    std::printf(" id=%d exp=%d extra=%lld data=%s vlen=%zu", event.id.value(), event.expires_at,
                static_cast<long long>(event.extra), hex_head(event.data).c_str(), event.data.size());
  }
}

int run_script(const char *script_path) {
  std::ifstream in(script_path);
  if (!in) {
    std::fprintf(stderr, "cannot read %s\n", script_path);
    return 2;
  }
  Env env;
  env.init();

  std::vector<td::TQueue::Event> buffer(MAX_RESULT_EVENTS);
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
    } else if (cmd == "replay") {
      if (tok.size() < 2) {
        std::printf("reject replay argc=%zu\n", tok.size());
        continue;
      }
      auto fresh = td::TQueue::create();
      env.storage->replay(*fresh);
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
    } else if (cmd == "id") {
      int32_t value;
      if (tok.size() != 2 || !parse_i32(tok[1], value)) {
        std::printf("reject id argc=%zu\n", tok.size());
        continue;
      }
      auto result = EventId::from_int32(value);
      if (result.is_error()) {
        std::printf("id n=%d ok=0 err=%s\n", value, result.error().message().str().c_str());
      } else {
        auto id = result.move_as_ok();
        std::printf("id n=%d ok=1 value=%d valid=%d empty=%d\n", value, id.value(), static_cast<int>(id.is_valid()),
                    static_cast<int>(id.empty()));
      }
    } else if (cmd == "idnext") {
      int32_t value;
      if (tok.size() != 2 || !parse_i32(tok[1], value)) {
        std::printf("reject idnext argc=%zu\n", tok.size());
        continue;
      }
      auto base = EventId::from_int32(value);
      if (base.is_error()) {
        std::printf("idnext n=%d ok=0 err=%s\n", value, base.error().message().str().c_str());
        continue;
      }
      auto result = base.ok().next();
      if (result.is_error()) {
        std::printf("idnext n=%d ok=0 err=%s\n", value, result.error().message().str().c_str());
      } else {
        std::printf("idnext n=%d ok=1 value=%d\n", value, result.ok().value());
      }
    } else if (cmd == "idadv") {
      int32_t value;
      int64_t offset;
      if (tok.size() != 3 || !parse_i32(tok[1], value) || !parse_i64(tok[2], offset) || offset < 0) {
        std::printf("reject idadv argc=%zu\n", tok.size());
        continue;
      }
      auto base = EventId::from_int32(value);
      if (base.is_error()) {
        std::printf("idadv n=%d off=%lld ok=0 err=%s\n", value, static_cast<long long>(offset),
                    base.error().message().str().c_str());
        continue;
      }
      auto result = base.ok().advance(static_cast<size_t>(offset));
      if (result.is_error()) {
        std::printf("idadv n=%d off=%lld ok=0 err=%s\n", value, static_cast<long long>(offset),
                    result.error().message().str().c_str());
      } else {
        std::printf("idadv n=%d off=%lld ok=1 value=%d\n", value, static_cast<long long>(offset),
                    result.ok().value());
      }
    } else if (cmd == "idprint") {
      int32_t value;
      if (tok.size() != 2 || !parse_i32(tok[1], value)) {
        std::printf("reject idprint argc=%zu\n", tok.size());
        continue;
      }
      auto base = EventId::from_int32(value);
      if (base.is_error()) {
        std::printf("idprint n=%d ok=0 err=%s\n", value, base.error().message().str().c_str());
      } else {
        std::printf("idprint n=%d text=%s\n", value, id_text(base.ok()).c_str());
      }
    } else {
      std::printf("reject unknown=%s\n", cmd.c_str());
    }
  }
  return 0;
}

int run_check(const char *case_name) {
  auto queue = td::TQueue::create();
  auto memory = td::make_unique<td::TQueueMemoryStorage>();
  queue->set_callback(std::move(memory));

  td::TQueue::RawEvent raw;
  raw.expires_at = 1000;
  raw.data = "x";
  raw.extra = 0;
  if (std::strcmp(case_name, "invalid_event_id") == 0) {
    raw.event_id = EventId();
    bool added = queue->do_push(1, std::move(raw));
    std::printf("after added=%d\n", static_cast<int>(added));
  } else if (std::strcmp(case_name, "valid_event_id") == 0) {
    raw.event_id = EventId::from_int32(5).move_as_ok();
    bool added = queue->do_push(1, std::move(raw));
    std::printf("after added=%d size=%zu head=%d\n", static_cast<int>(added), queue->get_size(1),
                queue->get_head(1).value());
  } else {
    std::fprintf(stderr, "unknown check case %s\n", case_name);
    return 2;
  }
  return 0;
}

}  // namespace

int main(int argc, char **argv) {
  if (argc == 3 && std::strcmp(argv[1], "--check") == 0) {
    return run_check(argv[2]);
  }
  if (argc != 2) {
    std::fprintf(stderr, "usage: tqueue_probe <script.txt>\n       tqueue_probe --check <case>\n");
    return 2;
  }
  return run_script(argv[1]);
}