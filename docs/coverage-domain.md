# 判据定义域（coverage domain）

只覆盖 `tdjson` C ABI 的 `td_execute` 纯函数路径。`measured` 需 test + fact；`unmeasured` 只占回流条件，不占 fact。

## 实体类 × 文本形态

实体类取自 `td/telegram/MessageEntity.cpp:1783-1810` 的 `find_entities` 调用序。

| 实体类 | ASCII | 非 ASCII / emoji | 边界（<4 字符 / 空文本） | blocker / 回流条件 |
|---|---|---|---|---|
| mention | measured（F005, F010） | measured（F007） | measured（F005, F010） | — |
| hashtag | measured（F009, F011） | measured（F007, F009） | unmeasured | 边界（1/256 字符、须含字母）：回流 T7 |
| botCommand | measured（F011） | unmeasured | unmeasured | 非 ASCII：回流 T7 |
| cashtag | measured（F011） | unmeasured | unmeasured | 非 ASCII / `@username` 后缀之外形态：回流 T7 |
| bankCardNumber | measured（F011） | — | measured（F011：非 Luhn 拒收） | 13–19 位各长度、各卡组织前缀：回流 T7 |
| tonAddress | unmeasured | — | unmeasured | 缺合法样本（伪造 base64 校验和不过）；回流：T7 |
| url（含 tgUrl / mailto） | **unmeasured** | unmeasured | unmeasured | Rust 未实现；回流：T7（`match_urls` 约 670 行） |
| emailAddress | **unmeasured（已知缺口）** | — | unmeasured | CONTROL 组证明两侧不同（F004/F008）；回流：T7 |
| mediaTimestamp | measured（F011） | — | unmeasured | `mm:ss` 与 `hh:mm:ss` 已测；边界（>59 秒、超长段）：回流 T7 |

注：`#tag@username` 后缀形态已入 corpus（F011）；`$CASHTAG@username` 后缀仍未入，记 unmeasured。

## action 面

| 项 | 状态 | blocker / 回流条件 |
|---|---|---|
| `getTextEntities` | measured（F004, F005, F011）；url/email 两类除外 | 缺口见上表 |
| `getOption` | unmeasured（Rust 未实现，CONTROL 可检出） | 回流：T4 |
| `td_api.tl` 其余 action | unknown（未枚举） | 回流：T4 枚举后建 corpus |

## 边界之外（永久不在本仪器范围）

| 模块 | 行数 | 原因 |
|---|---|---|
| `tddb` | 4,792 | 在 C ABI 之下，互操作需 C++ ABI |
| `td/mtproto` | 7,443 | 同上 |
| `tdactor` | 5,695 | 同上 |
| `tdutils` | 48,142 | 同上 |
| 有状态客户端路径 `td_send`/`td_receive` | — | 需 Telegram 测试 DC 账号；回流条件：拿到测试账号或用本地 MTProto 桩 |