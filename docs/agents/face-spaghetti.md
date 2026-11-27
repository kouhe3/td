# Face Spaghetti 落点

本仓库的 face-spaghetti 配置。`SKILL.md` §9 读它；手改优先于重跑 setup。

## 目标框架

首要目标是**检验 face-spaghetti skill 本身**，RIIR 是练习载体（用户决定，2026-11-27）；「产出可上生产的 TDLib-RS」不是目标。

- 验收不是覆盖率：`MATCH 1561` 只说明样本有效，不说明交付。
- 本仓库落在 skill 的**排除区**：有文档、有测试、有可执行 oracle，skill 的适用范围是「未文档化、未测试」。这一轮被检验的是「有 oracle 的对象怎么做」，属它的边缘。
- 判据来源 = 只有实现（`docs/adr/0001-json-abi-as-the-rewrite-boundary.md` 的边界），且上游每天更新 → 只有快照态、没有完成态（快照见 `docs/gate-*.json`）。

## 落点

| 项 | 路径 / 命令 | 覆盖开关 |
|---|---|---|
| 台账 | `docs/facts.md` | `FACTS_FILE` |
| 计划 / todo | `tasks/plan.md`（门禁行 + 任务表 T1–T56 + 变更记录）；`tasks/todo.md` 不存在，短期队列用 plan 的进度节 | `PLAN_GLOBS` |
| 判据定义域表 | `docs/coverage-domain.md` | — |
| 进度 / surface 表 | `tasks/plan.md` 的「进度」节：两个口径分开写（可判表面 / TDLib 本体），主分母饱和后切 lane cell（round 16） | — |
| 领域语言 | 未建（`/domain-modeling` 按需） | — |
| 一次性探测目录 | `work/`（gitignored） | — |
| 仪器目录 | `tools/`：`tdjson_diff.py`（`td_execute` 差分 + `SEQUENCE` 段）、`binlog_diff.py`（binlog 输出与 `binlog_dump` 逐行比，`--all` 覆盖 14 个 fixture）、`tdjson_fuzz.py`（随机语料 + 随机序列）、`client_diff.py`（首启 / 第二次运行 / 未授权 / 遗留 C ABI / 异步五段）、十二个协议 lane（`auth_key_diff.py` / `msg_layer_diff.py` / `transport_diff.py` / `framing_diff.py` / `msgid_diff.py` / `rpc_diff.py` / `rpc_error_diff.py` / `auth_data_diff.py` / `svc_msg_diff.py` / `proxy_secret_diff.py` / `header_diff.py` / `session_diff.py`，各自 `--selftest`）、`gen_language_pack.py`（语言包 fixture）与八个 `gen_*.py --check` | — |
| 只读 oracle | `C:/tmp/td-build/libtdjson.dll`（md5 `0398814038978aaeef7f8d6ac62bb8ff`，97,185,382 B）；运行副本由 `tools/tdjson_diff.py` 直接加载，不改写。binlog lane 的 oracle 是 `C:/tmp/td-build/tddb/binlog_dump.exe`（`ninja -C C:/tmp/td-build binlog_dump`）。协议 / RPC lane 的 oracle 是一次性探针 `C:/tmp/td-probe/*.exe`，源码在 `tools/fixtures/*.cpp`，完整 g++ 命令行写在每个文件头 | — |
| backlog 发布 | GitHub issues，仓库钉死 `kouhe3/td`（`docs/agents/issue-tracker.md`）；`tasks/backlog.md` 保留未成 issue 的条目 | — |
| 配套文件 | `docs/agents/issue-tracker.md` / `triage-labels.md` / `domain.md` 已装（mattpocock-skills 1.3.2，project scope） | — |
| 判据形态（时间 / 随机） | 时间依赖的状态机把 `now` 当参数注入（`auth_data_probe` 的脚本），不模拟环境也不打桩时钟；算法内部含随机只判不变式（`msgid_probe`）；双精度一律比 64 位位型（`auth_data_probe`），避免格式化差异被读成分歧 | — |
| C 档 oracle | `tools/fixtures/session_probe.cpp`：假 `RawConnection` 注入明文 + 假回调记录事件，脚本驱动（`newconn`/`in`/`salt`）；Rust 对侧 `rust/src/bin/session_probe.rs`，仪器 `tools/session_diff.py`（F122–F125） | — |
| 门禁 | `uv run python tools/verify.py`（二十四项：build、instrument、binlog、auth_key、msg_layer、transport、framing、msgid、rpc、rpc_error、auth_data、svc_msg、proxy_secret、header、session、fuzz、client_options、client_lane、tl_schema、mime_tables、emoji_table、country_table、ton_address、api_functions；最后在 HEAD 跑约 45–55 s）；追加检查 `bash C:/Users/K/.omp/agent/skills/face-spaghetti/scripts/append-only-check.sh --worktree`（在 Git Bash 里跑） | — |
| 生成器的前置 | `gen_api_functions.py` / `gen_tl_schema.py` 读构建产物 `td/generate/auto/td/telegram/td_api.h` 的 `ID`（源码行 crc32 在 422/3464 个定义上是错的，F091）。`td/generate/auto/` 是 gitignored 的构建产物，所以 `--check` **必须先 build**——`verify.py` 里 build 排在这些门禁之前，`git archive` 导出后跑一遍也会先构建；这两个生成器的 docstring 已写明 | — |

