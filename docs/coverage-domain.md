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
| 同步 action 其余 12 项 | unmeasured（已枚举，见 F016） | 逐项建 corpus：T10 |

## 边界之外（永久不在本仪器范围）

| 模块 | 行数 | 原因 |
|---|---|---|
| `tddb` | 4,792 | 在 C ABI 之下，互操作需 C++ ABI |
| `td/mtproto` | 7,443 | 同上 |
| `tdactor` | 5,695 | 同上 |
| `tdutils` | 48,142 | 同上 |
| 有状态客户端路径 `td_send`/`td_receive` | — | 需 Telegram 测试 DC 账号；回流条件：拿到测试账号或用本地 MTProto 桩 |