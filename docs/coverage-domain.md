# 判据定义域（coverage domain）

覆盖 `tdjson` C ABI 的 `td_execute` 纯函数路径与客户端首启 lane（B1a）。`measured` 需 test + fact；`unmeasured` 只占回流条件，不占 fact。

## 实体类 × 文本形态

实体类取自 `td/telegram/MessageEntity.cpp:1783-1810` 的 `find_entities` 调用序。

| 实体类 | ASCII | 非 ASCII / emoji | 边界（<4 字符 / 空文本） | blocker / 回流条件 |
|---|---|---|---|---|
| mention | measured（F005, F010） | measured（F007） | measured（F005, F010） | — |
| hashtag | measured（F009, F011） | measured（F007, F009） | measured（F014：`#b#` 否决、>256 截断） | — |
| botCommand | measured（F011） | measured（F051） | measured（F057：`/`、`/a`、`/a b`、`/1`、`/_`、`/@bot`、`/start@`、词前缀否决） | — |
| cashtag | measured（F011） | measured（F051） | measured（F051：`$btc` 小写拒收、`$ABCDEFGHI` 过长拒收） | — |
| bankCardNumber | measured（F011） | — | measured（F051：12–19 位逐长度，非 Luhn 拒收；F057：Visa/MC/Amex/UnionPay/MIR 的组织长度表与边界前缀 2220/2221/2720/2204/2205） | — |
| tonAddress | measured（F013, F015） | — | measured（F051：坏校验和、短串、raw `0:<hex>`） | — |
| url（含 tgUrl / mailto） | measured（F008） | measured（F051） | measured（F051：IPv6、超长 path、无协议形态） | — |
| emailAddress | measured（F008） | — | measured（F051：非 ASCII 本地部分与域名） | — |
| mediaTimestamp | measured（F011） | — | measured（F051：`59:59`/`60:00`/`1:60`/`99:99:99`/`0:00`/`1:2:3`） | — |

注：`#tag@username` 后缀形态已入 corpus（F011）；`$CASHTAG@username` 后缀由 F051 覆盖。

## action 面

| 项 | 状态 | blocker / 回流条件 |
|---|---|---|
| `getTextEntities` | measured（F004, F005, F008, F011, F014） | — |
| `checkQuickReplyShortcutName` | measured（F017, F051：非 ASCII、ZWNJ、32/33 字符） | — |
| `getCountryFlagEmoji` | measured（F017, F051：非 ASCII 两字节输入按**字节**判长度） | — |
| `testReturnError` | measured（F017, F018） | — |
| `getFileMimeType` | measured（F019, F051：非 ASCII、超长名、多点扩展名） | — |
| `getFileExtension` | measured（F019, F051：大小写、反向缺失 MIME、空串） | — |
| `cleanFileName` | measured（F021, F051：64/65 字符 stem、16/17 字符 ext、纯符号名） | — |
| `getTextEmojiRanges` | measured（F020, F051：孤立 ZWJ、不完整序列、双肤色、标签序列） | — |
| `getJsonValue` / `getJsonString` | measured（F022, F051：重复键、空白、成员缺 `value`、未知 `@type`；F054：非法 JSON 文案；F060：±DBL_MAX 溢出与下溢，17 条） | — |
| `getThemeParametersJsonString` | measured（F023, F051：越界与负颜色、空对象） | — |
| `getChatFolderDefaultIconName` | measured（F024, F051：include/exclude/pinned 交叉、空 title） | — |
| `getPushReceiverId` | measured（F025, F051：非对象 payload、`data` 非对象、`p`/`user_id` 类型错配） | — |
| `getOption` | measured（F027：`version` 与两种拒绝） | `commit_hash` 绑定构建时 HEAD，两侧各自烘焙 → 仓库前进即不同，不构成可判 cell（unmeasured） |
| `parseTextEntities` | measured（F028, F029, F030, F051：`<tg-time>`/`<tg-emoji>`、未闭合标签、version 0/缺省） | — |
| `parseMarkdown` / `getMarkdownText` | measured（F031–F034, F051：v3 roundtrip 自检与降级路径） | — |
| `searchStringsByPrefix` | measured（F035, F051：大小写、重复项并列次序；F057：18 组抽样；F062：U+0020–U+FFFF 全表 1,658,474 次比较零分歧） | — |
| 8 个日志类 action | measured（F036, F037, F038, F053, F059；`SEQUENCE` 段 5 段 41 步，含隔离目录下的文件流、三条错误路径与 junction 路径） | — |
| `getLanguagePackString` | measured（F039：三道校验 + 无库 404） | 成功路径需要真实的语言包数据库文件（`tdlib-data`），回流条件 = 拿到 pack 数据后另立判据（见 backlog B7） |
| `searchQuote` | measured（F043, F051：负 offset / 越界长度实体、越界 quote_position） | — |
| `getPhoneNumberInfoSync` | measured（F041, F042, F051：`language_code` 无效，恒用内置 `en` 快照；`888` 前缀） | — |