配套分工见 skill 的 `references/pairing.md`；本文件只记落点。

## 本机事实

| 项 | 值 |
|---|---|
| oracle 构建目录 | `C:/tmp/td-build`（out-of-source，不污染仓库） |
| 构建配方 | MSYS2 **ucrt64**：`gcc/g++ 16.2.0`、OpenSSL `4.1.0`、zlib `1.4`、`gperf 3.3`、cmake 4.4.4、ninja 1.13.2、PHP 8.6（scoop） |
| 配置命令 | `cmake -S C:/projects/td -B C:/tmp/td-build -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER=C:/msys64/ucrt64/bin/gcc.exe -DCMAKE_CXX_COMPILER=C:/msys64/ucrt64/bin/g++.exe -DOPENSSL_ROOT_DIR=C:/msys64/ucrt64 -DCMAKE_PREFIX_PATH=C:/msys64/ucrt64 -DZLIB_ROOT=C:/msys64/ucrt64 -DTD_ENABLE_LTO=OFF -DTD_ENABLE_DOTNET=OFF` |
| 构建命令 | `PATH="C:/msys64/ucrt64/bin;C:/msys64/usr/bin;$PATH" ninja -C C:/tmp/td-build tdjson -j 16`（645 目标，13m23s） |
| 编译器 PATH 坑 | 不加 `C:/msys64/ucrt64/bin` 时 `g++.exe` 编译**静默失败**（rc=1、零输出）——`cc1plus` 找不到同目录 DLL |
| Rust 侧 | 工程在本仓库 `rust/`（crate `tdjson_rs`，cdylib）；目标 `x86_64-pc-windows-gnu`，linker = `C:/msys64/ucrt64/bin/gcc.exe`（`rust/.cargo/config.toml`）；构建 `cargo build --release --target x86_64-pc-windows-gnu --manifest-path rust/Cargo.toml`。依赖只有 `rusqlite`（`bundled`）——它用 `cc` 编译自带的 sqlite3.c，所以**同一个 `C:/msys64/ucrt64/bin` 也必须在 PATH 上**，否则构建报 `failed to find tool "gcc.exe"`（首次构建约 80 s，之后增量 10 s 内） |
| 解释器 | 一律 `uv run python tools/…`（裸 `python` 是用户约定里禁止的），Python 3.14 |
| 生成代码落点 | 构建写入**源码树** `td/generate/auto/`（`.gitignore` 的 `**/auto/` 覆盖），无自动清理 |
| 测试 DC（B1） | test DC2 = `149.154.167.40:443`，公钥同 `PublicRsaKeySharedMain.cpp:26` 的 test 钥匙，api_id/api_hash = `94575` / `a3406de8d171bb422bb6ddf3bbd800e2`；**全部编译期内置**，`use_test_dc:true` 即可（F065）。本机直连该地址**不通**、无本地 SOCKS 监听 |
| skill 目录 | `C:/Users/K/.omp/agent/skills/face-spaghetti` |

## 共享资源

客户端 lane 会写 gitignored 的 `work/client/`（每段每侧一个独立子进程 + 一个全新 `database_directory`，各子目录 `cpp-*` / `rs-*` / `*-async` / `*-legacy`）；不碰用户账号、不登录、不依赖网络（首启 129 行在连接建立前就到 `waitPhoneNumber`，F076）。纯函数 lane 仍只用 `td_execute`。

其余会落盘的仪器：`work/langpack/{cpp,rs}/`（语言包 fixture，**每侧一份副本**——C++ 会把 `!key_count` 写回它读的库，共享一份会让两侧输入不同）、`work/logs/`（`logStreamFile` 序列）、`work/logs-link/`（junction）、`work/logs-fuzz/`（随机序列的日志文件）。这些目录都由仪器自己建，删掉重跑即可，恢复命令即「重跑 `verify.py`」。

## 冻结树

本项目没有换目标，也不需要搬迁；这里的「冻结」指**里程碑快照**：

