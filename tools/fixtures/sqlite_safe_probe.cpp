// Oracle for td::SqliteConnectionSafe + td::SqliteKeyValueSafe
// (tddb/td/db/SqliteConnectionSafe.{h,cpp}, SqliteKeyValueSafe.h).
//
//   sqlite_safe_probe <script.txt> <workdir>
//
// Both classes use `SchedulerLocalStorage`, so `Scheduler::instance()` must be
// non-null while they are constructed: the probe runs the whole script inside
// an actor on `ConcurrentScheduler(0)` (docs/facts.md F216). The database is
// created first (`SqliteDb::open_with_key(path, true, key)`) because
// `SqliteConnectionSafe` opens with `allow_creation=false`.
//
// Script lines (blank lines and '#' comments ignored):
//   prepare        recreate the work dir and an empty database
//   prepare_empty  recreate the work dir without a database
//   conn           construct SqliteConnectionSafe (must not open anything yet)
//   get            connection->get()
//   kv             construct SqliteKeyValueSafe
//   set <k> <v>    kv.set
//   sget <k>       kv.get
//   close          connection->close()
//   destroy        connection->close_and_destroy()
//   drop           release the safe objects
//   exists         print whether the database file exists
//
// Two script shapes are expected to die (they are the abort pair): `prepare_empty,
// conn, get` (lazy open of a missing database) and `prepare, conn, destroy, get`
// (re-open after destroy). Both hit `LOG(FATAL)` and neither prints `done`.
//
// The narration prints `exists=` after each step, so "lazy open", "close keeps
// the file" and "destroy removes it" are judged as observable states, not by
// reading private fields.
//
// Build (`SqliteConnectionSafe.cpp` lives in libtddb.a):
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I tdactor -I tddb -I C:/tmp/td-build/tdutils tools/fixtures/sqlite_safe_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdutils/libtdutils.a
//   C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/sqlite/libtdsqlite.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi
//   -o C:/tmp/td-probe/sqlite_safe_probe.exe

#include "td/actor/ConcurrentScheduler.h"
#include "td/actor/actor.h"
#include "td/db/DbKey.h"
#include "td/db/SqliteConnectionSafe.h"
#include "td/db/SqliteDb.h"
#include "td/db/SqliteKeyValue.h"
#include "td/db/SqliteKeyValueSafe.h"
#include "td/utils/Slice.h"
#include "td/utils/misc.h"
#include "td/utils/port/path.h"
#include "td/utils/port/Stat.h"

#include <atomic>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <string>
#include <vector>

namespace {

std::atomic<bool> g_done{false};
std::string g_script_path;
std::string g_work_dir;

std::string read_all(const char *path) {
  std::ifstream in(path, std::ios::binary);
  if (!in) {
    std::fprintf(stderr, "cannot open %s\n", path);
    std::exit(2);
  }
  return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

bool file_exists(const std::string &path) {
  auto r_stat = td::stat(path);
  return r_stat.is_ok();
}

void print_exists(const char *tag) {
  std::printf("%s exists=%d\n", tag, static_cast<int>(file_exists(g_work_dir + "/kv.db")));
}

class Probe final : public td::Actor {
 public:
  void start_up() final {
    auto script = read_all(g_script_path.c_str());
    td::Slice rest(script);
    while (!rest.empty()) {
      auto pos = rest.find('\n');
      td::Slice line = pos == td::Slice::npos ? rest : rest.substr(0, pos);
      rest = pos == td::Slice::npos ? td::Slice() : rest.substr(pos + 1);
      while (!line.empty() && (line.back() == '\r' || line.back() == ' ')) {
        line.remove_suffix(1);
      }
      if (line.empty() || line[0] == '#') {
        continue;
      }
      auto tokens = td::full_split(line, ' ');
      auto command = tokens[0];
      if (command == "prepare" || command == "prepare_empty") {
        td::rmrf(g_work_dir).ignore();
        td::mkdir(g_work_dir).ensure();
        if (command == "prepare") {
          td::SqliteDb::open_with_key(g_work_dir + "/kv.db", true, td::DbKey::empty()).ensure();
        }
        print_exists(command.str().c_str());
      } else if (command == "conn") {
        connection_ = std::make_shared<td::SqliteConnectionSafe>(g_work_dir + "/kv.db", td::DbKey::empty());
        print_exists("conn");
      } else if (command == "get") {
        connection_->get();
        print_exists("get");
      } else if (command == "kv") {
        kv_safe_ = std::make_shared<td::SqliteKeyValueSafe>("kv", connection_);
        print_exists("kv");
      } else if (command == "set") {
        kv_safe_->get().set(tokens[1].str(), tokens.size() > 2 ? tokens[2].str() : "");
        std::printf("set %s %s\n", tokens[1].str().c_str(), tokens.size() > 2 ? tokens[2].str().c_str() : "");
      } else if (command == "sget") {
        auto value = kv_safe_->get().get(tokens[1].str());
        std::printf("sget %s=%s\n", tokens[1].str().c_str(), value.c_str());
      } else if (command == "close") {
        connection_->close();
        print_exists("close");
      } else if (command == "destroy") {
        connection_->close_and_destroy();
        print_exists("destroy");
      } else if (command == "drop") {
        kv_safe_.reset();
        connection_.reset();
        print_exists("drop");
      } else if (command == "exists") {
        print_exists("exists");
      } else {
        std::fprintf(stderr, "unknown command %s\n", command.str().c_str());
        std::exit(2);
      }
    }
    std::printf("done\n");
    g_done = true;
  }

 private:
  std::shared_ptr<td::SqliteConnectionSafe> connection_;
  std::shared_ptr<td::SqliteKeyValueSafe> kv_safe_;
};

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc != 3) {
    std::fprintf(stderr, "usage: sqlite_safe_probe <script.txt> <workdir>\n");
    return 2;
  }
  g_script_path = argv[1];
  g_work_dir = argv[2];

  td::ConcurrentScheduler scheduler(0, 0);
  scheduler.start();
  scheduler.create_actor_unsafe<Probe>(0, "Probe").release();
  while (!g_done && scheduler.run_main(0.1)) {
  }
  scheduler.finish();
  return 0;
}