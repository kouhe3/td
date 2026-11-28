# SPEC（as-built）— `tddb` 存储面（binlog + crypto + KV(binlog/sqlite) + TQueue + TdDb 路径）

> **版本**：基座 commit `1b9efe84d`（master，判定基座）× oracle `libtdjson.dll` md5 `0398814038978aaeef7f8d6ac62bb8ff`。文件名里的 hash 就是版本：基座或 oracle 漂移 ⇒ 按新 hash 另起 `docs/SPEC-<新hash>-<area>.md`，本文件置「过期」，不原地改数字。
>
> **as-built 规格，不是 to-build。** 它记录「已判过、数字可复现」的现状，供接手方当底图。
> 台账行为证据来源：`docs/facts.md` F185–F247（含 `td/telegram/SecureStorage` 的 F244–F247）。

## 1. 定义域

| 项 | 值 |
|---|---|
| C++ 源 | `tddb/td/db/binlog/{Binlog,BinlogEvent,BinlogInterface}.{h,cpp}`、`tddb/td/db/{BinlogKeyValue.h,SeqKeyValue.h,DbKey.h,KeyValueSyncInterface.h}`、`td/telegram/TdDb.{h,cpp}`、`tdutils/td/utils/crypto.{h,cpp}`、`tdutils/td/utils/port/path.cpp`（`mkpath`/`mkdir`/`realpath`） |
| C++ 源（SQLite） | `tddb/td/db/{SqliteKeyValue,SqliteDb,SqliteStatement}.{h,cpp}` + `tddb/td/db/detail/RawSqliteDb.{h,cpp}`，跑在仓库内置的 **SQLCipher 3.31.0** 上（`sqlite/`，构建产物 `C:/tmp/td-build/sqlite/libtdsqlite.a`） |
| C++ 源（TQueue） | `tddb/td/db/TQueue.{h,cpp}`（`TQueueImpl` + `TQueueMemoryStorage` + `TQueueBinlog<Binlog>` 模板的显式实例化；只有 `close(Promise)` 不在范围内） |
| C++ 源（连接安全层） | `tddb/td/db/{SqliteConnectionSafe.{h,cpp},SqliteKeyValueSafe.h}`：`LazySchedulerLocalStorage` 上的懒连接、`close_state_` 计数、`close_and_destroy`；驱动需要 scheduler actor（F216） |
| C++ 源（异步层） | `tddb/td/db/SqliteKeyValueAsync.cpp`：actor 内的 buffer/flush、promise 合批、`erase_by_prefix`/`set_all`/`close` 的强制 flush |
| C++ 源（静态加密，唯一不在 `tddb` 下的一条） | `td/telegram/SecureStorage.{h,cpp}`（195 + 408 = 603 行）：`ValueHash`、两条 `AesCbcState` 派生、`Secret`、`gen_random_prefix`、两个流状态机、`encrypt_value`/`decrypt_value`；判据只走内存路径 |
| 规模 | 被判 11 个文件 / 2,728 行（`tddb` 全目录 4,792 行）；SQLite 面另加 4 个文件 / 605 行（`SqliteKeyValue` 130 + `SqliteDb` 323 + `SqliteStatement` 205 + `RawSqliteDb` 减少量不计） |
| oracle | `libtdjson.dll` + `C:/tmp/td-build/tddb/{libtddb.a,binlog_dump.exe}` + `libtdcore.a`（`TdDb` 那条） |
| 台账 | F185–F247；`tasks/plan.md` T92–T117 |

## 2. 可复现现状

