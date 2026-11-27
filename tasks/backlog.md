# Backlog

`/setup-matt-pocock-skills` 未安装（本仓库没有启用 mattpocock marketplace plugin），按 face-spaghetti §7 第 3 条退回到本文件。每条必须写「为什么暂缓 + 回流条件」。

| id | 条目 | 为什么暂缓 | 回流条件 |
|---|---|---|---|
| B1 | 有状态客户端路径：`td_send` / `td_receive` / `td_json_client_*`。**前置已全部满足**：网络（F067）、DC/钥匙/api_id（F065）、凭据与 2FA（F073：生产 DC 端到端登录到 `authorizationStateReady`，码由既有会话代读）；实现范围也已探明——首次运行的 option 突发是确定性的（F074），**不需要实现 binlog 文件格式**，只需录制表 + 「已下发值」kv（F072） | 只剩实现工作量：请求/响应框架 + 未授权/授权状态机 + option 表与 kv 存储；没有外部阻塞 | 起步条件：用户批准范围 A（录制 option 表 + kv 存储 + 最小状态机，判据 = oracle 的 `td_receive` 序列逐条对齐）；测试 DC 的登录问题不影响这条路线（用生产 DC） |
| B2 | C ABI 之下的模块：`tddb`(4,792) / `td/mtproto`(7,443) / `tdactor`(5,695) / `tdutils`(48,142) | 这些模块的互操作要匹配 C++ ABI（名字改编 / `td::string` / 模板），Rust 侧无法对象级替换 | 目标改为内部模块级等价时，另造进程内 C++↔Rust 桥并重判 gate |
| ~~B3~~ | ~~`#tag@username` 后缀、`/botCommand`、`$cashtag`、银行卡号、TON 地址、媒体时间戳~~ **已解决**：六类全部入 corpus 并与 oracle 一致——`#tag@user`/botCommand/cashtag/mediaTimestamp（F011、F051）、TON（F013, F015）、银行卡含卡组织长度表（F051、F057） | — | — |
| ~~B4~~ | ~~`/setup-matt-pocock-skills` 产物缺失~~ **已解决**：`mattpocock-skills@1.3.2` 已按 project scope 装进 `.omp/plugins/`，setup 已跑，三个文件在 `docs/agents/`；本文件条目在变成可执行任务时开 issue（仓库 `kouhe3/td`） | — | — |
| ~~B5~~ | ~~Rust 侧工程不在本仓库（`C:/tmp/td-rs-probe`）~~ **已解决**：crate 移入 `rust/`（T6），仪器改指仓库内产物 | — | — |
| ~~B6~~ | ~~共享 JSON 层对超出 int64 的数字字面量与类型错配的文案与 TDLib 不同~~ **已解决**（T19）：`Json::Num` 变体 + `request_*` 逐字段类型校验，按 `td/tl/tl_json.h` 复刻六种错配文案、缺失/null 默认、`@type` 四种错误与 `@extra` 回显（F046–F048, F050） | — | — |
| B7（收窄） | 只剩 `getLanguagePackString` 的**成功路径**（`searchQuote` 已由 T13 完成） | 需要真实的语言包数据库文件（`tdlib-data`）；错误路径已由 F039 覆盖 | 能提供 pack 数据时把成功分支另立判据 |
| ~~B9~~ | ~~畸形 JSON 文本的 `json_decode` 报错文案未移植~~ **已解决**（T21）：12 条解析器消息 + 深度上限 100 + 尾部内容 + 宽松数字前缀全部移植，34 条 cell 逐字节相同（F054）；CONTROL 随之收敛为 B10 一条 + 自检四断言（F055） | — | — |
| ~~B10~~ | ~~超 DBL_MAX 的浮点字面量在 `getJsonValue` 里分叉~~ **已解决**（T23，F060）：机制是 libstdc++ `num_get` 溢出时给 ±DBL_MAX，Rust 侧在 `to_double_prefix` 夹紧 ±inf；17 条 cell 对齐，CONTROL 随之清空（当前无已知差异） | — | — |
| ~~B8~~ | ~~`getPhoneNumberInfoSync`~~ **已解决**（T14）：解内嵌 zlib+TL blob 生成 `rust/src/country_table.rs`，9 条 cell 与 6260 例差分全一致 | — | — |
| B11 | RPC 响应侧：`rpc_result#f35c6d01` 的 `req_msg_id` 匹配、`msg_container#73f1f8dc` 拆包、未知 msg_id 与重复应答 | 两个结构在 `mtproto_api.tl` 里是注释掉的，没有生成类；TDLib 在 `SessionConnection.cpp:239-300,521` 手写解析且为 private，要活的连接对象（网络 + 时钟 + 认证）才能驱动 | 造出 SessionConnection 级 oracle（模拟连接 + 固定时钟），或 TDLib 暴露公开解析入口 |
