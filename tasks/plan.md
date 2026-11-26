# Plan — RIIR（JSON/C ABI 边界差分实现）

gate: passed | mode=直接做 | oracle=libtdjson.dll@0398814038978aaeef7f8d6ac62bb8ff | criteria=uv run python tools/verify.py | round=3 | status=running | date=2026-11-26

## 范围

只在 **`tdjson` C ABI / JSON 边界**上做 Rust 实现，并用「同一 case 喂两个 dll、逐字节比输出」判等价。

- 在范围内：`td_execute` 的 action/object 语义（先 `getTextEntities`，再按 `td_api.tl` 扩）。
- 不在范围内：`tddb` / `td/mtproto` / `tdactor` / `tdutils`（合计 6.4 万行）——它们在该边界之下，互操作要匹配 C++ ABI，不可行（记 `unmeasured`，见 `docs/coverage-domain.md`）。
- 不碰上游 C++ 源码；Rust 侧工程在本仓库 `rust/`（T6 已从 `C:/tmp/td-rs-probe` 移入），本仓库同时放台账、仪器、计划与实现。

## 进度（2026-11-26，round 14）

两个口径分开看，不要混成一句百分比。

**① `tdjson` C ABI 上可离线判定的部分——基本做完**

| 项 | 数字 | 复现 |
|---|---|---|
| 同步 action | 29/29 逐字节对齐 | `tools/tdjson_diff.py` 的 MATCH / SEQUENCE 段 |
| 全部函数的参数解析 | 1114/1114 | `tools/tdjson_diff.py` 的 `argument_cells()`（由 `td_api.tl` 生成） |
| 语言包成功路径 | 27 条 cell（含 ordinary / pluralized / deleted / 版本有无） | `tools/gen_language_pack.py` + `docs/facts.md` F087 |
| 随机语料 | 21 个无状态 action、12 个种子 25,496 条零分歧 | `uv run python tools/tdjson_fuzz.py`、F088 |
| 客户端 lane | 3/4 对齐：首启、第二次运行、未授权状态机；真实登录只有 oracle 侧记录（F080） | `tools/client_diff.py`、`docs/coverage-domain.md` |
| 判据 cell 总数 | 1561 | `uv run python -c "import sys;sys.path.insert(0,'tools');import tdjson_diff as t;print(len(t.MATCH))"` |
| 门禁 | 11 项（新增 fuzz） | `uv run python tools/verify.py` |
| 定义域 | 51 行 measured、4 行 unmeasured（后者均不可判或缺引擎） | `docs/coverage-domain.md` 每行状态 |

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