```bash
cd C:/projects/td
uv run python tools/binlog_diff.py --all --selftest        # 读侧：14 个 fixture，diff_lines=0
uv run python tools/binlog_write_diff.py --selftest        # 写侧：27 case（20 明文 + 7 加密），1.9 s
uv run python tools/binlog_write_diff.py --encrypted       # 只跑加密半边，0.9 s
uv run python tools/crypto_diff.py --selftest              # 457 行，6.7 s
uv run python tools/tddb_paths_diff.py --selftest          # 35 行 + 目录树，0.2 s
uv run python tools/sqlite_kv_diff.py --selftest           # SQLite KV：11 case + 2 check，1.4 s
uv run python tools/tqueue_diff.py --selftest              # TQueue：12 case + 2 check，0.6 s
uv run python tools/tqueue_binlog_diff.py --selftest       # TQueueBinlog：10 case，4,016 B，1.2 s
uv run python tools/sqlite_safe_diff.py --selftest         # 连接安全层：8 case（6 + 2 abort）/ 122 行，1.9 s
uv run python tools/sqlite_async_diff.py --selftest        # 异步层串行驱动：6 case（4 + 2 abort）/ 70 行，1.8 s
```

## 3. 判据

| 面 | 判据形态 | 规模（实测） | fact |
|---|---|---|---|
| binlog 读侧 | 逐行（`binlog_dump` vs `binlog_head`） | 14 fixture，`diff_lines=0` | 早期 round |
| binlog 写侧·明文 | **文件逐字节** | 20 case / 345,272 B，`md5=339d6923`，1.9 s | F185/F186 |
| binlog 写侧·加密 | **结构 + 双向交叉读**（C++ `binlog_dump` 读 Rust 文件、Rust `binlog_head` 读 C++ 文件）+ Python 重算 `key`/`key_hash` | 7 case / 201,168 B，0.9 s | F189/F190 |
| crypto 原语 | 逐字节 + 独立 `hashlib`/`hmac`/`zlib` 反算 + RFC 7914 / RFC 4231 / NIST F.5.5 | 457 行，`md5=4d39370b`，6.7 s | F187/F188 |
| KV 层（`BinlogKeyValue` + `SeqKeyValue`） | 明文 binlog 逐字节 + narration + 双向交叉读（四条全跑不短路） | 21 case / 884,208 B（含 4 份副本），`md5=d8c284e1`，1.84 s | F191/F192 |
| 静态加密面（`td/telegram/SecureStorage`） | narration 逐行 + **双向交叉读** + 篡改后的 `Hash mismatch` | 5 case / 60 行语料 / 94 行输出，`PASS cases=5`，0.72 s | F244–F247 |
| `TdDb` 路径与 key | stdout 逐字节 + **目录树快照** | 35 行，`md5=ad8d60ca`，0.2 s | F193/F194 |
| SQLite KV（`SqliteKeyValue` + `SqliteDb` 的事务计数 + `SqliteStatement` 的 blob 绑定/视图） | narration 逐行（**行序本身是判据**）+ **双向交叉读**（C++ 读 Rust 库、Rust 读 C++ 库）+ abort 对（`--check`，只比 rc 类别） | 11 case + 2 check 例 / 376,832 B，`md5=715cc4cb`，8.3 s | F196–F198 |
| TQueue（`TQueueImpl` + `TQueueMemoryStorage` + `EventId`） | narration 逐行 + FLOOR + abort 对（`do_push` 的空 `EventId`）+ **`replay` 重建**（storage 重放到新队列，head/tail/size 必须一致） | 12 case + 2 check 例 / 语料 109 行 / 输出 198 行，`md5=87e3f3e6`，7.0 s | F199–F201 |
| TQueueBinlog（`push`→`add`/`rewrite`、`pop`→`erase`、`pop_batch`→`erase_batch`、`replay`） | **binlog 文件逐字节** + narration + **双向交叉 replay** + FLOOR；selftest 用**独立的 Python 帧解析**反算 type/flags/size/body（不复用任何一侧的 walker） | 10 case / 4,016 B，`md5=15f996ad`，1.2 s | F202–F204 |
| `SqliteConnectionSafe` / `SqliteKeyValueSafe` | narration 逐行 + **文件存在性** + abort（致命文案里的 `state N` token）+ FLOOR | 8 case（6 + 2 abort）/ 122 行，1.9 s | F217–F219 |
| `SqliteKeyValueAsync` | narration 逐行（含 promise 载荷）+ FLOOR + 两条 abort（空 key）+ 手算 cell | 6 case（4 + 2 abort）/ 70 行，1.8 s | F229–F231 |