| 项 | 值 |
|---|---|
| 快照日期 | 2026-11-26（round 1 = `6c4661eb4`（F044）；2 = `65deb30fc`（F052）；3 = `7c2b0dd00`（F056）；4 = `dfcfc1017`（F058）；5 = 链接 lane + 浮点溢出（F061）；6 = 非法 UTF-8 字节化（F064）；7 = 客户端首启 lane（F076/F077）；8 = 第二次运行 lane（F078）；9 = 未授权状态机（F079）；10 = 真实登录形态（F080，仅记录）；11 = 函数参数解析（F081，MATCH 1528）；12 = 嵌套 `@type` 两条路径（F082，MATCH 1534）；13 = 参数 lane 普查（F083，仅记录）；14 = 语言包成功路径 + 随机语料 lane（F087/F088，`70a3b4585`，MATCH 1561）；15 = 遗留 C ABI + 构造号表改读构建产物（F090/F091，`06c0ccf8b`）；16 = 异步请求路径三支分流（F096/F097，`6235a7b88`）） |
| 基线提交 | `6c4661eb4`（round 1） |
| 机器记录 | `docs/gate-2026-11-26.json`（round 1）与 `docs/gate-2026-11-26-round2.json`（round 2）；round 3 起没有入库快照，需要时用 `uv run python tools/verify.py --json > docs/gate-<date>.json` 现场生成 |
| 复跑命令 | `uv run python tools/verify.py` |
| 当时结果 | round 1：279 PASS；2：573（284）；3：645（318）；4：759（375）；5：793（392）；6：837（414，含 22 条 bytes cell）；7：837 + 客户端首启 129 行；8：+ 第二次运行 127 行（集合）；9：+ 未授权状态机 142 行；10：无新增仪器行（一次性实测 222 行）；11：MATCH 1528（+1114 个函数参数 cell）；12：MATCH 1534（+6 条嵌套 `@type` cell）；13：无新增仪器行（6 个参数变体全 same，证明回放表不需按参数分叉）；14：MATCH 1561（+27 条语言包 cell）+ 新 fuzz 门禁（11 项，30.7 s）；15：+ 遗留 C ABI 段（auth lane 143 行）与随机序列/构造号形态（32.7 s）；16：+ async 段（16 条请求）+ fuzz 3 种子（53.4 s）——门禁从十项到十一项，round 9 起 `client_lane` 是最慢的一项（约 30–47 s） |
| round 17（2026-11-27） | 新增 `auth_data`（salt / 时间差 / 窗口 / `check_packet`，3,026 步）与 `svc_msg`（7 个维护类服务消息，41 case）两条 lane，门禁 19 → 21，全绿 44.7 s；台账 F113–F115 |
| round 18（2026-11-27） | 新增 `proxy_secret`（`ProxySecret` 全公开行为，128 case，41 手写 + 87 随机），门禁 21 → 22；台账 F116–F117 |
| round 19（2026-11-27） | 新增 `header`（`MtprotoHeader` 选项矩阵，85 case × 两份头），门禁 22 → 23；台账 F118–F120；顺带修掉 oracle 的 ANSI argv 假分歧（F119） |
| round 20（2026-11-27） | JSON 面随机语料扩种子 2027–2030（3,400 条 + 80 段/1,800 步零分歧，F121），门禁 fuzz 项改为 4 个种子 |
| round 21（2026-11-27） | C 档可判性探测：`SessionConnection` 可用假传输驱动，六场景事件符合预期且两次运行一致（F122/F123）；C 档从 blocked 降为「待立项」 |
| round 22（2026-11-27） | C 档实现：`session` lane（35 op / 88 行，容器 + rpc_result + 11 个 error_code + 换盐），门禁 23 → 24；台账 F124/F125 |
| 已知缺口 | 七条，全部带 blocker：真实登录（`ready` 之后没有 MTProto 引擎，异步 lane 只覆盖未授权态）、`getOption commit_hash`（两侧各烘焙 HEAD）、第二次运行顺序（oracle 并发）、非空号码 `setAuthenticationPhoneNumber`（联网挂起）、`addLogMessage` level ≤ 0（oracle abort）、含非法 UTF-8 的语言包值（`td::JsonBuilder` 是 CHECK）、C ABI 之下。另有两条窄缺口写在 F124 行内（`req_msg_id == 0`、gzip 分支）。`check_packet` 的 `Ignore too old or too new` 分支在 update 后恒真（F113）。B1 的四段与 B7 都已关闭。**纯函数面无已知行为差异**（CONTROL 为空，改由管道自检接手） |

换目标或大改前先重跑 `tools/verify.py`；数字变了就把旧快照行置 `过期`，另起一行记新快照（1 = F044，2 = F052，3 = F056，4 = F058，5 = F061，6 = F064，7 = F076/F077，8 = F078，9 = F079，10 = F080，11 = F081，12 = F082，13 = F083，14 = F087/F088，15 = F090–F095，16 = F096–F098，17 = F113–F115，18 = F116/F117，19 = F118–F120，20 = F121，21 = F122/F123，22 = F124/F125）。注意 Rust dll 的 md5 **随 HEAD 变化**（`build.rs` 烘焙 commit hash），跨轮只比大小与 warning 数。

仪器自己加载运行时：`tools/tdjson_diff.py` 用 `os.add_dll_directory` 挂上 `C:/msys64/ucrt64/bin`——CPython 3.8+ 的 `CDLL` 走 `LOAD_LIBRARY_SEARCH_DEFAULT_DIRS`，`PATH` 不参与依赖查找，所以不必再导出 PATH。