# Plan — RIIR（JSON/C ABI 边界差分实现）

gate: passed | mode=直接做 | oracle=libtdjson.dll@0398814038978aaeef7f8d6ac62bb8ff | criteria=uv run python tools/verify.py | round=36 | status=running | date=2026-11-27

## 范围

只在 **`tdjson` C ABI / JSON 边界**上做 Rust 实现，并用「同一 case 喂两个 dll、逐字节比输出」判等价。

- 在范围内：`td_execute` 的 action/object 语义（先 `getTextEntities`，再按 `td_api.tl` 扩）。
- 不在范围内：`tddb` / `td/mtproto` / `tdactor` / `tdutils`（合计 6.4 万行）——它们在该边界之下，互操作要匹配 C++ ABI，不可行（记 `unmeasured`，见 `docs/coverage-domain.md`）。
- 不碰上游 C++ 源码；Rust 侧工程在本仓库 `rust/`（T6 已从 `C:/tmp/td-rs-probe` 移入），本仓库同时放台账、仪器、计划与实现。

## 进度（2026-11-27，round 32）

两个口径分开看，不要混成一句百分比。

**① `tdjson` C ABI 上可离线判定的部分——基本做完**

| 项 | 数字 | 复现 |
|---|---|---|
| 同步 action | 29/29 逐字节对齐 | `tools/tdjson_diff.py` 的 MATCH / SEQUENCE 段 |
| 全部函数的参数解析 | 1114/1114 | `tools/tdjson_diff.py` 的 `argument_cells()`（由 `td_api.tl` 生成） |
| 语言包成功路径 | 27 条 cell（含 ordinary / pluralized / deleted / 版本有无） | `tools/gen_language_pack.py` + `docs/facts.md` F087 |
| 随机语料 | 21 个无状态 action + 8 个日志 action 的随机序列；15 个种子 19,550 条 + 300 段/6,758 步零分歧（新增 2027–2030，F121） | `uv run python tools/tdjson_fuzz.py`、F095、F121 |
| 遗留 C ABI | `td_json_client_*` 5 个入口对齐（auth lane 143 行） | `tools/client_diff.py` 的 legacy 段、F090 |
| 异步请求路径 | 三支分流（本地解析错误 / 同步 action / 进 Td），16 条请求逐条相同 | `tools/client_diff.py` 的 async 段、F096–F097 |
| 客户端 lane | 3/4 对齐：首启、第二次运行、未授权状态机；真实登录只有 oracle 侧记录（F080） | `tools/client_diff.py`、`docs/coverage-domain.md` |
| 判据 cell 总数 | 1561 | `uv run python -c "import sys;sys.path.insert(0,'tools');import tdjson_diff as t;print(len(t.MATCH))"` |
| 协议 / RPC 面 | 25 条 lane：auth_key 14 字段、msg_layer 4、transport 6、framing 3 case、msgid 6 值 + 3 不变式、rpc 96 B、rpc_error 24 B、auth_data 3,026 步、svc_msg 41 case/344 B、proxy_secret 128 case、header 85 case × 2 头、session 72 op/182 行、web_proxy 26 case、misc 80 case、tls_init 16 扩展 × 8 样本/侧、dh_handshake 32 case、rsa 23 case、public_rsa 9 case、public_rsa_cdn 21 行、buffer_slice 34 行、dc_id 273 行、slice 119 行 + 17 check、slice_cmp 595 行、slice_prefix 578 行、slice_ptr 106 行 + 5 check | 各自的 `--selftest`、F103–F156 |
| 门禁 | 37 项（本轮新增 `slice_ptr`） | `uv run python tools/verify.py` |
| 定义域 | 84 个 measured 行、6 个 unmeasured（后者均不可判、不可达或缺引擎） | `grep -c '\| measured（' docs/coverage-domain.md` \| `grep -c '\| unmeasured \|' docs/coverage-domain.md` |

**② 作为「TDLib 的 Rust 重写」——远未开始**

| 项 | 数字 | 复现 |
|---|---|---|
| 手写 Rust | 8,708 行 | `wc -l rust/src/lib.rs` |
| 生成 / 数据表 | 13,324 行 | `wc -l rust/src/{api_functions,client_options,tl_schema,emoji_table,mime_tables,country_table}.rs` |
| 上游 C++（不含 `auto/`） | 380,965 行 | `find td -name '*.cpp' -o -name '*.h' \| grep -v /auto/ \| xargs wc -l \| tail -1` |

代码量比 2.3%，但这个数字不当进度用：`tddb` / `td/mtproto` / `tdactor` / `tdutils`（约 6.4 万行）一行没碰，客户端状态机只有「消息表回放」——真实登录的 222 条消息（F080）是录制来的，不是 Rust 侧算出来的。

**一句话**：「`tdjson` 的可判表面」约等于完成，「TDLib 本体」约等于没开始，中间隔的是 MTProto 引擎。

## 任务