判据事实里最容易被接手方踩到的几条（全部有复现命令）：

- 帧 `[size:u32][id:u64][type:i32][flags:i32][extra:u64][data][crc32:u32]` 小端，`size = 32 + data`，crc32 覆盖前 `size-4` 字节；`size % 4 != 0` 是 `LOG(FATAL)`。
- **文件不是 append-only**：>50 KiB 且死事件 5:1 时自动 reindex（实测 4104 B / 114 帧）。
- id 计数器不落盘：reopen 按 replay 重算，只数追加事件（实测 5 → 3）。
- 加密 header 帧 128 B 明文（`type=-3`，`[flags][TL 32B salt][TL 16B iv][TL 32B hash]`），其后 AES-256-CTR 且 keystream **跨帧、跨 flush** 连续。
- 随机性只局部化到 **33 字节共享前缀**（28 字节帧头 + 清零 flags + salt 长度字节）；两次运行此后每个字节都不同。
- `pbkdf2_sha256` 把 dkLen CHECK 成 32、`iterations > 0`（0 次是 `0xC0000409`）；`AesCtrState` 只收 32B key + 16B iv。
- `mkpath` **只按 `'\\'` 切分**；`mkdir` 把 `ERROR_ALREADY_EXISTS`(183) 当成功，所以已存在的普通文件能当目录通过。
- `get_binlog_path`/`get_sqlite_path` 在匿名命名空间里，**没有 oracle 能调用**，只能靠仪器 selftest 按源码断言。
- KV 层：**帧 id ≠ 序号**（`add_event` 丢掉 `seq_no`，写 `create_raw(event_id, …)`）；`Rewrite` 的目标是**该 key 上一次事件的 id**；`erase` 写 `type=-2` 的 32 字节空帧，而 `set key ""` 写 magic 帧 + 两个 TL 字符串——`get` 对三者都返回 `""`，只有 `isset` 和帧类型能分。
- KV 层：`init` **不过滤 magic**（外来类型只要是 KV 形状就变活条目，之后的 `set` 会 rewrite 那个外来事件的 id）；真正的闸门是 TdDb 的 `switch` 与 `default: LOG(FATAL)`。`magic` 是 **per-init** 的（`init` 先 `close()` → 重置为默认）。
- KV 层：打开的 `Binlog` 持有整文件写锁，Windows 上第二个句柄读到 0 字节；`sync` 是唯一无观测副作用的 flush。`erase_by_prefix` 按 `WaitFreeHashMap` 遍历序写帧——**多匹配的删除顺序不可复现**，语料限制成每次最多一个匹配（覆盖边界）。
- `TdDb::check_parameters` 是 **private**（oracle 用 `#define private public`）；`mkpath` 只按 `\\` 切分，所以 `/` 路径整段只发一次 `CreateDirectoryW` 并失败；`realpath` 是纯词法 `GetFullPathNameW`。
- SQLite KV：**同一个 KV 有两套行序**。`get_all()` 与 `get_by_prefix("")` 都跑 `SELECT k, v FROM kv`（全表扫，**rowid 序**），而 `get_by_range` / `get_by_prefix(p)` 跑 `?1 <= k AND k < ?2`（走 `k BLOB PRIMARY KEY` 索引，**key 序**）。`set` 是 `REPLACE INTO`，即删旧的再插新的，所以**改写一个 key 会把它移到 rowid 序的末尾**：`set a; set b; set a` 之后 `getall` 给 `b, a`，`range a z` 给 `a, b`。把行序当噪声、只比集合，就把这条 lane 里最容易错的实现细节放走了（F198 的变异 ② 正是打这里）。
- SQLite KV：`get_by_range("", till)` **忽略 `till`**（空的 `from` 直接选全表语句）；`get_by_range(from, "")` 走 `?1 <= k` 的 rare 形式。`get_by_prefix` 会把前缀从返回的 key 上**剥掉**，`get_by_range` 不剥。
- SQLite KV：`next_prefix` 把最后一个非 `0xFF` 字节加一；**全 `0xFF` 的前缀没有后继**，于是 `get_by_prefix`/`erase_by_prefix` 走 `?1 <= k`——在字节序上这恰好等价于「以该前缀开头」。由此 `erase_by_prefix("")` **删掉整张表**。
- SQLite KV：空 `Slice` 的 `data()` 是 `""` 而不是 `nullptr`（`tdutils/td/utils/Slice-decl.h:139`），所以 `bind_blob` 永远绑成 **zero-length blob**，绝不是 NULL。`get()` 对「缺失的 key」和「空值」都返回 `""`，`crc32` 都是 0；能分开它们的只有 `get_all()` 里那条 `CHECK(!key.empty())`（它只拒绝**空 key**，不拒绝空值）。
- SQLite KV：`commit` 没有配对的 `begin` 是 `Status::Error("No matching begin for commit")`；计数器在共享的 `RawSqliteDb` 上，**两次 `begin` 需要两次 `commit`**，只提交一次只是把计数从 2 降到 1（不执行 SQL），第三次 `commit` 才报错。
- SQLite KV：`close()` 是 `*this = SqliteKeyValue()`（丢连接），关闭后 `footprint` 不应留下 `-journal`/`-wal`/`-shm`；`drop()` 之后对象为空，再次 `init` 会 `CREATE TABLE IF NOT EXISTS` 出一张**空表**；`destroy` 删四个路径（`-shm` 不参与「删不掉就报错」的检查）。
- TQueue：**一条规则贯穿三个 API**——`get_size` 不把 data 为空的**最后一个**事件算进去。于是 `clear(q, 0)` 只返回 `size - 1` 个事件（最后那个保留槽位、保留 `log_event_id`、data 被清空并让 storage rewrite 它），`pop` 对「下一个 id 就是 tail」的事件走 rewrite 而不是删除，`get` 返回的也是这个 size 而不是它交回的事件数（`max_events = 0` 时能观察到 `ret` 与 `n` 分叉）。
- TQueue：`get` 的清理发生在「满了就停」之前——`max_events = 0` 仍然会把队头过期/空 data 的事件 `pop` 掉，所以一次「什么都没返回」的 `get` 也会改变 head。
- TQueue：`push` 只在 `tail_id.empty()` 时采纳 `hint_new_id`；有 hint 就把 tail 钉在那里，之后的 push 从 tail 往后数。不给 hint 时 tail 取 `Random::fast(2000001, MAX_ID/2)`，**不可复现**——语料因此全程给 hint。
- TQueue：`get` 的两个窗口是 `from_id > tail + 10`（future）与 `from_id < head - 100000`（past），**边界值本身不报错**（`tail + 10` 恰好通过）。
- TQueue：`push` 的检查顺序是 empty-data → 太大 → queue 0 → 满 → 总长 → already-expired，所以一个「又大又过期」的请求报的是 `Data is too big`。
- TQueue：`EventId::from_int32(0)` **成功**（0 有效但 `empty()`），`MAX_ID = 2000000000` 及以上报 `Invalid ID`；`next()`/`advance()` 先算再校验，`next(1999999999)` 因此失败。
- TQueue：GC 只在**队列的调度时间严格早于 `unix_time_now`** 时触发，而那个时间是该队列第一次 push 时的 `expires_at`——之后 push 的短命事件不会提前它。
- TQueueBinlog：帧体是 `TQueueLogEvent` = `queue_id:i64 ‖ event_id:i32 ‖ expires_at:i32 ‖ data:TL-string ‖ [extra:i64 仅当 != 0]`，`magic = 2314 + (extra != 0)`。帧长 = **32（头 28 + crc 4）+ body**，所以 `extra = 7` 的帧 60 字节、`extra = 0` 的 52 字节；`binlog_dump` 打印的 `size:28` 是 **body** 长度。
- TQueueBinlog：`clear`/`pop` **只清 data、不清 extra**，所以一个 2315 的事件被清空后 rewrite 帧仍然是 **2315**，body 里 `extra` 仍是原值。「按当前 extra 决定 magic」这句话仍然成立，但语料要按「extra 保留」来写。
- TQueueBinlog：`erase` 是 32 字节的 `type = -2`、`flags = 1`、空 data 帧，`id` 指向**被删事件的 `log_event_id`**；`erase_batch` 先用 `next_event_id(0)`（计数器不动）再逐条 `erase`。
- TQueueBinlog：`replay` 拿到的是 `BinlogEventsProcessor` 的 **live 集**，不是磁盘上的帧——6 个帧里只有 1 个能进队列。Rust 侧若直接 `walk_frames` 后逐帧 `do_push`，会把 erase 帧当成 `Wrong magic`。

