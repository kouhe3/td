# Backlog

`/setup-matt-pocock-skills` 未安装（本仓库没有启用 mattpocock marketplace plugin），按 face-spaghetti §7 第 3 条退回到本文件。每条必须写「为什么暂缓 + 回流条件」。

| id | 条目 | 为什么暂缓 | 回流条件 |
|---|---|---|---|
| B1 | 有状态客户端路径：`td_send` / `td_receive` / `td_json_client_*` | 需要 Telegram 测试 DC 账号（`test/tdclient.cpp:236` 硬编码 `api_id=94575`，`:676` 硬编码测试手机号） | 拿到测试账号，或造本地 MTProto 桩让 `td_receive` 在离线可判 |
| B2 | C ABI 之下的模块：`tddb`(4,792) / `td/mtproto`(7,443) / `tdactor`(5,695) / `tdutils`(48,142) | 这些模块的互操作要匹配 C++ ABI（名字改编 / `td::string` / 模板），Rust 侧无法对象级替换 | 目标改为内部模块级等价时，另造进程内 C++↔Rust 桥并重判 gate |
| B3 | `#tag@username` 后缀、`/botCommand`、`$cashtag`、银行卡号、TON 地址、媒体时间戳 | 未入 corpus，Rust 侧未实现 | T3 扩 corpus 后逐类对齐 |
| ~~B4~~ | ~~`/setup-matt-pocock-skills` 产物缺失~~ **已解决**：`mattpocock-skills@1.3.2` 已按 project scope 装进 `.omp/plugins/`，setup 已跑，三个文件在 `docs/agents/`；本文件条目在变成可执行任务时开 issue（仓库 `kouhe3/td`） | — | — |
| ~~B5~~ | ~~Rust 侧工程不在本仓库（`C:/tmp/td-rs-probe`）~~ **已解决**：crate 移入 `rust/`（T6），仪器改指仓库内产物 | — | — |
| B6 | 共享 JSON 层对超出 int64 的数字字面量（如 `99999999999999999999`）在解析阶段就失败，回 `tdjson_rs: malformed request`；TDLib 走到 actions 层才回 `Can't parse as an integer string "..."`。两者都是 400，只是文案不同 | 为畸形输入改共享层语义，会波及所有 action；收益仅为错误文案 | 出现需要逐字节比对错误文案的判据时，给 `Json` 加“不可表示数字”变体并单独测 || B7 | `searchQuote`（引文定位）与 `getLanguagePackString` 的成功路径 | `searchQuote` 要移植 `MessageQuote::search_quote`（实体归并 + 定位），体量与 Markdown 家族相当；语言包成功路径需要真实的 pack 数据库文件（`tdlib-data`） | 需求指向引文高亮时做 B7；能提供语言包数据时把 `getLanguagePackString` 的成功分支另立判据 |
| ~~B8~~ | ~~`getPhoneNumberInfoSync`~~ **已解决**（T14）：解内嵌 zlib+TL blob 生成 `rust/src/country_table.rs`，9 条 cell 与 6260 例差分全一致 | — | — |
