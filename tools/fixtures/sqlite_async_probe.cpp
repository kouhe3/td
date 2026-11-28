// Oracle for td::SqliteKeyValueAsync (tddb/td/db/SqliteKeyValueAsync.cpp),
// driven one operation at a time: the probe sends an op, waits for its promise,
// prints the result, then sends the next one (facts F216/F229).
//
//   sqlite_async_probe <script.txt> <workdir>
//
// The implementation buffers `set`/`erase` and flushes them on a 10 ms timer or
// on a forced flush (`set_all` / `erase_by_prefix` / `close`), so with one op in
// flight the promise resolves after that flush -- which is exactly why the
// driver waits for the promise instead of pumping the scheduler blindly.
//
// Script:
//   prepare            recreate the work dir and database, open connection + kv
//   aset <k> <v>       async set
//   aget <k>           async get
//   aerase <k>         async erase
//   aprefix <p>        async erase_by_prefix (force flush first)
//   asetall <k> <v>    async set_all (single pair; multi-pair order is not judged)
//   aclose             async close (force flush)
//   sget <k>           sync read through the kv-safe wrapper (after the async ops)
//
// Build (`SqliteKeyValueAsync.cpp` is in libtddb.a):
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I tdactor -I tddb -I C:/tmp/td-build/tdutils tools/fixtures/sqlite_async_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdutils/libtdutils.a
//   C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/sqlite/libtdsqlite.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi
//   -o C:/tmp/td-probe/sqlite_async_probe.exe

#include "td/actor/ConcurrentScheduler.h"
#include "td/actor/actor.h"
#include "td/db/DbKey.h"
#include "td/db/SqliteConnectionSafe.h"
#include "td/db/SqliteDb.h"
#include "td/db/SqliteKeyValue.h"
#include "td/db/SqliteKeyValueAsync.h"
#include "td/db/SqliteKeyValueSafe.h"
#include "td/utils/Slice.h"
#include "td/utils/misc.h"
#include "td/utils/port/path.h"

#include <atomic>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <memory>
#include <string>
#include <vector>

