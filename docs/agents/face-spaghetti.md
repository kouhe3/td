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
| 仪器目录 | `tools/`：`tdjson_diff.py`（`td_execute` 差分 + `SEQUENCE` 段）、`binlog_diff.py`（binlog 输出与 `binlog_dump` 逐行比，`--all` 覆盖 14 个 fixture）、`tdjson_fuzz.py`（随机语料 + 随机序列）、`client_diff.py`（首启 / 第二次运行 / 未授权 / 遗留 C ABI / 异步五段）、十四个协议 lane（`auth_key_diff.py` / `msg_layer_diff.py` / `transport_diff.py` / `framing_diff.py` / `msgid_diff.py` / `rpc_diff.py` / `rpc_error_diff.py` / `auth_data_diff.py` / `svc_msg_diff.py` / `proxy_secret_diff.py` / `header_diff.py` / `session_diff.py` / `web_proxy_diff.py` / `misc_diff.py`，各自 `--selftest`）、`gen_language_pack.py`（语言包 fixture）与八个 `gen_*.py --check` | — |
| 只读 oracle | `C:/tmp/td-build/libtdjson.dll`（md5 `0398814038978aaeef7f8d6ac62bb8ff`，97,185,382 B）；运行副本由 `tools/tdjson_diff.py` 直接加载，不改写。binlog lane 的 oracle 是 `C:/tmp/td-build/tddb/binlog_dump.exe`（`ninja -C C:/tmp/td-build binlog_dump`）。协议 / RPC lane 的 oracle 是一次性探针 `C:/tmp/td-probe/*.exe`，源码在 `tools/fixtures/*.cpp`，完整 g++ 命令行写在每个文件头 | — |
| backlog 发布 | GitHub issues，仓库钉死 `kouhe3/td`（`docs/agents/issue-tracker.md`）；`tasks/backlog.md` 保留未成 issue 的条目 | — |
| 配套文件 | `docs/agents/issue-tracker.md` / `triage-labels.md` / `domain.md` 已装（mattpocock-skills 1.3.2，project scope） | — |
| 判据形态（时间 / 随机 / 压缩 / 判定 / 别名） | 时间依赖的状态机把 `now` 当参数注入（`auth_data_probe` 的脚本），不模拟环境也不打桩时钟；算法内部含随机只判不变式（`msgid_probe`）；双精度一律比 64 位位型（`auth_data_probe`），避免格式化差异被读成分歧；**整段随机且顺序被打乱时降到结构比较**（`tls_init_diff`：解析 ClientHello，比 16 个固定扩展的归一化内容 + GREASE 数量 + cipher 列表 + SNI + 长度集合 + HMAC 自洽，每侧 8 个独立进程采样）；**压缩只判解压后等价**（`session_diff` 的 gzip_packed：比解压后的对象，不比压缩字节）；**判定函数按分支枚举边界 + 随机语料**（`dh_handshake_diff`：位数 / 模表 / 两层素性各给正反例，含 RFC 3526 group 14 与一个非 safe prime；Rust 侧自写 Miller-Rabin 与 OpenSSL 结论对齐）；**所有权靠「写一处、读两处」显形**（`buffer_slice_diff`：clone 后 mutate 要看原对象是否被污染，copy 后 mutate 要求隔离，`confirm_read`/`truncate` 再看窗口移动） | — |
| C 档 oracle | `tools/fixtures/session_probe.cpp`：假 `RawConnection` 注入明文 + 假回调记录事件，脚本驱动（`newconn`/`in`/`salt`）；Rust 对侧 `rust/src/bin/session_probe.rs`，仪器 `tools/session_diff.py`（F122–F125） | — |
| TlsInit oracle | `tools/fixtures/tls_init_probe.cpp` **include** 了 `td/mtproto/TlsInit.cpp`（`TlsObfusaction` 只存在于该 TU），链接要加 `-Wl,--allow-multiple-definition`；Rust 对侧 `rust/src/bin/tls_init_probe.rs`，仪器 `tools/tls_init_diff.py`（结构比较 + 每侧 8 个独立进程，F132/F133） | — |
| DH 参数 oracle | `tools/fixtures/dh_handshake_probe.cpp` 调 `DhHandshake::check_config(g, prime, nullptr)`（`nullptr` 跳过缓存分支）；Rust 对侧 `rust/src/bin/dh_handshake_probe.rs`（前 25 个小素数试除 + 12 个固定基 Miller-Rabin），仪器 `tools/dh_handshake_diff.py`（32 case，F136/F137） | — |
| RSA oracle | `tools/fixtures/rsa_probe.cpp` 对 PEM 文件调 `RSA::from_pem_public_key` 再跑 `get_fingerprint` / `encrypt` / `decrypt_signature`；Rust 对侧 `rust/src/bin/rsa_probe.rs`（base64 + DER 手写解析，SPKI 与 PKCS#1 两种 PEM 头），仪器 `tools/rsa_diff.py`（23 case，PEM 由仪器现场构造，F138–F140） | — |
| RSA 查表 oracle | `tools/fixtures/public_rsa_probe.cpp` 调 `PublicRsaKeySharedMain::create(is_test)` + `get_rsa_key(fps)`；Rust 对侧 `rust/src/bin/public_rsa_probe.rs`（PKCS#1 PEM 常量 + 同一套指纹），仪器 `tools/public_rsa_diff.py`（9 case，已知指纹由 `rsa_probe` 现算，F140/F141） | — |
| CDN 表 oracle | `tools/fixtures/public_rsa_cdn_probe.cpp`：`DcId::external(2)` + 脚本驱动 `add <pem>` / `get <fps>` / `drop` / `has`；Rust 对侧 `rust/src/bin/public_rsa_cdn_probe.rs`（只存指纹集合），仪器 `tools/public_rsa_cdn_diff.py`（`PASS lines=21`，F142/F143） | — |
| BufferSlice oracle | `tools/fixtures/buffer_slice_probe.cpp`：脚本驱动 `new`/`clone`/`copy`/`mutate`/`read`/`truncate`/`after`/`print`；Rust 对侧 `rust/src/bin/buffer_slice_probe.rs`（`Rc<RefCell<Vec<u8>>>` + begin/end），仪器 `tools/buffer_slice_diff.py`（`PASS lines=34`，F144/F145） | — |
| DcId oracle | `tools/fixtures/dc_id_probe.cpp`：脚本驱动 `is_valid` / `create` / `from_value` / `internal` / `external` / `cmp` / `cmp_external` / `cmp_mixed` / `raw`；Rust 对侧 `rust/src/bin/dc_id_probe.rs`（值类型 + 打印的五个分支），仪器 `tools/dc_id_diff.py`（`PASS lines=273`，F147/F148） | — |
| 门禁 | `uv run python tools/verify.py`（三十三项：build、instrument、binlog、auth_key、msg_layer、transport、framing、msgid、rpc、rpc_error、auth_data、svc_msg、proxy_secret、header、session、web_proxy、misc、tls_init、dh_handshake、rsa、public_rsa、public_rsa_cdn、buffer_slice、dc_id、fuzz、client_options、client_lane、tl_schema、mime_tables、emoji_table、country_table、ton_address、api_functions；最后在 HEAD 跑约 110–180 s）；追加检查 `bash C:/Users/K/.omp/agent/skills/face-spaghetti/scripts/append-only-check.sh --worktree`（在 Git Bash 里跑） | — |
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
| round 22（2026-11-27） | C 档实现：`session` lane（48 op / 122 行，容器 + rpc_result + 11 个 error_code + 换盐 + ack/new_session/req=0 + detailed-info 族 / msgs_all_info / msgs_state_info），门禁 23 → 24；台账 F124/F125 |
| round 23（2026-11-27） | 换面到 `td/telegram` 纯函数：`web_proxy` lane（26 case，secret 校验 / 0x70 标记 / address / HMAC bridge），门禁 24 → 25；台账 F126/F127 |
| round 24（2026-11-27） | 继续换面：`misc` lane（44 case，username / phone / offending / 用户名判定），门禁 25 → 26；台账 F128/F129；`clean_name`/`clean_input_string` 标 unmeasured |
| round 25（2026-11-27） | C 档收尾 + 判据形态换代：`session` 补 gzip_packed / pong / future_salts（72 op / 182 行，F130/F131）；新增 `tls_init` lane（16 固定扩展 × 8 样本/侧，结构比较，F132/F133），门禁 26 → 27 |
| round 26（2026-11-27） | 关掉 T63 的 unmeasured：misc lane 扩到 74 条 argv case + 6 条 stdin 长输入（`clean_name` / `strip_empty_characters` / `clean_input_string`，F134/F135）；门禁仍 27 项 |
| round 27（2026-11-27） | 第二类「判定函数」面：`DhHandshake::check_config` 的 32 条边界+随机语料（位数 / `p mod 4g` 表 / p 素性 / `(p-1)/2` 素性），Rust 侧自写 Miller-Rabin 与 OpenSSL 结论一致；门禁 27 → 28（F136/F137） |
| round 28（2026-11-27） | 格式驱动面：`RSA` lane 的 PEM/SPKI 解析（base64 + DER）、三种失败文案、指纹（TL 序列化 + SHA1 offset 12）、裸 `x^e mod n`；18 case，PEM 现场构造；门禁 28 → 29（F138/F139） |
| round 29（2026-11-27） | 补 PKCS#1（TDLib 自带 key 表用的就是它）+ 查表面：rsa 18 → 23 case，新增 public_rsa lane（main/test 两把 key 的指纹、请求指纹的顺序语义、`{...}` 文案）；门禁 29 → 30（F140/F141） |
| round 30（2026-11-27） | 动态表面：`PublicRsaKeySharedCdn` 的 add / get / drop / has（脚本驱动 21 行），换了 `DcId` 构造与生命周期；去重分支不可观察，记在 F142；门禁 30 → 31（F142/F143） |
| round 31（2026-11-27） | 所有权面：`tdutils::BufferSlice` 的 clone（别名）/ copy（深拷贝）/ `as_mutable_slice`（无 COW）/ confirm_read / truncate / after，脚本驱动 34 行；门禁 31 → 32（F144/F145） |
| round 32（2026-11-27） | 值面：`td/telegram/net/DcId` 的 `is_valid` / `create` / `from_value` + 派生谓词、`==` / `<`、`operator<<` 的五个分支，276 行脚本 / 273 行输出；CHECK 分支（`get_raw_id` / `internal` / `external`）记 unmeasured；门禁 32 → 33（F147/F148） |
| 已知缺口 | 七条，全部带 blocker：真实登录（`ready` 之后没有 MTProto 引擎，异步 lane 只覆盖未授权态）、`getOption commit_hash`（两侧各烘焙 HEAD）、第二次运行顺序（oracle 并发）、非空号码 `setAuthenticationPhoneNumber`（联网挂起）、`addLogMessage` level ≤ 0（oracle abort）、含非法 UTF-8 的语言包值（`td::JsonBuilder` 是 CHECK）、C ABI 之下。lane 内还有四处窄缺口：`pong` 的 `sent_destroy_auth_key_` 分支（探针不调 `destroy_key`，走不到）、`TlsInit::wait_hello_response` 的响应校验（需要 socket，F130/F132）、`ChainBufferReader` 的跨节点读（`prepare_read` 只给当前节点，`advance`/`move_as_buffer_slice` 在 clone 上 CHECK 失败，F146）、`DcId` 的 CHECK 分支（`get_raw_id`/`internal`/`external` 对非法输入 abort，进程级行为两侧运行时不同，F147）。`check_packet` 的 `Ignore too old or too new` 分支在 update 后恒真（F113）。gzip_packed / future_salts / pong 已在 T64 关闭，B1 的四段与 B7 也已关闭。**纯函数面无已知行为差异**（CONTROL 为空，改由管道自检接手） |

换目标或大改前先重跑 `tools/verify.py`；数字变了就把旧快照行置 `过期`，另起一行记新快照（1 = F044，2 = F052，3 = F056，4 = F058，5 = F061，6 = F064，7 = F076/F077，8 = F078，9 = F079，10 = F080，11 = F081，12 = F082，13 = F083，14 = F087/F088，15 = F090–F095，16 = F096–F098，17 = F113–F115，18 = F116/F117，19 = F118–F120，20 = F121，21 = F122/F123，22 = F124/F125，23 = F126/F127，24 = F128/F129，25 = F130–F133，26 = F134/F135，27 = F136/F137，28 = F138/F139，29 = F140/F141，30 = F142/F143，31 = F144/F145，32 = F147/F148）。注意 Rust dll 的 md5 **随 HEAD 变化**（`build.rs` 烘焙 commit hash），跨轮只比大小与 warning 数。

仪器自己加载运行时：`tools/tdjson_diff.py` 用 `os.add_dll_directory` 挂上 `C:/msys64/ucrt64/bin`——CPython 3.8+ 的 `CDLL` 走 `LOAD_LIBRARY_SEARCH_DEFAULT_DIRS`，`PATH` 不参与依赖查找，所以不必再导出 PATH。