# Face Spaghetti 落点

本仓库的 face-spaghetti 配置。`SKILL.md` §9 读它；手改优先于重跑 setup。

## 落点

| 项 | 路径 / 命令 | 覆盖开关 |
|---|---|---|
| 台账 | `docs/facts.md` | `FACTS_FILE` |
| 计划 / todo | `tasks/plan.md`、`tasks/todo.md` | `PLAN_GLOBS` |
| 判据定义域表 | `docs/coverage-domain.md` | — |
| 进度 / surface 表 | 未建 | — |
| 领域语言 | 未建（`/domain-modeling` 按需） | — |
| 一次性探测目录 | `work/`（gitignored） | — |
| 仪器目录 | `tools/` | — |
| 只读 oracle | `C:/tmp/td-build/libtdjson.dll`（md5 `0398814038978aaeef7f8d6ac62bb8ff`，97,185,382 B）；运行副本由 `tools/tdjson_diff.py` 直接加载，不改写 | — |
| backlog 发布 | GitHub issues，仓库钉死 `kouhe3/td`（`docs/agents/issue-tracker.md`）；`tasks/backlog.md` 保留未成 issue 的条目 | — |
| 配套文件 | `docs/agents/issue-tracker.md` / `triage-labels.md` / `domain.md` 已装（mattpocock-skills 1.3.2，project scope） | — |
| 门禁 | `python tools/verify.py`（构建 + 差分仪器 + 客户端 lane + 六个生成器 `--check` 一条命令）；追加检查 `bash C:/Users/K/.omp/agent/skills/face-spaghetti/scripts/append-only-check.sh --worktree`（在 Git Bash 里跑） | — |

配套分工见 skill 的 `references/pairing.md`；本文件只记落点。

## 本机事实

| 项 | 值 |
|---|---|
| oracle 构建目录 | `C:/tmp/td-build`（out-of-source，不污染仓库） |
| 构建配方 | MSYS2 **ucrt64**：`gcc/g++ 16.2.0`、OpenSSL `4.1.0`、zlib `1.4`、`gperf 3.3`、cmake 4.4.4、ninja 1.13.2、PHP 8.6（scoop） |
| 配置命令 | `cmake -S C:/projects/td -B C:/tmp/td-build -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER=C:/msys64/ucrt64/bin/gcc.exe -DCMAKE_CXX_COMPILER=C:/msys64/ucrt64/bin/g++.exe -DOPENSSL_ROOT_DIR=C:/msys64/ucrt64 -DCMAKE_PREFIX_PATH=C:/msys64/ucrt64 -DZLIB_ROOT=C:/msys64/ucrt64 -DTD_ENABLE_LTO=OFF -DTD_ENABLE_DOTNET=OFF` |
| 构建命令 | `PATH="C:/msys64/ucrt64/bin;C:/msys64/usr/bin;$PATH" ninja -C C:/tmp/td-build tdjson -j 16`（645 目标，13m23s） |
| 编译器 PATH 坑 | 不加 `C:/msys64/ucrt64/bin` 时 `g++.exe` 编译**静默失败**（rc=1、零输出）——`cc1plus` 找不到同目录 DLL |
| Rust 侧 | 工程在本仓库 `rust/`（crate `tdjson_rs`，cdylib）；目标 `x86_64-pc-windows-gnu`，linker = `C:/msys64/ucrt64/bin/gcc.exe`（`rust/.cargo/config.toml`）；构建 `cargo build --release --target x86_64-pc-windows-gnu --manifest-path rust/Cargo.toml` |
| 生成代码落点 | 构建写入**源码树** `td/generate/auto/`（`.gitignore` 的 `**/auto/` 覆盖），无自动清理 |
| 测试 DC（B1） | test DC2 = `149.154.167.40:443`，公钥同 `PublicRsaKeySharedMain.cpp:26` 的 test 钥匙，api_id/api_hash = `94575` / `a3406de8d171bb422bb6ddf3bbd800e2`；**全部编译期内置**，`use_test_dc:true` 即可（F065）。本机直连该地址**不通**、无本地 SOCKS 监听 |
| skill 目录 | `C:/Users/K/.omp/agent/skills/face-spaghetti` |

## 共享资源

客户端 lane 会写 gitignored 的 `work/client/` 与 `work/gen-client-db/`（每侧一个独立子进程、一个全新 `database_directory`）；不碰用户账号、不登录、不依赖网络（首启 129 行在连接建立前就到 `waitPhoneNumber`，F076）。纯函数 lane 仍只用 `td_execute`。

## 冻结树

本项目没有换目标，也不需要搬迁；这里的「冻结」指**里程碑快照**：

| 项 | 值 |
|---|---|
| 快照日期 | 2026-11-26（round 1 = `6c4661eb4`（F044）；2 = `65deb30fc`（F052）；3 = `7c2b0dd00`（F056）；4 = `dfcfc1017`（F058）；5 = 链接 lane + 浮点溢出（F061）；6 = 非法 UTF-8 字节化（F064）；7 = 客户端首启 lane（F076/F077）；8 = 第二次运行 lane（F078）；9 = 未授权状态机（F079）；10 = 真实登录形态（F080，仅记录）；11 = 函数参数解析（F081，MATCH 1528）） |
| 基线提交 | `6c4661eb4`（round 1） |
| 机器记录 | `docs/gate-2026-11-26.json` |
| 复跑命令 | `python tools/verify.py` |
| 当时结果 | round 1：279 PASS；2：573（284）；3：645（318）；4：759（375）；5：793（392）；6：837（414，含 22 条 bytes cell）；7：837 + 客户端首启 129 行；8：+ 第二次运行 127 行（集合）；9：+ 未授权状态机 142 行；10：无新增仪器行（一次性实测 222 行）；11：MATCH 1528（+1114 个函数参数 cell）——十项门禁全绿贯穿（round 9 起 `client_lane` 约 14 s） |
| 已知缺口 | B1 的首启、第二次运行与未授权状态机已对齐（F076/F078/F079）；真实登录只在 oracle 侧留了形态记录（F080），等价性不做（没有 MTProto 引擎）；B7（语言包成功路径需 pack 数据）。**纯函数面无已知行为差异**（CONTROL 为空）；`coverage-domain` 里除 `getOption commit_hash`、第二次运行顺序、真实登录、嵌套 `@type` 用构造号、具体类带错 `@type` 外没有缺口 |

换目标或大改前先重跑 `tools/verify.py`；数字变了就把旧快照行置 `过期`，另起一行记新快照（1 = F044，2 = F052，3 = F056，4 = F058，5 = F061，6 = F064，7 = F076/F077，8 = F078，9 = F079，10 = F080，11 = F081）。注意 Rust dll 的 md5 **随 HEAD 变化**（`build.rs` 烘焙 commit hash），跨轮只比大小与 warning 数。

仪器自己加载运行时：`tools/tdjson_diff.py` 用 `os.add_dll_directory` 挂上 `C:/msys64/ucrt64/bin`——CPython 3.8+ 的 `CDLL` 走 `LOAD_LIBRARY_SEARCH_DEFAULT_DIRS`，`PATH` 不参与依赖查找，所以不必再导出 PATH。