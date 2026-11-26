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
| 门禁 | `bash C:/Users/K/.omp/agent/skills/face-spaghetti/scripts/append-only-check.sh --worktree`（在 Git Bash 里跑）；`python tools/tdjson_diff.py` | — |

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

未建。