## 4. 不可判项 + 回流条件

| 项 | 原因 | 回流条件 |
|---|---|---|
| SQLite 文件的字节 | 两侧 sqlite 版本跨 22 个次版本（3.31.0 vs 3.53.1），页布局与 `sqlite_master` 文本不可比 | 两侧对齐到同一份 amalgamation；否则用交叉读代替（已做） |
| 随机前缀的内容与 `Secret::create_new` 的随机性 | `Random::secure_bytes` 走 OS，两次运行必不同 | 只判结构（长度公式 + 首字节 = 长度 + 16 对齐）；内容不可判 |
| `encrypt_file` / `decrypt_file` 的**其余** OS 失败 | 除两条已钉的 `FileFd` 文案外，错误文本随平台与本地化变 | happy path、对齐公式、双向交叉读、篡改检测与两条文案已由 `secure_storage_file` lane 判掉（F552）；只把「其余 OS 失败」留在这里 |
| SQLCipher 加密路径（`PRAGMA key` / `cipher_compatibility`） | `DbKey` 非空才走，`rusqlite` 的 bundled 不带 sqlcipher | 换 `bundled-sqlcipher`，且接受密文仍不可比 |
| `RawSqliteDb::last_error` 在 `SQLITE_CORRUPT` 时删库 | 需要造一个损坏库，且删完才有可观察差异 | 造损坏库并只比「文件是否还在」 |
| `SqliteStatement::explain()` 的 query plan 文本 | 计划文本随 sqlite 版本变 | 版本对齐后 |
| `trace()` 的 SQL 文本 | 调试接口，不是 KV 合同 | 不判 |
| 多元素 `set_all` | `FlatHashMap` 遍历序决定写入顺序，进而决定 rowid 序 | 只判单元素；多元素顺序不可复现 |
| `TQueue` 无 hint 的初始 tail | `Random::fast(2000001, MAX_ID/2)` 不可注入 | 语料全程给 hint；无 hint 只判「落在区间内」 |
| `run_gc` 的 50 ms 预算与 `(++counter & 128)` 分片 | 小语料一轮远不到 50 ms，也不到 128 次迭代 | 造 >128 个事件并让一轮超过 50 ms，或把时钟变成参数 |
| `Time::now()` 在 `clear` 的耗时告警里 | 只在超过 20 ms 时打日志 | 造一个清空耗时 >20 ms 的队列 |
| `TQueueBinlog` 的加密形态（`DbKey` 非空 → 帧体 AES-CTR） | 未驱动 `TQueueBinlog` + 非空 `DbKey` 的组合；binlog 写侧加密本身已判（F189/F190） | 接 `DbKey` 重跑，只多一条 8 字节级断言 |
| `TQueueBinlog` 的 reindex | 语料 4,016 B，远低于 `> 50 KiB + 5:1 死比` 的触发线 | 造 10 万级事件的队列语料 |
| `TQueue::close(Promise)` | 需要 actor 才能兑现 Promise | 造出脱离 actor 的 Promise |
| `TdDb::open` 全流程 | 走 actor/Promise，需要 scheduler | 造出脱离 actor 的入口 |
| `change_key` 的 `old_db_key` 迁移分支 | 未驱动 | 有迁移语料时 |
| 同一 `Binlog` 对象加密后再 `init` | C++ 侧 `0xC0000005`（文件完好） | 不判；按 `TdDb` 的用法每次 open 新建对象 |
| `erase_by_prefix` 的**多匹配**删除顺序 | 帧按 `WaitFreeHashMap` 遍历序写，移植哈希表才能复现 | 语料每次最多一个匹配；多键顺序由 `erase_batch` 覆盖 |
| 异步层的延迟 flush 时序 | `MAX_PENDING_QUERY_DELAY = 10 ms` 与 `MAX_PENDING_QUERY_COUNT = 100` 要两个以上操作同时在飞才观察得到；串行驱动下每个 op 都等到 flush | 写一个「并发投递 N 个 op、只等 promise 计数」的驱动，并接受时序抖动 |
| 多元素 `set_all` 的遍历序 | `FlatHashMap` 迭代序决定 `kv->set_all` 的写入顺序（影响 rowid 序） | 只判单元素；多元素顺序另立 lane（与 `erase_by_prefix` 同类问题） |
| 多个 buffered op 的 promise 合批 | `set_promises` 在 flush 时一次性兑现，串行驱动看不到「合批」 | 同上，需要并发驱动 |
| 连接安全层的 per-thread 槽位 | `SchedulerLocalStorage` 每线程一份，探针只起 1 个 scheduler 线程，只用得到 0 号 | 起 ≥2 个 scheduler 线程并在两个 actor 里各取一次连接 |
| ≥ MAX_PATH 的路径、非 ASCII 路径 | `GetFullPathNameW` 260 单元缓冲未定义 / 未测 | 上游改缓冲或加语料 |
| Windows 错误文案的本地化 | `FormatMessage` 跟 locale 走 | 两侧同 locale 时可比（当前即如此） |

