# Plan — RIIR（JSON/C ABI 边界差分实现）

gate: passed | mode=直接做 | oracle=libtdjson.dll@0398814038978aaeef7f8d6ac62bb8ff | criteria=python tools/tdjson_diff.py | round=1 | status=running | date=2026-11-26

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
| T10 | 同步 action 剩余 16 项：优先 `getFileMimeType`/`getFileExtension`/`cleanFileName`/`getTextEmojiRanges`（表驱动 + emoji 表），再 `getJsonValue`/`getJsonString`/`getThemeParametersJsonString`；`searchStringsByPrefix`（Hints 模糊搜索）与 `parseMarkdown`/`parseTextEntities` 单列 | F016, F018 | planned |
| T5 | 决定生成面策略：不手写移植 49 万行生成代码；第一阶段按 action 手写薄层，第二阶段复用 Rust TL 代码生成器 | F002 | done（ADR-0002） |
| T11 | 验证 `grammers-tl-gen` + `grammers-tl-parser` 能否直接吃 TDLib 的 `.tl` | F040 | done（可解析 3470/3473，需去注释预处理 + 补 3 条构造；结论写回 ADR-0002） |
| T14 | `getPhoneNumberInfoSync`：解内嵌 `help.countriesList` blob（zlib + TL 二进制，注意 TL 字符串是 1 字节长度前缀）→ 生成 `rust/src/country_table.rs`，实现清洗/最长匹配/patterns 格式化 | F041, F042 | done（9 条 cell + 6260 例差分为 0） |
| T12 | 8 个日志类 action 按「有序序列」判据接入：仪器加 `SEQUENCE` 段，两侧各跑同一串请求（set → get → 还原），逐项比结果；序列跑完恢复原值，避免污染其余 cell。依据探针 `work/probe_logs.py` 的实测：`getLogVerbosityLevel` 默认 5、`getLogStream` 为 `logStreamDefault`、未知 tag → 400 `Log tag is not found` | — | planned |
| T6 | 把 Rust crate 从 `C:/tmp/td-rs-probe` 移进仓库 `rust/`，仪器改指仓库内产物 | F003 | done |

## 变更记录

| 日期 | 依据 | 受影响 | 决定 |
|---|---|---|---|
| 2026-11-26 | F003, F004 | T1 | 差分边界定为 `tdjson` C ABI；放弃 C++ 对象 API（名字改编 + `td::string` + 模板不可复现） |
| 2026-11-26 | —（配置变更，非行为） | 全部 | 装 `mattpocock-skills@1.3.2`（project scope）并跑 `/setup-matt-pocock-skills`：issue tracker = GitHub `kouhe3/td`（钉死，禁用 `tdlib/td`），triage 五标签已建，domain 文档 single-context |