| id | 任务 | 依据 | 状态 |
|---|---|---|---|
| T1 | 把探针升成仪器 `tools/tdjson_diff.py`：oracle md5 自检 + MATCH/CONTROL 双段 + 确定性地板 | F003, F004, F006 | done |
| T2 | 补 UTF-16 偏移 cell（emoji / 中文 / 组合字符） | F007 | done（7/7 MATCH；组合字符未测，留 T3） |
| T3 | 对齐其余实体类：cashtag / botCommand / bankCardNumber / mediaTimestamp / hashtag 后缀 | F011 | done（MATCH 14/14） |
| T7 | 移植 `match_urls`（约 670 行）+ `is_email_address` + `fix_url`，覆盖 url / email；补混合排序 cell | F008, F012 | done（6 条 cell 由 FAIL→PASS，含 1448 项 TLD 表） |
| T8 | 用 `tools/gen_ton_address.py` 生成合法样本，Rust 侧实现 base64url + crc16 + tag 校验 | F013, F015 | done（两种 tag 均 PASS） |
| T9 | hashtag 两条边界（`#b#` 否决、>256 截断）+ 非 ASCII 域名/本地部分样本 | F014 | done（`#b#` 与 300 字符两条 cell PASS） |
| T4 | 枚举 `td_execute` 可同步执行的 action 并建 corpus；先实现 3 个纯函数 | F016, F017, F018 | done（白名单 20 项；3 项已对齐，MATCH 33/33） |
| T10 | 同步 action 剩余 16 项：优先 `getFileMimeType`/`getFileExtension`/`cleanFileName`/`getTextEmojiRanges`（表驱动 + emoji 表），再 `getJsonValue`/`getJsonString`/`getThemeParametersJsonString`；`searchStringsByPrefix`（Hints 模糊搜索）与 `parseMarkdown`/`parseTextEntities` 单列 | F016, F018 | done（已由 T12/T13/T14 与后续批次全部完成；29/29 action 有 Rust 实现） |
| T5 | 决定生成面策略：不手写移植 49 万行生成代码；第一阶段按 action 手写薄层，第二阶段复用 Rust TL 代码生成器 | F002 | done（ADR-0002） |
| T13 | `searchQuote`：按引用白名单裁剪实体 + 从 `quote_position` 向两侧交替扫描字节级匹配 | F043 | done（8 条 cell） |
| T11 | 验证 `grammers-tl-gen` + `grammers-tl-parser` 能否直接吃 TDLib 的 `.tl` | F040 | done（可解析 3470/3473，需去注释预处理 + 补 3 条构造；结论写回 ADR-0002） |
| T14 | `getPhoneNumberInfoSync`：解内嵌 `help.countriesList` blob（zlib + TL 二进制，注意 TL 字符串是 1 字节长度前缀）→ 生成 `rust/src/country_table.rs`，实现清洗/最长匹配/patterns 格式化 | F041, F042 | done（9 条 cell + 6260 例差分为 0） |
| T12 | 8 个日志类 action 按「有序序列」判据接入：仪器加 `SEQUENCE` 段，两侧各跑同一串请求（set → get → 还原），逐项比结果；序列跑完恢复原值，避免污染其余 cell。依据探针 `work/probe_logs.py` 的实测：`getLogVerbosityLevel` 默认 5、`getLogStream` 为 `logStreamDefault`、未知 tag → 400 `Log tag is not found` | F036, F037, F038 | done（3 段 19 步序列全绿） |
| T6 | 把 Rust crate 从 `C:/tmp/td-rs-probe` 移进仓库 `rust/`，仪器改指仓库内产物 | F003 | done |
| T15 | 实体类未测形态收口：botCommand/cashtag 非 ASCII、`$CASHTAG@username` 后缀、bankCard 12–19 位、tonAddress 坏校验和与 raw 形态、url 的 IPv6/超长/非 ASCII 域名、email 非 ASCII、mediaTimestamp 边界 | F011, F014 | done（40 条 cell，F051） |
| T16 | action 面 A：mime/filename/emojiRanges/quickReply/countryFlag 的非 ASCII 与超长（含 C++ 按字节判长度的差异） | F017, F019, F020, F021 | done（F051） |
| T17 | action 面 B：json/theme/folderIcon/pushReceiver 的边界（重复键、越界颜色、include 交叉、`p` 长度）；`logStreamFile` 在隔离目录下另立判据 | F022–F025, F036–F038, F053 | done（`logStreamFile` 由 SEQUENCE 第 4 段 16 步覆盖，F053） |
| T18 | action 面 C：parseTextEntities 的 `<tg-time>`/`<tg-emoji>`、parseMarkdown v3 roundtrip 降级路径、searchStringsByPrefix 非 ASCII 归一化、searchQuote 越界裁剪、getPhoneNumberInfoSync 其它语言 | F028–F035, F043, F041 | done（F051） |
| T19 | 请求层对齐（原 B6）：`td/tl/tl_json.h` 的缺失/null 默认、类型错配文案、int32/int64 字面量、`@type` 四种错误、`@extra` 回显；CONTROL 换成畸形 JSON 文案（B9） | F046–F050 | done（339 PASS，failures=0） |
| T20 | 第二阶段生成面：按 ADR-0002 复用 TL 生成器产出 types/functions（F040 已验证）。B1 已收口；第一阶段（参数表 + 全部函数的参数解析）由 T32 完成，第二阶段仍缺判据——C ABI 只有 JSON，TL 二进制编解码在边界之下 | F040 | blocked（判据未定：生成的结构体验证什么） |
| T21 | 原 B9：把 `td::json_decode` 的解析器消息逐条移植（含深度上限、尾部内容、宽松数字前缀），顺带修 `getJsonValue` 的内层文案；CONTROL 改为 B10 一条 + 自检四断言 | F054, F055 | done（34 条 cell，instrument 645 PASS） |
| T22 | lane 尾巴：`/botCommand` 边界与词前缀否决、银行卡各卡组织长度表（Visa/MC/Amex/UnionPay/MIR 与 2220/2221/2720/2204/2205 边界）、searchStringsByPrefix 变音符号归一化 18 组 | F057 | done（57 条 cell，实现无需改动） |
| T23 | 原 B10：归因 `to_double` 的溢出语义（libstdc++ `num_get` 给 ±DBL_MAX）并夹紧 Rust 侧的 ±inf；修完 CONTROL 清空 | F060 | done（17 条数值 cell，instrument 793 PASS） |
| T24 | 符号链接 lane：junction 实测 `realpath` 不解析链接（`GetFullPathNameW`），词法实现无需改；链接由仪器自建并进 SEQUENCE | F059 | done（6 步序列） |
| T25 | 归一化全表：对 U+0020–U+FFFF 逐码点 × 27 查询扫 `searchStringsByPrefix`，1,658,474 次比较零分歧（把 F057 的 18 组抽样升级为全表） | F062 | done（一次性探针，结论进台账） |
| T26 | 非法 UTF-8 设计改动：`td_execute` 按字节读请求、`Json::Str` 改 `Vec<u8>`、孤立代理按 C++ 编原始三字节；各 action 的拒绝文案逐条对齐（`Text/Strings must be encoded in UTF-8`、`JSON has invalid encoding`、`cleanFileName` → 空、实体字段带类型名前缀） | F063 | done（22 条 bytes cell，instrument 837 PASS） |
| T27 | B1 切片评估：确认测试 DC 参数（全是编译期内置）、打通代理、抓离线基线、实测登录（测试 DC 的码规则过期，改走生产 DC + 既有会话代读码 + 2FA） | F065–F074 | done（生产 DC 端到端登录成功、`logOut` 清场，基线 `work/prod_baseline.txt`；实现范围已探明） |
| T28 | B1a 首启 lane：实现 `td_create_client_id`/`td_send`/`td_receive` 的最小客户端（收到 `setTdlibParameters` 后按序吐首启序列），option 表由 `tools/gen_client_options.py` 从 oracle 采集生成（3 个动态值运行时算）；判据 = `tools/client_diff.py` 两侧子进程采集的 raw 序列逐行对齐（白名单外零差异 + 变异自检 + A/A 地板） | F076, F074 | done（129 行逐行相同、`failures=0`；`verify.py` 九门禁全绿，`warnings=0`） |
| T29 | B1b 第二次运行 lane：在 `database_directory` 下写初始化标记，第二次运行改吐 125 条最终值表（`tools/gen_client_options.py` 生成第二张表）；判据 = `tools/client_diff.py` 的 second-run 段按**集合**比较（顺序不参与，F078） | F078, F072 | done（两侧集合相等、127 行 / 125 option、`failures=0`；九门禁全绿） |
| T30 | B1c 未授权状态机 lane：首个非 `setTdlibParameters` 请求补 6 条全局突发表 + 5 个请求的响应（空号码 / checkCode / checkPassword / getMe / logOut→Closed）；判据 = `tools/client_diff.py` 的 auth 段逐行比较（平铺 142 行） | F079 | done（142 行逐行相同、`failures=0`；九门禁全绿，17.5 s） |
| T31 | B1d 真实登录形态：生产 DC 六步共 222 行（一次性实测，不进回归）。Rust 侧不回放登录段——没有 MTProto 引擎，回放只能验证自己写死的表 | F080 | done（形态与清理过程进台账；等价性仍 `unmeasured`） |
| T32 | T20 第一阶段：`tools/gen_tl_schema.py` 从 `td_api.tl` 生成 3470 条定义的参数表（`rust/src/tl_schema.rs`，1114 函数 + 2356 构造）；Rust 在 `td_execute` 里对**全部函数**（含 29 个同步 action）先做 JSON 参数解析，抽象类要 `@type`、具体类不要；判据 = 1114 个 argument cell（MATCH 1528）逐字节相同 | F081 | done（`failures=0`；十项门禁全绿，17.7 s） |
| T33 | 补嵌套 `@type` 两条路径：抽象类字段接受构造号（新增 `CONSTRUCTOR_IDS` 表 + `Unknown constructor 0x…`）、具体类字段只要求是对象（「不校验字段」这半条被 F084 更正）；判据 = 6 条新 cell（`input_message_content` 的数字号/未知号/递归、`theme` 的错 `@type`/错字段/标量） | F082 | done（MATCH 1534、`failures=0`；十项门禁全绿） |
| T34 | B1e 参数 lane 普查：6 个 `setTdlibParameters` 变体（语言 ×2、设备、系统版本、应用版本、加密 key）对首启序列的影响 | F083 | done（全部 129 行相同、两次运行一致；唯一敏感项是 `use_test_dc`） |
| T35 | B7：`getLanguagePackString` 的成功路径。只读 `td/telegram/LanguagePackManager.cpp` 与 `tddb/td/db/SqliteKeyValue` 就能确定格式——库是调用方给的 SQLite 文件，表名 `"kv_<target>_<code>"`，值 `1<ordinary>` / `2<zero>\0…\0<other>` / 其它=deleted；`tdlib-data` 公开仓库已 404，且根本不需要它。判据 = `tools/gen_language_pack.py` 现场造两侧同名 fixture，27 条 cell 逐字节相同 | F087, F039 | done（`failures=0`；B7 关闭） |
| T36 | 随机语料 lane：`tools/tdjson_fuzz.py`——按 `td_api.tl` 结构化生成（21 个无状态 action）+ 手写语料的字节级变异，种子唯一决定语料，每轮打印 `corpus md5`，oracle 连跑两遍做 A/A 地板。判据 = 两次侧逐字节相同；接进 `verify.py` 做第 3 项门禁 | F088 | done（12 个种子 25,496 条零分歧；上线前抓到 4 类真实分歧：嵌套具体类字段、`null` 元素、`bool` 取数字、`return_none_for_empty_query`，见 F084–F086） |
| T37 | 遗留 C ABI：把 `td_json_client_create/send/receive/destroy` 从桩改成真实实现（自己的队列、不带 `@client_id`、`execute` 忽略句柄），并让畸形请求回一条 `400 Failed to parse request as JSON object: …`；判据 = `tools/client_diff.py` 新增 legacy 段（两侧各一子进程，auth lane 143 行逐行相同）+ oracle 内部一致性「遗留 lane == 现代 lane 去掉 `@client_id`」 | F090 | done（`failures=0`；`td_execute(NULL)` 也顺手对齐成空请求的解析错误） |
| T38 | 加深随机语料：构造号形态的 `@type`（用构建产物的 id 表）、嵌套深度 3、按 action 白名单注入非法 UTF-8、8 个日志 action 的 20 段自恢复随机序列。过程中发现两个生成器的构造号是错的（源码行 crc32 vs 规范化 crc32，422/3464 不同），改为读 `td/generate/auto/.../td_api.h` | F091–F095 | done（11 个种子 16,150 条 + 220 段/4,958 步零分歧；抓到 6 类真实分歧：id 表、两处 UTF-8 文案、`addLogMessage` abort、两处日志级别夹紧） |
| T39 | 异步请求路径：`td_send` 任意函数按三支分流——解析错误/同步 action/同步 option 本地作答，其余进 Td（未授权态回 401）；`@extra` 回显插在 `@client_id` 之前；`getOption` 走异步时非同步选项回 `optionValueEmpty`。顺带把 6 条全局 update 改成启动后自发（F097 更正 F079） | F096, F097 | done（`client_diff.py` 的 async 段 16 条请求逐条相同；首启/auth/legacy 三段仍绿） |
| T40 | 随机语料进门槛：`--seeds` 支持多种子，变异加入结构改动（改/删/加字段、容器压 null）；门禁跑 3 个种子 | F098 | done（2,550 条 + 60 段/1,351 步零分歧） |
| T41 | 目标定位（用户决定 2026-11-27）：首要目标是检验 face-spaghetti skill 本身，RIIR 是练习载体；此后不按「交付可上生产的 TDLib-RS」推进，覆盖数只作样本有效性 | F099 | done（定位写入 `docs/agents/face-spaghetti.md` 的「目标框架」） |
| T42 | 存储面第一条 lane：`tddb` binlog 读取器（`rust/src/bin/binlog_head.rs`）+ 仪器 `tools/binlog_diff.py`，判据 = 与 `binlog_dump` 的**整份输出**逐行相同（event 行含 `format::escaped` 的 data，尾部含 handler 汇总与 key trie）；顺带实证加密方案（首帧明文 `AesCtrEncryption`，其后 AES-256-CTR，默认 key = `raw("cucumber")`）与 processor 的 Rewrite 替换语义 | F100, F101 | done（438 行 = 147 event + 291 尾部、`diff_lines=0`；selftest 捕获篡改行；已进 `verify.py` 第 3 项门禁） |
| T43 | binlog 语料扩展：仪器加 `--all`，覆盖 `work/*/td_test.binlog` 全部 14 个文件（含真实登录的 24,576 B 库、含 Rewrite 的小库 10,128 B）；门禁改跑 `--all --selftest` | F102 | done（14/14 整份输出相同） |
| T44 | 协议面第一块：官方向量 samples-auth_key 步骤 1–4（resPQ 解析、pq 分解、`p_q_inner_data_dc` 序列化），Rust 侧 `rust/src/bin/auth_key_probe.rs` + `tools/auth_key_diff.py`；判据是**外部规范向量**，不是 C++ oracle | F103, F104 | done（4 个字段全等；RED→GREEN 已记） |
| T45 | 协议面第二块：`server_DH_params_ok` 解密与 `server_DH_inner_data` 解析（AES-256-IGE + SHA1 校验），字段 `answer_len/hash_ok/g/server_time/dh_prime/g_a` 对齐官方向量 | F105 | done（11 个字段全等） |
| T46 | 协议面第三块：`g_b` / `auth_key` 的 2048-bit 模幂与 `new_nonce_hash1` KDF，对齐样本步骤 7–9；至此 DH 握手（步骤 1–9）端到端对齐官方向量 | F106 | done（14 个字段全等） |
| T47 | 协议面第四块：MTProto 2.0 消息层（msg_key、KDF2、AES-256-IGE 封装）；没有官方向量，判据改为自建 C++ oracle（`tools/fixtures/msgprobe.cpp`，链接 TDLib 的 `KDF2` 与 `aes_ige_encrypt`）；Rust 侧 `rust/src/bin/msg_layer_probe.rs` + `tools/msg_layer_diff.py` | F107 | done（4 个字段全等） |
| T48 | 协议面第五块：完整包封装（salt / session_id / msg_id / seq_no / length / padding）；oracle = TDLib `Transport::write`，Rust 侧 `rust/src/bin/transport_probe.rs` 按 X=0 解回 | F108 | done（88 B 包、6 字段全等） |
| T49 | 协议面第六块：intermediate 传输 framing（magic、4 字节小端长度、quick-ack 位、padded 变体）；oracle = TDLib `tcp::IntermediateTransport`，Rust 侧 `rust/src/bin/framing_probe.rs` | F109 | done（3 个 case） |
| T50 | 协议面第七块：`seq_no` 状态机（逐值对齐）与 `msg_id` 生成（含随机，只判不变式：高 32 位 = `now << 32`、`% 4 == 0`、严格递增）；oracle = TDLib `AuthData`，Rust 侧 `rust/src/bin/msgid_probe.rs` | F110 | done（seq 6/6、两侧不变式均成立） |
| T51 | RPC 层第一块：请求头 `invokeWithLayer` + `initConnection`（手写 TL 编码，含 flags 与 params jsonObject）；oracle = TDLib `MtprotoHeader::get_default_header`，Rust 侧 `rust/src/bin/rpc_probe.rs` | F111 | done（96 B 逐字节相同） |
| T52 | RPC 层第二块：`rpc_error` 的双向互通（Rust 编码 ↔ TDLib 生成类的 `store`/`fetch`），并确认「生成代码只写字段、构造号由 boxed 外层写」 | F112 | done（24 B 字段、双向 roundtrip） |
| T53 | RPC 响应侧：`rpc_result#f35c6d01` / `msg_container#73f1f8dc` 解析与 `req_msg_id` 匹配 | B11 | blocked（无 oracle：`mtproto_api.tl` 里两个结构被注释、无生成类；TDLib 解析在 `SessionConnection` 且是 private，需要活连接） |
| T54 | 协议面第八块：`AuthData` 的 salt / 服务端时间差 / 入站出站 msg_id 有效性 / `check_packet` 状态机，用**显式注入的 `now`** 当模拟时钟；oracle = `tools/fixtures/auth_data_probe.cpp`，判据 = 同一脚本下两侧 stdout 逐字节相同（手写 3,026 行脚本 + 8 个随机脚本） | F113, F115 | done（`PASS scripts=9 steps=3026`；RED 由 `<`→`<=` 变异提供） |
| T55 | 协议面第九块：7 个维护类服务消息的生成编码与两个抽象分派点（`BadMsgNotification` / `MsgDetailedInfo`），含三种解析错误文案；oracle = `tools/fixtures/svc_msg_probe.cpp`，判据 = 41 个 case 双向 roundtrip 逐字节相同 | F114, F115 | done（`PASS cases=41 fields_bytes=344`） |
| T56 | 维护类消息的**策略**：`SessionConnection::on_packet(bad_msg_notification)` 的 `error_code`→动作表（16/20 重发、17 断连、18/19/32–35/64 失败、其它 unknown）与 `on_packet(bad_server_salt)` 的换盐 + 单条消息失败 | B11 | blocked（方法在 `SessionConnection` 且 private，需要 actor 与活 `RawConnection`；回流条件 = 造出 SessionConnection 级 oracle 或公开解析入口） |
| T57 | 协议面第十块：`ProxySecret` 全公开行为（`from_link` 三级回退、`from_binary` 长度规则与 truncate、`get_encoded_secret`、`get_proxy_secret`、`use_random_padding`、`emulate_tls`、`get_domain`）与三类错误文案；oracle = `tools/fixtures/proxy_secret_probe.cpp`，判据 = 41 个手写 + 87 个随机 case 逐字节相同 | F116, F117 | done（`PASS cases=128`、`corpus md5=a001043c88beac8cf144567798a7d769`；RED 由 17B 接受条件放宽提供） |
| T58 | 协议面第十一块：把 `MtprotoHeader` 从单点扩成选项矩阵（代理类型 × emulator × 语言包/语言码 × tz_offset × 非 ASCII/长字符串），同时比 default 与 anonymous；oracle = `tools/fixtures/header_probe.cpp`，判据 = 85 case 两份头逐字节相同 | F118, F119, F120 | done（`PASS cases=85`；RED 由 emulator 位 `1<<10`→`1<<11` 提供；顺带修掉 oracle 的 ANSI argv 坑，F119） |
| T59 | JSON 面随机语料扩种子：2027–2030 各 850 条 + 序列，记录语料指纹；门禁的 fuzz 项从 3 个种子加到 4 个（2027） | F121 | done（3,400 条 + 80 段/1,800 步零分歧；门禁 fuzz 项 4 种子） |
| T60 | C 档可判性探测：用假 `RawConnection` + 假回调驱动 `SessionConnection`，注入 rpc_result / bad_server_salt / bad_msg_notification(16/17/99) / 容器，看事件是否可复现 | F122, F123 | done（**可驱动**；6 个场景事件符合预期，两次运行逐字节相同；布局两坑记入 F123） |
| T61 | C 档实现：以 `session_probe` 为骨架造「可注入字节」的 oracle，再写 Rust 侧的 session 策略子集（rpc_result/容器解析、error_code→动作、换盐），并扩到 `msgs_ack` / `new_session_created` / `req_msg_id==0` / `msg_detailed_info` 族 / `msgs_all_info` / `msgs_state_info` | F122–F125 | done（`session_diff.py`：48 op/122 行、`PASS`；三次变异 RED：code 20 文案、new_session 参数、detailed-info 字段；gzip 与 pong/future_salts 仍未覆盖，记在 F124） |
| T62 | 换面到 `td/telegram` 纯函数：Web 代理的 `get_web_proxy_secret` / `get_web_proxy_link_secret` / `get_web_proxy_address` / `get_web_proxy_link_secret()`（带标记）/ `get_web_proxy_bridge_capability`（HMAC） | F126, F127 | done（`web_proxy_diff.py`：26 case、`PASS`；RED 由上下文常量 v1→v2 提供） |
| T63 | 继续换面：`td/telegram/misc.cpp` 的 `clean_username` / `clean_phone_number` / `replace_offending_characters` / `is_valid_username` / `is_allowed_username`（`clean_name` 与 `clean_input_string` 需要 utf8_truncate / 空格表 / emoji 规则，本轮标 unmeasured） | F128, F129 | done（`misc_diff.py`：44 case、`PASS`；两次变异 RED：小写映射、保留前缀表） |