## 5. 失效条件

任一变化都要重编探针 + 重跑对应 lane + 重新落快照：

- **基座漂移**：判定基座不是 `1b9efe84d`（或 oracle md5 变）⇒ 本文件整体作废，按新 hash 另起一份；
- `tddb/td/db/binlog/Binlog.{h,cpp}`、`BinlogEvent.{h,cpp}`、`BinlogInterface.h`、`BinlogKeyValue.h`、`SeqKeyValue.h`、`DbKey.h`；
- **Rust 侧**：`rust/src/binlog_format.rs`（三份探针共享的帧格式，改动会同时影响 write / kv / read 三条仪器）；
- `tdutils/td/utils/crypto.{h,cpp}`（PBKDF2 迭代次数、HMAC 常量、AES-CTR 进位）；
- `td/telegram/TdDb.{h,cpp}`（`as_db_key`、`check_parameters`、两个路径 static）；
- `tdutils/td/utils/port/path.cpp`（`mkpath`/`mkdir`/`realpath`）；
- `tddb/td/db/{SqliteKeyValue.{h,cpp},SqliteStatement.{h,cpp},detail/RawSqliteDb.{h,cpp}}`（SQL 语句、绑定与视图、`next_prefix`、事务计数）；
- `rust/src/sqlite_kv.rs`（Rust 侧同一套语句）；`rusqlite` 的 bundled sqlite 跨大版本（当前 3.53.1）；
- `tddb/td/db/TQueue.{h,cpp}`（`get_size` 的空尾规则、`pop` 的 rewrite 分支、两个 sanity 窗口、六个 `MAX_*` 常量）；`rust/src/tqueue.rs`（Rust 侧同一套队列语义）；
- `TQueueLogEvent::store/parse` 的字段顺序、`BINLOG_EVENT_TYPE`（2314）、`rust/src/binlog_format.rs` 的 `Frame`/`walk_frames`/`Processor`；
- `tddb/td/db/{SqliteConnectionSafe.{h,cpp},SqliteKeyValueSafe.h}` 或 `LazySchedulerLocalStorage` 的语义；
- oracle 重建（`libtdjson.dll` md5 变）或 `binlog_dump.exe` 重编。
- `td/telegram/SecureStorage.{h,cpp}`（哈希、两条 `AesCbcState` 派生、校验和 `(255 + 239 - sum % 255) % 255`、前缀结构、两个状态机）或 `tdutils` 的 `DataView` 分块大小（128 KiB）。