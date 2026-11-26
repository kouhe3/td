# 判据定义域（coverage domain）

只覆盖 `tdjson` C ABI 的 `td_execute` 纯函数路径。`measured` 需 test + fact；`unmeasured` 只占回流条件，不占 fact。

## 实体类 × 文本形态

实体类取自 `td/telegram/MessageEntity.cpp:1783-1810` 的 `find_entities` 调用序。

| 实体类 | ASCII | 非 ASCII / emoji | 边界（<4 字符 / 空文本） | blocker / 回流条件 |
|---|---|---|---|---|
| mention | measured（F005, F010） | measured（F007） | measured（F005, F010） | — |
| hashtag | measured（F009, F011） | measured（F007, F009） | measured（F014：`#b#` 否决、>256 截断） | — |
| botCommand | measured（F011） | unmeasured | unmeasured | 非 ASCII：回流 T7 |
| cashtag | measured（F011） | unmeasured | unmeasured | 非 ASCII / `@username` 后缀之外形态：回流 T7 |
| bankCardNumber | measured（F011） | — | measured（F011：非 Luhn 拒收） | 13–19 位各长度、各卡组织前缀：回流 T7 |
| tonAddress | measured（F013, F015） | — | unmeasured | 非法校验和 / raw 形态（`0:<hex>`）：回流 T9 |
| url（含 tgUrl / mailto） | measured（F008） | unmeasured | unmeasured | 非 ASCII 域名、IPv6、超长 path：回流 T9 |
| emailAddress | measured（F008） | — | unmeasured | 非 ASCII 本地部分：回流 T9 |
| mediaTimestamp | measured（F011） | — | unmeasured | `mm:ss` 与 `hh:mm:ss` 已测；边界（>59 秒、超长段）：回流 T7 |

注：`#tag@username` 后缀形态已入 corpus（F011）；`$CASHTAG@username` 后缀仍未入，记 unmeasured。

## action 面

| 项 | 状态 | blocker / 回流条件 |
|---|---|---|
| `getTextEntities` | measured（F004, F005, F008, F011, F014） | — |
| `checkQuickReplyShortcutName` | measured（F017） | 非 ASCII 名称：回流 T10 |
| `getCountryFlagEmoji` | measured（F017） | 非 ASCII 两字节输入（C++ 按**字节**判长度）：回流 T10 |
| `testReturnError` | measured（F017, F018） | — |
| `getOption` | unmeasured（Rust 未实现；CONTROL 靠它区分两实现） | 回流：T10（需先定可同步选项白名单） |
| `getFileMimeType` | measured（F019） | 非 ASCII 文件名、无扩展名之外形态：回流 T10 |
| `getFileExtension` | measured（F019） | 反向表未覆盖的 MIME：回流 T10 |
| `cleanFileName` | measured（F021） | 超长（>64 stem / >16 ext）、纯符号名：回流 T10 |
| `getTextEmojiRanges` | measured（F020） | 孤立 ZWJ、不完整序列、双肤色组合：回流 T10 |
| `getJsonValue` / `getJsonString` | measured（F022） | 重复键、超大指数、非法 JSON 的错误文案：回流 T10 |
| `getThemeParametersJsonString` | measured（F023） | 超出 24 位的颜色值、缺字段：回流 T10 |
| `getChatFolderDefaultIconName` | measured（F024） | 非白名单 icon 名与各 include 组合的交叉：回流 T10 |
| `getPushReceiverId` | measured（F025） | `p` 解不出 9 字节、`data` 非对象、负 `user_id`：回流 T10 |
| `getOption` | measured（F027：`version` 与两种拒绝） | `commit_hash` 绑定构建时 HEAD，两侧各自烘焙 → 仓库前进即不同，不构成可判 cell（unmeasured） |
| `parseTextEntities` | measured（F028, F029, F030） | 超出 int64 的 `version` 字面量走共享 JSON 层、错误文案不同（见 backlog B6）；`<tg-time>`、`<tg-emoji>` 等扩展标签未入 corpus：回流 T10 |
| `parseMarkdown` / `getMarkdownText` | measured（F031–F034） | `get_markdown_v3` 的 roundtrip 自检降级路径（构造失败输入）未入 corpus：回流 T10 |
| `searchStringsByPrefix` | measured（F035） | 非 ASCII 归一化（变音符号全表）、rating 并列时的次序未入 corpus：回流 T10 |
| 8 个日志类 action | measured（F036, F037, F038；`SEQUENCE` 段 3 段 19 步） | `logStreamFile`（写文件）未测：会落盘，回流条件 = 造隔离目录后另立判据 |
| 同步 action 其余 3 项（白名单共 29，见 F026） | unmeasured | `searchQuote` / `getLanguagePackString` / `getPhoneNumberInfoSync`：回流 T10 |

## 边界之外（永久不在本仪器范围）

| 模块 | 行数 | 原因 |
|---|---|---|
| `tddb` | 4,792 | 在 C ABI 之下，互操作需 C++ ABI |
| `td/mtproto` | 7,443 | 同上 |
| `tdactor` | 5,695 | 同上 |
| `tdutils` | 48,142 | 同上 |
| 有状态客户端路径 `td_send`/`td_receive` | — | 需 Telegram 测试 DC 账号；回流条件：拿到测试账号或用本地 MTProto 桩 |