| T64 | session lane 收 C 档剩余：`gzip_packed`（顶层递归 + rpc_result 内）、`pong`（含 too-low reset 分支）、`future_salts`（裸 vector/裸元素），判据 = 72 op / 182 行事件逐字节相同 | F130, F131 | done（`session_diff.py`：`PASS lines=182`；四次变异 RED：排序方向、递归 original、pong reset、rpc 内 gzip 尺寸） |
| T65 | 换面到 `TlsInit`：判据从「逐字节」降到「结构 + 固定段」——16 个固定扩展的归一化内容、2 个 GREASE、cipher 列表、SNI、ECH 长度集合、HMAC 自洽、总长 `1716+域长+32k` | F132, F133 | done（`tls_init_diff.py`：每侧 8 样本，`PASS extensions=16`；三次变异 RED：sigalgs 字节、HMAC 末 4 字节、GREASE 字） |
| T66 | 补 T63 的 unmeasured：`clean_name` / `strip_empty_characters` / `clean_input_string`，判据 = 74 条 argv case + 6 条 stdin 长输入（34996/34997/35000 字节边界）逐字节相同 | F134, F135 | done（`misc_diff.py`：80 case、`PASS`；三次变异 RED：NBSP 折叠、LENGTH_LIMIT-3、空格表首字节集） |
| T67 | 换形态到「随机/边界输入 + 素性判定」：`DhHandshake::check_config` 的四段（位数、`p mod 4g` 表、p 素性、`(p-1)/2` 素性），语料 32 条含 RFC 3526 group 14 与一个非 safe prime；Rust 侧 12 基 Miller-Rabin | F136, F137 | done（`dh_handshake_diff.py`：32 case、`PASS`，3.7 s；三次变异 RED：模表、半素数检查、位数检查） |
| T68 | 换面到格式驱动：`td/mtproto/RSA.cpp` 的 PEM/SPKI 解析（base64 + DER）、三种失败文案、`get_fingerprint`（TL 序列化 + SHA1 offset 12）、裸 `x^e mod n` 的 encrypt/decrypt；语料 18 条，PEM 由仪器现场构造 | F138, F139 | done（`rsa_diff.py`：18 case、`PASS`；三次变异 RED：模数长度、指纹偏移、`x>=n` 边界） |
| T69 | 补 PKCS#1 形态并换到查表面：rsa lane 加 PKCS#1 语料（18 → 23），新增 public_rsa lane（`PublicRsaKeySharedMain` 的 main/test 两把 key、指纹顺序语义、`{...}` 文案） | F140, F141 | done（`public_rsa_diff.py`：9 case、`PASS`；三次变异 RED：只查首个指纹、忽略 provider、括号形态） |
| T70 | 换到「动态表 + 生命周期」面：`PublicRsaKeySharedCdn` 的 `add_rsa` / `get_rsa_key` / `drop_keys` / `has_keys`，脚本驱动 22 行；Rust 侧只需指纹集合（返回的是请求指纹） | F142, F143 | done（`public_rsa_cdn_diff.py`：21 行、`PASS`；三次变异 RED：drop 不清空、只看首个 key、has 取反） |
| T71 | 换到内存所有权面：`tdutils::BufferSlice` 的 clone / copy / as_mutable_slice / confirm_read / truncate / after，脚本驱动 37 行；判据形态 = **别名可见性**（clone 共享、copy 隔离、无 COW） | F144, F145 | done（`buffer_slice_diff.py`：34 行、`PASS`；三次变异 RED：clone 变深拷贝、confirm_read 不推进、mutate 忽略 begin） |
| T72 | 尝试 `ChainBufferWriter` / `ChainBufferReader`：append / extract_reader / prepare_read / advance / clone / cut_head / sync_with_writer | F146 | blocked（`prepare_read` 只给当前节点且节点边界依赖分配策略；`advance` / `move_as_buffer_slice` 在 clone 上 CHECK 失败；探针撤回） |
| T73 | 换回小面：`td/telegram/net/DcId` 的静态 `is_valid` / `create` / `from_value` 边界与派生谓词（`is_empty` / `is_main` / `is_exact` / `get_value` / `==` / `<` / `operator<<`），脚本驱动 276 行；判据形态 = 逐字节（纯值面，无随机源） | F147, F148 | done（`dc_id_diff.py`：273 行、`PASS`；三次变异 RED：上界 `<`、create 回退、is_main 丢失） |
| T74 | 换到 `tdutils::Slice` 的视图边界：`remove_prefix` / `remove_suffix` / `substr`（含截断） / `truncate`（只缩不涨），脚本驱动 + 每例一进程的 CHECK 车道；判据形态 = 逐字节 + 判定（abort 与否） | F149, F150 | done（`slice_diff.py`：119 行 + 17 check、`PASS`，3 s；三次变异 RED：trunc 截断、substr2 少截 1、prefix 的 CHECK 收紧） |
| T75 | 换到同一文件的查找与比较：`find` / `rfind`（含 `npos`、首/末命中、空 slice）与 `==` / `!=` / `<`（公共前缀、**unsigned char** 序、前缀短者小）；纯值面，无 abort 分支 | F151, F152 | done（`slice_cmp_diff.py`：595 行、`PASS`，0.3 s；三次变异 RED：find 漏末字节、i8 序、equals 丢长度） |
| T76 | 收掉 `misc.h` 的前后缀判定：`begins_with` / `ends_with` 的 17×17 全对（超长 affix 返回 false、空 affix 恒 true 且走 one-past-end 零长 slice、含 `0x00`/`0x80`/`0xff` 载荷） | F153, F154 | done（`slice_prefix_diff.py`：578 行、`PASS`，0.3 s；三次变异 RED：begins 读尾、ends 读头、空后缀返 false） |
| T77 | 收 `Slice`/`MutableSlice` 的访问与指针：`back()`（尾部取值 + 空 slice 的 CHECK abort）、`operator[]`（合法域 `[0, size)`）、`data/begin/end/ubegin/uend` 四件套与 `remove_prefix` 后的整体平移；越界 `operator[]` 是 UB，记 unmeasured | F155, F156 | done（`slice_ptr_diff.py`：106 行 + 5 check、`PASS`，0.3 s；三次变异 RED：back 取头、index 忽略 begin、remove_prefix 双推） |

