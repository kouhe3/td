# Backlog

`/setup-matt-pocock-skills` 未安装（本仓库没有启用 mattpocock marketplace plugin），按 face-spaghetti §7 第 3 条退回到本文件。每条必须写「为什么暂缓 + 回流条件」。

| id | 条目 | 为什么暂缓 | 回流条件 |
|---|---|---|---|
| B1 | 有状态客户端路径：`td_send` / `td_receive` / `td_json_client_*` | 需要 Telegram 测试 DC 账号（`test/tdclient.cpp:236` 硬编码 `api_id=94575`，`:676` 硬编码测试手机号） | 拿到测试账号，或造本地 MTProto 桩让 `td_receive` 在离线可判 |
| B2 | C ABI 之下的模块：`tddb`(4,792) / `td/mtproto`(7,443) / `tdactor`(5,695) / `tdutils`(48,142) | 这些模块的互操作要匹配 C++ ABI（名字改编 / `td::string` / 模板），Rust 侧无法对象级替换 | 目标改为内部模块级等价时，另造进程内 C++↔Rust 桥并重判 gate |
| B3 | `#tag@username` 后缀、`/botCommand`、`$cashtag`、银行卡号、TON 地址、媒体时间戳 | 未入 corpus，Rust 侧未实现 | T3 扩 corpus 后逐类对齐 |
| B4 | `/setup-matt-pocock-skills` 产物（issue-tracker / triage-labels / domain）缺失 | 仓库未启用 mattpocock marketplace plugin；skill 的 setup 只写自己的落点，不替它建 | 用户启用 plugin 后跑一次 setup，把本文件条目迁进 tracker |
| ~~B5~~ | ~~Rust 侧工程不在本仓库（`C:/tmp/td-rs-probe`）~~ **已解决**：crate 移入 `rust/`（T6），仪器改指仓库内产物 | — | — |