## 请求层（29 个 action 共用）

`td/tl/tl_json.h` 的语义由全部同步 action 共享，单元格按「字段形态 × 期望类型」枚举。

| 项 | 状态 | blocker / 回流条件 |
|---|---|---|
| 缺失 / null 字段 → 默认值（`""` / 0 / null 对象） | measured（F046） | — |
| 类型错配文案：String / Number / Boolean / Array / Object 六种 `Expected X, but receive Y` | measured（F046） | — |
| int32 / int64 也接受 String 字面量；越界或脏字面量 → `Can't parse as an integer string "<literal>"` | measured（F046） | — |
| `@type`：缺失 / 未知字符串 / 未知构造号 / 已知但不同步 / 非 String-Integer | measured（F046, F047） | 数字形态只覆盖 `td_api.tl` 函数段（1114 条）内的构造号 |
| `@extra` 回显（含错误响应；数字保留原始字面量） | measured（F048） | — |
| 畸形 JSON 文本的 `json_decode` 报错文案（12 条消息 + 深度上限 + 尾部内容） | measured（F054） | — |
| 请求 / 字符串里的非法 UTF-8、孤立代理对（22 条 bytes cell，各 action 的拒绝文案逐条对齐） | measured（F063） | — |

## 客户端 lane（B1a：首次运行）

| 项 | 状态 | blocker / 回流条件 |
|---|---|---|
| 首启序列：`td_create_client_id` → `td_send(setTdlibParameters)` → drain 到 `waitPhoneNumber`（129 行 = 127 `updateOption` + 2 `updateAuthorizationState`） | measured（F076：`tools/client_diff.py`，A/A 地板 + 变异自检 + 两侧逐行相同） | — |
| 动态值 `unix_time` / `utc_time_offset` / `commit_hash` | measured（F076：语义校验——与实际时刻相差 ≤5 s、两侧时区偏移相同、各自可解释） | `commit_hash` 两侧各自烘焙 HEAD，不做相等比较 |
| `@client_id` 注入（id 从 1 起、每行携带） | measured（F076：两侧 id 都是 1 的断言） | — |
| 同一进程里的第二个 client | unmeasured | 会多 6 条全局 update 且顺序错位（F077）；判据按「一个进程一个 client」设计 |
| 第二次运行（同一 `database_directory`，只发变化项） | unmeasured | 需要「已下发值」kv（F072）；回流条件 = kv 实现后另立判据 |
| `waitPhoneNumber` 之后的登录状态机 | unmeasured | 需要实时验证码 + 2FA（F073），不可回归；回流条件 = 一次性实测记录 + 人工 |
| 其它 `setTdlibParameters` 参数 lane（`use_test_dc:false`、加密 key、语言 / 设备字段） | unmeasured | 回流条件 = 该 lane 有独立判据时 |

## 边界之外（永久不在本仪器范围）

| 模块 | 行数 | 原因 |
|---|---|---|
| `tddb` | 4,792 | 在 C ABI 之下，互操作需 C++ ABI |
| `td/mtproto` | 7,443 | 同上 |
| `tdactor` | 5,695 | 同上 |
| `tdutils` | 48,142 | 同上 |
| 客户端 lane 之下的 `tddb` 文件格式（binlog） | — | 判据是 `td_receive` 的输出，不是磁盘内容（F074），所以不实现 |