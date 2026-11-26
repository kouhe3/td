# Plan — RIIR（JSON/C ABI 边界差分实现）

gate: passed | mode=直接做 | oracle=libtdjson.dll@0398814038978aaeef7f8d6ac62bb8ff | criteria=python tools/tdjson_diff.py | round=2 | status=running | date=2026-11-26

## 范围

只在 **`tdjson` C ABI / JSON 边界**上做 Rust 实现，并用「同一 case 喂两个 dll、逐字节比输出」判等价。

- 在范围内：`td_execute` 的 action/object 语义（先 `getTextEntities`，再按 `td_api.tl` 扩）。
- 不在范围内：`tddb` / `td/mtproto` / `tdactor` / `tdutils`（合计 6.4 万行）——它们在该边界之下，互操作要匹配 C++ ABI，不可行（记 `unmeasured`，见 `docs/coverage-domain.md`）。
- 不碰上游 C++ 源码；Rust 侧工程在仓库外（`C:/tmp/td-rs-probe`），本仓库只放台账、仪器与计划。

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
| T16 | action 面 A：mime/filename/emojiRanges/quickReply/countryFlag 的非 ASCII 与超长（含 C++ 按字节判长度的差异） | F017, F019, F020, F021 | done（F051；空名/纯符号 botCommand 仍在 coverage-domain 记 unmeasured） |
| T17 | action 面 B：json/theme/folderIcon/pushReceiver 的边界（重复键、越界颜色、include 交叉、`p` 长度）；`logStreamFile` 在隔离目录下另立判据 | F022–F025, F036–F038, F053 | done（`logStreamFile` 由 SEQUENCE 第 4 段 16 步覆盖，F053） |
| T18 | action 面 C：parseTextEntities 的 `<tg-time>`/`<tg-emoji>`、parseMarkdown v3 roundtrip 降级路径、searchStringsByPrefix 非 ASCII 归一化、searchQuote 越界裁剪、getPhoneNumberInfoSync 其它语言 | F028–F035, F043, F041 | done（F051） |
| T19 | 请求层对齐（原 B6）：`td/tl/tl_json.h` 的缺失/null 默认、类型错配文案、int32/int64 字面量、`@type` 四种错误、`@extra` 回显；CONTROL 换成畸形 JSON 文案（B9） | F046–F050 | done（339 PASS，failures=0） |
| T20 | 第二阶段生成面：按 ADR-0002 复用 TL 生成器产出 types/functions（F040 已验证），前提是 B1 有结论 | F040 | blocked（B1） |
| T21 | 原 B9：把 `td::json_decode` 的解析器消息逐条移植（含深度上限、尾部内容、宽松数字前缀），顺带修 `getJsonValue` 的内层文案；CONTROL 改为 B10 一条 + 自检四断言 | F054, F055 | done（34 条 cell，instrument 645 PASS） |

## 变更记录

| 日期 | 依据 | 受影响 | 决定 |
|---|---|---|---|
| 2026-11-26 | F003, F004 | T1 | 差分边界定为 `tdjson` C ABI；放弃 C++ 对象 API（名字改编 + `td::string` + 模板不可复现） |
| 2026-11-26 | —（配置变更，非行为） | 全部 | 装 `mattpocock-skills@1.3.2`（project scope）并跑 `/setup-matt-pocock-skills`：issue tracker = GitHub `kouhe3/td`（钉死，禁用 `tdlib/td`），triage 五标签已建，domain 文档 single-context |
| 2026-11-26 | F046–F050 | T19, T15–T18, T20 | 主分母（29/29 action）饱和后改用 lane cell 记账：`docs/coverage-domain.md` 里 15 条「回流 T10/T13/T14」已全部指向完成的任务，重编为 T15–T18；B6 落实为 T19 并关闭 |
| 2026-11-26 | F046, F049 | 仪器 | CONTROL 从两条类型错配（已对齐，必然转绿）换成两条畸形 JSON 文案（B9 未实现），并把原 CONTROL 两条移进 MATCH |
| 2026-11-26 | F053 | T17 | `logStreamFile` 走隔离目录 `work/logs`（仪器自己建目录并自检存在），序列跑完必须恢复 `logStreamDefault` |
| 2026-11-26 | F054, F055 | T21 | B9 关闭；CONTROL 收敛为 B10 一条，管道正确性改由自检四断言保证（不再用「未实现」造差异） |