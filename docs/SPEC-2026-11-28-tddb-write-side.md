# SPEC（as-built）— `tddb` 存储写侧（binlog + crypto + KV + TdDb 路径）

> **as-built 规格，不是 to-build。** 它记录「已判过、数字可复现」的现状，供接手方当底图。
> 台账行为证据来源：`docs/facts.md` F185–F194。

## 1. 定义域

| 项 | 值 |
|---|---|
| C++ 源 | `tddb/td/db/binlog/{Binlog,BinlogEvent,BinlogInterface}.{h,cpp}`、`tddb/td/db/{BinlogKeyValue.h,SeqKeyValue.h,DbKey.h,KeyValueSyncInterface.h}`、`td/telegram/TdDb.{h,cpp}`、`tdutils/td/utils/crypto.{h,cpp}`、`tdutils/td/utils/port/path.cpp`（`mkpath`/`mkdir`/`realpath`） |
| 规模 | 被判 11 个文件 / 2,728 行（`tddb` 全目录 4,792 行） |
| oracle | `libtdjson.dll` + `C:/tmp/td-build/tddb/{libtddb.a,binlog_dump.exe}` + `libtdcore.a`（`TdDb` 那条） |
| 台账 | F185–F194；`tasks/plan.md` T92–T96 |

## 2. 可复现现状

```bash
cd C:/projects/td
uv run python tools/binlog_diff.py --all --selftest        # 读侧：14 个 fixture，diff_lines=0
uv run python tools/binlog_write_diff.py --selftest        # 写侧：27 case（20 明文 + 7 加密），1.9 s
uv run python tools/binlog_write_diff.py --encrypted       # 只跑加密半边，0.9 s
uv run python tools/crypto_diff.py --selftest              # 457 行，6.7 s
uv run python tools/tddb_paths_diff.py --selftest          # 35 行 + 目录树，0.2 s
```

## 3. 判据

| 面 | 判据形态 | 规模（实测） | fact |
|---|---|---|---|
| binlog 读侧 | 逐行（`binlog_dump` vs `binlog_head`） | 14 fixture，`diff_lines=0` | 早期 round |
| binlog 写侧·明文 | **文件逐字节** | 20 case / 345,272 B，`md5=339d6923`，1.9 s | F185/F186 |
| binlog 写侧·加密 | **结构 + 双向交叉读**（C++ `binlog_dump` 读 Rust 文件、Rust `binlog_head` 读 C++ 文件）+ Python 重算 `key`/`key_hash` | 7 case / 201,168 B，0.9 s | F189/F190 |
| crypto 原语 | 逐字节 + 独立 `hashlib`/`hmac`/`zlib` 反算 + RFC 7914 / RFC 4231 / NIST F.5.5 | 457 行，`md5=4d39370b`，6.7 s | F187/F188 |
| KV 层（`BinlogKeyValue` + `SeqKeyValue`） | 明文 binlog 逐字节 + narration + 双向交叉读（四条全跑不短路） | 21 case / 884,208 B（含 4 份副本），`md5=d8c284e1`，1.84 s | F191/F192 |
| `TdDb` 路径与 key | stdout 逐字节 + **目录树快照** | 35 行，`md5=ad8d60ca`，0.2 s | F193/F194 |

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

## 4. 不可判项 + 回流条件

| 项 | 原因 | 回流条件 |
|---|---|---|
| SQLite 文件的字节 | 两侧 sqlite 版本不同（`rusqlite` bundled vs C++ 侧），页布局不可比 | 版本对齐后再谈 |
| `TdDb::open` 全流程 | 走 actor/Promise，需要 scheduler | 造出脱离 actor 的入口 |
| `change_key` 的 `old_db_key` 迁移分支 | 未驱动 | 有迁移语料时 |
| 同一 `Binlog` 对象加密后再 `init` | C++ 侧 `0xC0000005`（文件完好） | 不判；按 `TdDb` 的用法每次 open 新建对象 |
| `erase_by_prefix` 的**多匹配**删除顺序 | 帧按 `WaitFreeHashMap` 遍历序写，移植哈希表才能复现 | 语料每次最多一个匹配；多键顺序由 `erase_batch` 覆盖 |
| ≥ MAX_PATH 的路径、非 ASCII 路径 | `GetFullPathNameW` 260 单元缓冲未定义 / 未测 | 上游改缓冲或加语料 |
| Windows 错误文案的本地化 | `FormatMessage` 跟 locale 走 | 两侧同 locale 时可比（当前即如此） |

## 5. 失效条件

任一变化都要重编探针 + 重跑对应 lane + 重新落快照：

- `tddb/td/db/binlog/Binlog.{h,cpp}`、`BinlogEvent.{h,cpp}`、`BinlogInterface.h`、`BinlogKeyValue.h`、`SeqKeyValue.h`、`DbKey.h`；
- **Rust 侧**：`rust/src/binlog_format.rs`（三份探针共享的帧格式，改动会同时影响 write / kv / read 三条仪器）；
- `tdutils/td/utils/crypto.{h,cpp}`（PBKDF2 迭代次数、HMAC 常量、AES-CTR 进位）；
- `td/telegram/TdDb.{h,cpp}`（`as_db_key`、`check_parameters`、两个路径 static）；
- `tdutils/td/utils/port/path.cpp`（`mkpath`/`mkdir`/`realpath`）；
- oracle 重建（`libtdjson.dll` md5 变）或 `binlog_dump.exe` 重编。