namespace {

std::atomic<bool> g_done{false};

struct Op {
  std::string kind;
  std::string a;
  std::string b;
};

std::string read_all(const char *path) {
  std::ifstream in(path, std::ios::binary);
  if (!in) {
    std::fprintf(stderr, "cannot open %s\n", path);
    std::exit(2);
  }
  return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

class Probe final : public td::Actor {
 public:
  Probe(std::string work_dir, std::vector<Op> ops) : work_dir_(std::move(work_dir)), ops_(std::move(ops)) {
  }

  void start_up() final {
    advance();
  }

 private:
  std::string work_dir_;
  std::vector<Op> ops_;
  size_t next_ = 0;
  std::shared_ptr<td::SqliteConnectionSafe> connection_;
  std::shared_ptr<td::SqliteKeyValueSafe> kv_safe_;
  td::unique_ptr<td::SqliteKeyValueAsyncInterface> async_;

  void finish_all() {
    std::printf("done\n");
    g_done = true;
  }

  void unit_done(const std::string &line) {
    std::printf("%s -> ok\n", line.c_str());
    advance();
  }

  void advance() {
    while (next_ < ops_.size()) {
      const auto &op = ops_[next_++];
      if (op.kind == "prepare") {
        td::rmrf(work_dir_).ignore();
        td::mkdir(work_dir_).ensure();
        td::SqliteDb::open_with_key(work_dir_ + "/kv.db", true, td::DbKey::empty()).ensure();
        connection_ = std::make_shared<td::SqliteConnectionSafe>(work_dir_ + "/kv.db", td::DbKey::empty());
        kv_safe_ = std::make_shared<td::SqliteKeyValueSafe>("kv", connection_);
        async_ = td::create_sqlite_key_value_async(kv_safe_, 0);
        std::printf("prepare -> ok\n");
        continue;
      }
      if (op.kind == "sget") {
        auto value = kv_safe_->get().get(op.a);
        std::printf("sget %s -> %s\n", op.a.c_str(), value.c_str());
        continue;
      }
      if (op.kind == "aset") {
        auto line = "aset " + op.a + " " + op.b;
        async_->set(op.a, op.b, td::PromiseCreator::lambda([this, line](td::Result<td::Unit> result) {
                       if (!result.is_ok()) {
                         std::printf("%s -> error\n", line.c_str());
                         advance();
                         return;
                       }
                       unit_done(line);
                     }));
        return;
      }
      if (op.kind == "aget") {
        auto line = "aget " + op.a;
        async_->get(op.a, td::PromiseCreator::lambda([this, line](td::Result<td::string> result) {
                       std::printf("%s -> %s\n", line.c_str(), result.is_ok() ? result.ok().c_str() : "error");
                       advance();
                     }));
        return;
      }
      if (op.kind == "aerase") {
        auto line = "aerase " + op.a;
        async_->erase(op.a, td::PromiseCreator::lambda([this, line](td::Result<td::Unit> result) {
                        if (!result.is_ok()) {
                          std::printf("%s -> error\n", line.c_str());
                          advance();
                          return;
                        }
                        unit_done(line);
                      }));
        return;
      }
      if (op.kind == "aprefix") {
        auto line = "aprefix " + op.a;
        async_->erase_by_prefix(op.a, td::PromiseCreator::lambda([this, line](td::Result<td::Unit> result) {
                                   if (!result.is_ok()) {
                                     std::printf("%s -> error\n", line.c_str());
                                     advance();
                                     return;
                                   }
                                   unit_done(line);
                                 }));
        return;
      }
      if (op.kind == "asetall") {
        td::FlatHashMap<td::string, td::string> values;
        values.emplace(op.a, op.b);
        auto line = "asetall " + op.a + " " + op.b;
        async_->set_all(std::move(values), td::PromiseCreator::lambda([this, line](td::Result<td::Unit> result) {
                                           if (!result.is_ok()) {
                                             std::printf("%s -> error\n", line.c_str());
                                             advance();
                                             return;
                                           }
                                           unit_done(line);
                                         }));
        return;
      }
      if (op.kind == "aclose") {
        async_->close(td::PromiseCreator::lambda([this](td::Result<td::Unit> result) {
          std::printf("aclose -> %s\n", result.is_ok() ? "ok" : "error");
          advance();
        }));
        return;
      }
      std::fprintf(stderr, "unknown op %s\n", op.kind.c_str());
      std::exit(2);
    }
    finish_all();
  }
};

std::vector<Op> parse_ops(const std::string &script, const std::string &work_dir) {
  (void)work_dir;
  std::vector<Op> ops;
  td::Slice rest(script);
  while (!rest.empty()) {
    auto pos = rest.find('\n');
    td::Slice line = pos == td::Slice::npos ? rest : rest.substr(0, pos);
    rest = pos == td::Slice::npos ? td::Slice() : rest.substr(pos + 1);
    if (!line.empty() && line.back() == '\r') {
      line.remove_suffix(1);
    }
    if (line.empty() || line[0] == '#') {
      continue;
    }
    auto parts = td::full_split(line, ' ', 3);
    Op op;
    op.kind = parts[0].str();
    if (parts.size() > 1) {
      op.a = parts[1].str();
    }
    if (parts.size() > 2) {
      op.b = parts[2].str();
    }
    if (op.a == "-") {
      op.a.clear();
    }
    ops.push_back(std::move(op));
  }
  return ops;
}

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc != 3) {
    std::fprintf(stderr, "usage: sqlite_async_probe <script.txt> <workdir>\n");
    return 2;
  }
  auto script = read_all(argv[1]);
  auto ops = parse_ops(script, argv[2]);

  td::ConcurrentScheduler scheduler(0, 0);
  scheduler.start();
  scheduler.create_actor_unsafe<Probe>(0, "Probe", std::string(argv[2]), std::move(ops)).release();
  while (!g_done && scheduler.run_main(0.1)) {
  }
  scheduler.finish();
  return 0;
}