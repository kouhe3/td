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
| 门禁 | `python tools/verify.py`（构建 + 差分仪器 + 五个生成器 `--check` 一条命令）；追加检查 `bash C:/Users/K/.omp/agent/skills/face-spaghetti/scripts/append-only-check.sh --worktree`（在 Git Bash 里跑） | — |

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
| skill 目录 | `C:/Users/K/.omp/agent/skills/face-spaghetti` |

## 共享资源

无。当前判据只用 `td_execute` 纯函数路径：不登录、不联网、不碰配置目录与注册表。

## 冻结树

本项目没有换目标，也不需要搬迁；这里的「冻结」指**里程碑快照**：

| 项 | 值 |
|---|---|
| 快照日期 | 2026-11-26（round 1 = `6c4661eb4`；round 2 = T19 + T15–T18（F052）；round 3 = logStreamFile + json_decode 文案（F056）；round 4 = T22 的 lane 尾巴（F058）） |
| 基线提交 | `6c4661eb4`（round 1） |
| 机器记录 | `docs/gate-2026-11-26.json` |
| 复跑命令 | `python tools/verify.py` |
| 当时结果 | round 1：六项全绿、279 PASS；round 2：573 PASS（MATCH 284，F052）；round 3：645 PASS（MATCH 318，F056）；round 4：759 PASS（MATCH 375，F058）——七项全绿贯穿 |
| 已知缺口 | B1（有状态客户端，需测试 DC 账号）、B7（语言包成功路径需 pack 数据）、B10（超 DBL_MAX 浮点，兼作 CONTROL）；另：请求/字符串里的非法 UTF-8 与孤立代理对两侧表示不同 |

换目标或大改前先重跑 `tools/verify.py`；数字变了就把旧快照行置 `过期`，另起一行记新快照（round 1 = F044，2 = F052，3 = F056，4 = F058）。注意 Rust dll 的 md5 **随 HEAD 变化**（`build.rs` 烘焙 commit hash），跨轮只比大小与 warning 数。

仪器自己加载运行时：`tools/tdjson_diff.py` 用 `os.add_dll_directory` 挂上 `C:/msys64/ucrt64/bin`——CPython 3.8+ 的 `CDLL` 走 `LOAD_LIBRARY_SEARCH_DEFAULT_DIRS`，`PATH` 不参与依赖查找，所以不必再导出 PATH。