## 变更记录

| 日期 | 依据 | 受影响 | 决定 |
|---|---|---|---|
| 2026-11-26 | F003, F004 | T1 | 差分边界定为 `tdjson` C ABI；放弃 C++ 对象 API（名字改编 + `td::string` + 模板不可复现） |
| 2026-11-26 | —（配置变更，非行为） | 全部 | 装 `mattpocock-skills@1.3.2`（project scope）并跑 `/setup-matt-pocock-skills`：issue tracker = GitHub `kouhe3/td`（钉死，禁用 `tdlib/td`），triage 五标签已建，domain 文档 single-context |
| 2026-11-26 | F046–F050 | T19, T15–T18, T20 | 主分母（29/29 action）饱和后改用 lane cell 记账：`docs/coverage-domain.md` 里 15 条「回流 T10/T13/T14」已全部指向完成的任务，重编为 T15–T18；B6 落实为 T19 并关闭 |
| 2026-11-26 | F046, F049 | 仪器 | CONTROL 从两条类型错配（已对齐，必然转绿）换成两条畸形 JSON 文案（B9 未实现），并把原 CONTROL 两条移进 MATCH |
| 2026-11-26 | F053 | T17 | `logStreamFile` 走隔离目录 `work/logs`（仪器自己建目录并自检存在），序列跑完必须恢复 `logStreamDefault` |
| 2026-11-26 | F054, F055 | T21 | B9 关闭；CONTROL 收敛为 B10 一条，管道正确性改由自检四断言保证（不再用「未实现」造差异） |
| 2026-11-26 | F057 | T22 | 三条尾巴 lane 全部与 oracle 一致、无需改实现；`coverage-domain` 的 unmeasured 只剩「符号链接路径」「归一化全表」「非法 UTF-8」三处 |
| 2026-11-26 | F059, F060 | T23, T24 | 符号链接 lane 与浮点溢出都收掉；CONTROL 清空（当前无已知差异），`unmeasured` 只剩「归一化全表抽样」「非法 UTF-8」两处 |
| 2026-11-26 | F062 | T25 | 归一化改成全表扫描（码点 × 27 查询），`unmeasured` 只剩「非法 UTF-8 / 孤立代理对」一处，且它需要设计改动 |
| 2026-11-26 | F063 | T26 | 用户批准设计改动：请求按字节读、`Json::Str` 改字节串。实测 TDLib 是**逐 action 拒绝**而非透传，因此只需要「解析保留字节 + 各点校验」，不必改动作内部算法 |
| 2026-11-26 | F065–F074 | T27 | 测试 DC 的固定码规则实测已过期（F070）；改走**生产 DC**：验证码由既有 Telethon 会话代读、2FA 一次通过（F073）。范围也随之收窄——首次运行的 option 突发确定性（F074），不需要 binlog 格式，只需录制表 + kv（F072），B1 从「项目级未知」变成「有边界的实现任务」 |
| 2026-11-26 | F076 | T28, 仪器 | 用户批准 B1a：只做首次运行 lane（129 行）；第二次运行的 kv 与登录段留给后续轮次。判据从 `tdjson_diff.py` 扩到 `verify.py`（新增 CLIENT 段），gate 重判为 round 3 |
| 2026-11-26 | F087 | T35, 仪器 | B7 的 blocker 不成立：`getLanguagePackString` 的库由调用方给路径、格式在本仓库源码里，`tdlib-data` 更是已 404。改为 `tools/gen_language_pack.py` 现场造库；Rust 侧为此引入 `rusqlite`（bundled，零系统依赖），每侧读自己的 fixture 副本 |
| 2026-11-26 | F084–F086 | T33, T36, Rust 实现 | 随机语料抓到 4 类分歧，判据随之更严：具体构造名的字段**递归校验**（F084 更正 T33 的「不校验字段」）、`null` 在任何位置取默认（F086）、`bool` 接受数字与数字字符串（F085）。三处都是 Rust 侧错，oracle 侧未动 |
| 2026-11-26 | F090 | T37, 仪器 | `td_json_client_*` 从桩改成真实实现；判据不新造：先用 oracle 内部一致性证明「遗留 lane == 现代 lane 去掉 `@client_id`」，再拿两侧跑同一条 auth lane |
| 2026-11-26 | F091 | T38, 两个生成器 | 构造号改从构建产物 `td_api.h` 读。这是本轮最重的一处：之前的表让 Rust 认 422 个 oracle 不认的 id，手写语料查不出来（它们只用名字），只有随机语料用数字形态才暴露 |
| 2026-11-26 | F092–F094 | T38, Rust 实现 | 三处判据修正：`@type` 解码失败的文案不带前缀、字段级 UTF-8 失败带前缀、负日志级别的两种夹紧。`addLogMessage` level ≤ 0 是 oracle abort，排除出语料并记不可测（F093） |
| 2026-11-26 | F096, F097 | T39, 仪器 | 异步路径不再「静默丢弃」：按 Td 的真实分流实现三支，并把 6 条全局 update 从「首个请求触发」改成「启动后自发」。async 段只比每条请求自己的应答，突发归 auth 段——这样判据不受排空时机影响 |
| 2026-11-26 | F098 | T40, 门槛 | 随机语料从单种子扩到 3 个种子进门槛，变异加入结构改动；门槛总时长 34 s → 50 s 左右，仍是一分钟内 |
| 2026-11-27 | 用户决定（目标 B）；F099 | 全部 | 目标定为检验 face-spaghetti skill、RIIR 为练习；验收从覆盖率改为「skill 在本对象上是否有效 + 缺口落盘」，判据时间线的误记以 F099 勘误 |
| 2026-11-27 | F100 | T42, 仪器, Rust 依赖 | 新增存储面 lane：判据形态从「JSON 请求/响应」换成「文件 → 四栏文本」；binlog 默认加密，Rust 侧引入 `aes`/`ctr`/`hmac`/`pbkdf2`/`sha2` 五个纯 Rust 依赖（无系统依赖） |
| 2026-11-27 | F102 | T43, 仪器 | binlog 判据从单文件扩到全部 14 个 fixture（含真实登录库）；门禁里的 binlog 项改为 `--all --selftest` |
| 2026-11-27 | F103, F104 | T44, 仪器 | 换面到协议层：第一条 lane 用 Telegram 官方 auth_key 样本当判据（不依赖 C++ oracle）。流程缺陷：预注册与实现同批落账，已用 RED→GREEN 补齐并记在 F103/F104 |
| 2026-11-27 | F105 | T45 | 协议面第二块：AES-256-IGE + `answer_with_hash` 的哈希口径（只覆盖解析长度，不含对齐填充）落进 F105 |
| 2026-11-27 | F106 | T46, Rust 依赖 | 协议面第三块：引入 `num-bigint`（纯 Rust）做 2048-bit 模幂；DH 握手 1–9 全对齐官方向量 |
| 2026-11-27 | F107 | T47, 仪器 | 协议面第四块：消息层没有官方向量，改自建 C++ oracle（`msgprobe.cpp`，链接 `tdmtproto`/`tdutils` 静态库）；KDF2 的拆位规则落进 F107 |
| 2026-11-27 | F108 | T48, 仪器 | 协议面第五块：完整包封装；写 X=0 / 读 X=8 的方向差异与「prefix 由 storer 自己写」落进 F108 |
| 2026-11-27 | F109 | T49, 仪器 | 协议面第六块：intermediate framing；TDLib 无 abridged 分支，范围据此收窄并写进 F109 |
| 2026-11-27 | F110 | T50, 仪器 | 协议面第七块：seq_no 逐值对齐；msg_id 因 `Random::secure_int32` 只能判不变式——这是第三种判据形态（字节 / 字段 / 不变式） |
| 2026-11-27 | F111 | T51, 仪器 | RPC 层第一块：请求头手写 TL 编码对齐；oracle 链接 `tdcore`/`tdapi` 等七个静态库才链通，构建命令写进 `tools/fixtures/rpc_probe.cpp` 文件头 |
| 2026-11-27 | F112 | T52, 仪器 | RPC 层第二块：`rpc_error` 双向互通；「生成的 store/fetch 不含构造号」这条落进 F112，避免以后拿带 ID 的串去 parse |
| 2026-11-27 | F112, B11 | T53, coverage-domain | 收口：协议面与 RPC 面的 9 条 measured 行写进覆盖表；RPC 响应侧因无 oracle 标 blocked，回流条件写进 B11 |
| 2026-11-27 | F113 | T54, 仪器 | 协议面第八块改用**注入时钟**：所有 `now` 来自脚本，状态机因此可逐字节判；双精度一律按位型打印，避免 printf/format 差异被误读成分歧。`check_packet` 的 `Ignore too old or too new` 分支在 update 后恒真，记未覆盖（同 F110） |
| 2026-11-27 | F114 | T55, 仪器 | 协议面第九块覆盖「服务消息」：`rpc_result` / `msg_container` 仍被注释（T53 blocked），但 7 个生成类可用，抽象分派与三种解析错误文案一并入判据 |
| 2026-11-27 | F115 | T54, T55 | 两条 lane 的判据灵敏度由变异实测：`<`→`<=` 与构造号末位各改一次都被首个分歧行抓住，恢复后转绿 |
| 2026-11-27 | F113, B11 | T56, coverage-domain | `bad_msg_notification` 的 error_code→动作表仍是 blocked：机制在 `SessionConnection.cpp:344-386`（read-only）可读，但入口 private + actor，无法用探针驱动；只落回流条件，不占 fact |
| 2026-11-27 | F116 | T57, 仪器 | 协议面第十块换到纯函数：`ProxySecret` 的判据全在公开 API 上，且 base64 / hex 的边界规则本身是被测合同（三级回退顺序、去 padding 的长度规则），因此用「手写边界 + 随机字符串/字节语料」两条腿 |
| 2026-11-27 | F117 | T57 | 变异只改 17B 的接受条件（去掉 `dd` 检查），首个分歧落在 `link 00…00`；证明 lane 抓的是长度规则而不是「输出有变化」 |
| 2026-11-27 | F118 | T58, 仪器 | `MtprotoHeader` 从单点扩成矩阵：代理类型、emulator、语言包组合、tz_offset、非 ASCII 与 253/254/300 字节字符串；匿名头一并比 |
| 2026-11-27 | F119 | T58, 探针 | 非 ASCII 参数的假分歧来自 Windows ANSI argv（GBK）；oracle 探针改读宽命令行再转 UTF-8，判据才落到被测物上 |
| 2026-11-27 | F120 | T58 | emulator 位变异被抓在 `DIFF … 0 1 none`；证明 flags 的每一位都在判据里 |
| 2026-11-27 | F121 | T59, 门禁 | 随机语料扩 4 个新种子：人工 sweep 3,400 条零分歧，门禁只多纳入 1 个种子（2027），把门禁增量控制在几秒内 |
| 2026-11-27 | F122, F123 | T60, T61, coverage-domain | C 档的可判性门已过：`SessionConnection` 可用假传输驱动，C 档不再是「无 oracle」；`rpc_result`/容器的两个布局坑写进台账，避免下一个人按「标准 TL」注入而误判 |
| 2026-11-27 | F124 | T61, 仪器 | C 档实现收口：oracle 改成脚本化（`newconn`/`in`/`salt`），Rust 侧复刻解析与 11 个 error_code 分支；脚本里的 message id 带墙钟，指纹按去掉时间戳的模板算 |
| 2026-11-27 | F125 | T61 | 变异只改 code 20 的文案，首个分歧落在 `DIFF line 52`；证明策略表逐条在判据里 |
| 2026-11-27 | F124, F125 | T61 | C 档扩到 ack / new_session / req=0：41 op / 104 行；新增一次参数对调的变异，抓到 `DIFF line 99` |
| 2026-11-27 | F124, F125 | T61 | C 档再扩 detailed-info 族与 msgs_all_info / msgs_state_info：48 op / 122 行；第三次变异（detailed-info 字段对调）抓到 `DIFF line 109` |
| 2026-11-27 | F126 | T62, 仪器 | 换面到 `td/telegram`：Web 代理三个纯函数（含 0x70 标记形态与 HMAC bridge）入判据；Host 归一化（IDN）不做，只测与 host 无关的部分 |
| 2026-11-27 | F127 | T62 | 变异把 bridge 上下文从 v1 改成 v2，bridge 输出全变；证明这条判据绑的是常量与 HMAC 本身 |
| 2026-11-27 | F128 | T63, 仪器 | 继续换面到 `misc.cpp` 的轻量清洗函数；`clean_name` / `clean_input_string` 的复杂规则先留 unmeasured，避免为一条 lane 复制整套 UTF-8 表 |
| 2026-11-27 | F129 | T63 | 去掉小写映射的变异被抓在 `DIFF username 416243`；证明三条清洗规则各自在判据里 |
| 2026-11-27 | F128, F129 | T63 | misc lane 扩到用户名判定：`is_valid_username` / `is_allowed_username` 入判据（44 case），第二次变异（保留前缀表）抓到 `DIFF allowed_username` |
| 2026-11-27 | F130, F131 | T64 | session lane 补 gzip_packed（两层）、pong、future_salts：182 行；顺带记下 mtproto_api 的裸 vector/裸元素（`vector<future_salt>` 无 `0x1cb5c415`、元素无构造号）与「gzip 递归沿用外层 MsgInfo」 |
| 2026-11-27 | F132, F133 | T65 | 新增 tls_init lane：判据从逐字节降到「结构 + 固定段 + HMAC 自洽」，因为 ClientHello 的 session id / 两把 X25519 key / 1184 字节 ML-KEM / random 都新鲜且扩展顺序被打乱；门禁 26 → 27 |
| 2026-11-27 | F134, F135 | T66 | 关掉 T63 的 unmeasured：`clean_name` / `strip_empty_characters` / `clean_input_string` 入判据（74 + 6 条），顺带记下 `utf8_truncate` 用字节长度比字符数的怪行为；长输入走 stdin 绕开 argv 上限 |
| 2026-11-27 | F136, F137 | T67 | 新增 dh_handshake lane：判定函数按四段分支枚举边界 + 随机语料（32 条），Rust 侧自写 12 基 Miller-Rabin 与 OpenSSL 结论对齐；`g` 越界也报 `Bad prime mod 4g`，门禁 27 → 28 |
| 2026-11-27 | F138, F139 | T68 | 新增 rsa lane：PEM/SPKI（base64 + DER）解析 + 三种失败文案 + 指纹 + 裸 `x^e mod n`（18 case，PEM 现场构造）；指纹输入**不含构造号**（与 F112 同规则），门禁 28 → 29 |
| 2026-11-27 | F140, F141 | T69 | rsa 补 PKCS#1（`BEGIN RSA PUBLIC KEY`，TDLib 自带 key 表就用它）；新增 public_rsa lane（指纹顺序语义 + `{...}` 文案），门禁 29 → 30 |
| 2026-11-27 | F142, F143 | T70 | 新增 public_rsa_cdn lane：动态表的 add / get / drop / has（`DcId::external(2)`），去重分支不可观察记在 F142；门禁 30 → 31 |
| 2026-11-27 | F144, F145 | T71 | 新增 buffer_slice lane：clone 别名 / copy 深拷贝 / `as_mutable_slice` 无 COW / confirm_read / truncate / after（34 行）；顺带记下脚本字节参数的十进制坑，门禁 31 → 32 |
| 2026-11-27 | F146 | T72 | 尝试 ChainBuffer 跨节点读，三条路都堵（`prepare_read` 只给当前节点、`advance`/`move_as_buffer_slice` 在 clone 上 CHECK 失败），探针撤回、标 blocked；这本身是「先判可判性再写判据」的一次实践 |
| 2026-11-27 | F147, F148 | T73, 门禁 | 新增 dc_id lane：DcId 值面边界（`create` / `from_value` / `is_valid` + 派生谓词与打印），门禁 32 → 33；`get_raw_id` / `internal` / `external` 的 CHECK 分支不覆盖（进程级 abort，两侧运行时不同），记 unmeasured |
| 2026-11-27 | F149, F150 | T74, 门禁 | 新增 slice lane：`Slice` 的 `remove_prefix` / `remove_suffix` / `substr` / `truncate` 边界，脚本（合法域）+ `--check`（每例一进程，只比「是否 abort」）；门禁 33 → 34 |
| 2026-11-27 | F151, F152 | T75, 门禁 | 新增 slice_cmp lane：`Slice` 的 `find` / `rfind`（npos 打印成无符号值）与 `==` / `!=` / `<`（unsigned 序 + 前缀规则），595 行全值面；门禁 34 → 35 |
| 2026-11-27 | F134–F152 | coverage-domain | 回填 round 26–34 漏记的 8 条 lane（dh_handshake / rsa / public_rsa / public_rsa_cdn / buffer_slice / dc_id / slice / slice_cmp），并把 misc 的 case 数、`clean_name` 的状态、`rpc_result` 的 gzip/pong/future_salts 备注改成已判；定义域 73 → 82 measured、7 → 6 unmeasured，计数口径改成 `grep -c`（原来 `grep -o` 的口径会重复计数） |
| 2026-11-27 | F153, F154 | T76, 门禁 | 新增 slice_prefix lane：`misc.h` 的 `begins_with` / `ends_with`（超长 affix 短路、空 affix 走 one-past-end 零长 slice、头部/尾部窗口），578 行 17×17 全对；门禁 35 → 36 |
| 2026-11-27 | F155, F156 | T77, 门禁 | 新增 slice_ptr lane：`back()` / `operator[]`（合法域）与 `data`/`begin`/`end`/`ubegin`/`uend` 的字节偏移一致性，Slice 与 MutableSlice 双轨 + 两种移位；空 slice 的 `back()` 走 `--check` abort 判据，越界 `operator[]` 记 unmeasured（UB）；门禁 36 → 37 |