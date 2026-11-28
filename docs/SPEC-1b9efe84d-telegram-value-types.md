# SPEC（as-built）— `td/telegram` 值类型面（`Usernames`、`MessageId`、`DialogId`、topic ids）

> **版本**：基座 commit `1b9efe84d`（master，判定基座）× oracle `libtdjson.dll` md5 `0398814038978aaeef7f8d6ac62bb8ff`（同一基座的 C++ 构建）。文件名里的 hash 就是版本：基座或 oracle 漂移 ⇒ 按新 hash 另起 `docs/SPEC-<新hash>-<area>.md`，本文件置「过期」，不原地改数字。
>
> **as-built 规格，不是 to-build。** 它只记录「已判过、数字可复现」的现状，供接手方当底图。
> 台账行：`docs/facts.md` F205–F223。

## 1. 定义域

| 项 | 值 |
|---|---|
| C++ 源 | `td/telegram/Usernames.{h,cpp}`（409 行）：构造分类、六个改值操作、`check_validness`、五个 `can_*` 谓词、`operator<<` |
| C++ 源（`MessageId`） | `td/telegram/MessageId.{h,cpp}`（253 行）+ `ServerMessageId.h` / `ScheduledServerMessageId.h` 的区间常量：三种构造、两个有效性区间、`get_type`、`prev/next` 进位、调度 date/server 拆解、`operator<<` |
| C++ 源（`DialogId`） | `td/telegram/DialogId.{h,cpp}`（211 行）+ `UserId.h` / `ChatId.h` / `ChannelId.h` / `SecretChatId.h` 的 MAX/MIN 常量：六个构造、五段负区间、四个取值、五个 `get_dialog_ids` 重载、`operator<<` |
| C++ 源（topic ids） | `td/telegram/ForumTopicId.h`（98 行）+ `td/telegram/SavedMessagesTopicId.{h,cpp}`（纯值部分）：`is_valid`/`get`/`==`/`operator<<`、`general()`、与 `MessageId` 的往返、`get_unique_id`/`is_author_hidden`、三种打印 |
| 传入结构 | `telegram_api::username`（生成类，判据只读 `username_`/`active_`/`editable_`/`deletable_`/`expired_` 五个字段） |
| 依赖 | `tdutils` 的 `check_utf8` 与 `StringBuilder`（vector 打 `{a, b}`）；**不碰** `Td`、actor、scheduler |
| 语言/构建 | C++（oracle：`C:/tmp/td-build/libtdcore.a` + `libtdapi.a`）对 Rust（`rust/src/bin/usernames_probe.rs`） |
| oracle 探针 | `C:/tmp/td-probe/usernames_probe.exe`，源码 `tools/fixtures/usernames_probe.cpp`，完整 g++ 命令行在文件头 |
| 判定基座 | C++ 提交 `1b9efe84d`；oracle 指纹即基座构建（`libtdjson.dll` md5） |

## 2. 可复现现状

```bash
cd C:/projects/td
uv run python tools/usernames_diff.py --selftest    # 21 case + 1 check / 146 行语料 / 242 行输出，0.9 s
uv run python tools/message_id_diff.py --selftest   # 8 case + 4 check / 268 行语料 / 430 行输出，1.1 s
uv run python tools/dialog_id_diff.py --selftest    # 7 case + 4 check / 199 行语料 / 302 行输出，0.9 s
uv run python tools/topic_id_diff.py --selftest     # 5 case（4 + 1 abort）/ 34 行语料 / 56 行输出，0.4 s
```

实测（2026-11-28）：`PASS cases=21 checks=1 lines=242`，`corpus md5=c0cb8ba7e1d426f9cac0550c443a8a43`；冷构建后首跑 16.6 s。四条判据全跑不短路（narration 逐行、FLOOR、abort 对、8 条手算 cell）。

## 3. 判据

| lane | 判据形态 | 规模（实测） | fact |
|---|---|---|---|
| `usernames` | narration 逐行 + FLOOR + abort 对 + 手算 cell | 21 case + 1 check / 242 行 | F205–F207 |
| `message_id` | narration 逐行 + FLOOR + 四条 abort 对 + 手算 cell | 8 case + 4 check / 430 行 | F208–F210 |
| `dialog_id` | narration 逐行 + FLOOR + 四条 abort 对 + 手算 cell | 7 case + 4 check / 302 行 | F212–F214 |
| `topic_id` | narration 逐行 + FLOOR + 一条 abort + 手算 cell | 5 case（4 + 1 abort）/ 56 行 | F221–F223 |
| `td_api_objects`（`get_usernames_object` / `get_message_id` 两段） | narration 逐行（JSON 逐字节）+ FLOOR + abort 对 + 手算 cell | 5 case + 1 check / 100 行（与 text-transforms 的 `get_outline_object` 共用一条 lane） | F234–F237 |
| `notification_sound`（通知声音） | narration 逐行（原始字节）+ FLOOR + 两条 abort 对 + 手算 cell | 9 case + 2 check / 134 行 | F249–F252 |
| `media_geometry`（`Dimensions` + `MediaAreaCoordinates`） | narration 逐行（64 位位型 + 六位小数 + `areaPosition` JSON）+ FLOOR + 两条 abort 对 + 手算 cell | 3 case + 2 check / 102 行 | F239–F242 |

判据句（每条都是验收）：

- `uv run python tools/usernames_diff.py --selftest` → `PASS cases=21 checks=1 lines=242`；
- `usernames_probe --check reorder_missing` → 两侧都 abort（`CHECK(result.has_editable_username())`，rc `3221226505`）；
- 同一份语料连跑两次 oracle → 输出逐行相同（FLOOR）。

语料覆盖的判据事实（F206）：

1. `first_username` 非空且 records 为空 → `active={first}`、`editable_username_pos_=0`；两者同时非空 → `LOG(ERROR)` 后留空对象（不 CHECK）。
2. `deletable` 先把该条 `editable` 清掉，名字仍进 `active`。
3. 只有第一条 `editable` 被采纳；后面的降级进 `other_editable`，并保持原 `active`/`disabled` 归属。
4. `active=0` 的 editable 走 `disabled` 数组下标 + `is_editable_username_disabled_`。
5. 打印格式 `Usernames[<editable>, active {...}, ...]`：**没有 editable 时仍带前导逗号**（`, active`）。
6. `check_validness` 任何非法（空名、非法 UTF-8、重复、位置越界）→ 整个对象作废成空。
7. `reorder_to` **不复制** `is_editable_username_disabled_`：disabled 可编辑对象 reorder 后 `pos` 指进 `active` → 越界 → 作废；reorder 丢掉 editable 用户名 → `CHECK` abort。
8. `toggle` 对未知名为安全早退（`can_toggle` 先闸，尾部 `UNREACHABLE` 走不到）；`add_secondary` 前插并偏移可编辑位置；`delete_secondary` 可经 `toggle(true, ...)` 复活可编辑名。

`MessageId` 的判据事实（F209）：

9. 普通 ID 只有「低 20 位为零」或「低 2 位是 1/2」才 valid；其余是 `invalid message <raw>`，scheduled 而类型不在 {4,5,6} 是 `scheduled invalid message <raw>`。
10. `operator<<` 的 local / yet-unsent 尾巴是 `. + 低 20 位原值`（`local message 1.2`），不是 local 序号。
11. 调度日期 = `(id >> 21) + 2^30`；调度 server id = `(id >> 3) & (2^18-1)`。
12. scheduled 的 `get_next_message_id` 不跨 server 边界（同类型 `id + 12`，升类型 `id - current + type`）；普通 ID 走 `get_next_server_message_id` 的进位。
13. `get_scheduled_server_message_id()` 先 `CHECK(is_scheduled_server())`；`is_any_server()` 对 scheduled-but-invalid 会 abort（探针里显式挡）。
14. 构造期两条 LOG 拒绝都不 CHECK：`send_date <= 2^30`、scheduled id 不在 `(0, 2^18)` 且 `force=false`，都留 0。

`DialogId` 的判据事实（F213）：

15. 五段负区间的端点开闭是核心：`[-999999999999, 0)` Chat、`[-1997852516352, -10^12)` Channel、`[-2002147483648, -2×10^12)` SecretChat、`[-4×10^12, …)` 剩余是 monoforum Channel；用户是 `(0, 2^40-1]`。
16. `-2×10^12` 本身**不是** secret chat（区间排除零 id），落到 monoforum 分支变成 Channel。
17. `-2002147483649`（比下界再小 1）不是 `None` 而是 Channel——monoforum 是最后的兜底。
18. 四个构造器都不 CHECK：非法 `UserId`/`ChatId`/`ChannelId`/`SecretChatId` 一律产出 0。
19. `operator<<` 恒为 `chat <raw id>`，与类型无关。

`ForumTopicId` / `SavedMessagesTopicId` 的判据事实（F222）：

20. `ForumTopicId::is_valid` 是 `> 0`，但 `operator<<` 对 0 和负数照样打 `topic N`；`general()` 就是 `ForumTopicId(1)`。
21. `from_top_thread_message_id` 取 `MessageId` 的 server id（`id >> 20`），`to_top_thread_message_id` 再造 `ServerMessageId`——往返后 `top=` 回到 `server message N`。
22. `MessageId` 的 `id == 0` 允许转 topic（`CHECK(id == 0 || is_server())`），其它非 server 值（`1000`、yet-unsent）abort。
23. `SavedMessagesTopicId` 三种打印：无效 `[no topic]`、`DialogId(2666000)` `[Author Hidden topic]`、其余 `[topic of <DialogId>]`；`get_unique_id` 就是内层 DialogId 的 raw id。

## 4. 不可判项 + 回流条件

| 项 | 原因 | 回流条件 |
|---|---|---|
| `store`/`parse` 序列化 | 需要 TL storer 与版本化字段布局，本 lane 只判值语义 | 接 `TlStorer` 的对拍 lane（与 `BinlogKeyValue` 的 store 同型） |
| 几何相邻值类型（`PhotoSizeSource` / `PhotoSizeType` / `Dimensions` 之外的媒体几何） | 需要更多 TL 对象与 `FileType` 装配 | 与对象面 lane 合流后再判 |
| `NotificationSound` 的 `store`/`parse` | 只接受 `LogEventStorer*`/`LogEventParser`，构造函数调 `set_context(G())`，`G()` 在非 actor 里 LOG_CHECK | 造出 `Global` 上下文或上游放开 CHECK |
| `peerNotifySettings` 的平台分支 | 编译期 `TD_ANDROID`/`TD_DARWIN_*` | 只判本机构建的 `other_sound_` 分支 |
| `Td` 依赖的调用方（`UserManager` 装配、`check_username` 流程） | 需要 `Td` 与网络/状态机 | 造出 `Td` 的 seam 或按 ABI 面 indirect 判 |
| 并发/跨线程可见性 | `Usernames` 是值类型，调用方负责同步 | 不需要（纯值语义） |
| `MessageId` 的两条 order 谓词（`is_message_id_order_ascending`/`descending`） | 需要一串 `telegram_api::Message`，判的是「哪一条先违约」的日志而不是返回值 | 造多违约语料并把日志文本纳入判据 |
| `MessageId` 的 storer/parser 模板 | 序列化只有 `store_long`/`fetch_long`，但要跨版本布局 | 与 `Usernames` 的 store/parse 一起做 |
| `get_message_dialog_id` 与 `Peer`/`DialogPeer` 构造 | 需要 TL 对象 | 与 `MessageId` 的 TL 面同一条 lane |
| `DialogId` 的 storer/parser | `store_long`/`fetch_long` | 同上 |
| 吃 `Td` 的 topic 方法（topic type object / input peer / input user） | 需要 `Td` 与 user/dialog manager | 造 `Td` 的 seam，或按 ABI 面 indirect 判 |
| `SavedMessagesTopicId(DialogId, MessageForwardInfo*, DialogId)` | 需要转发信息对象 | 与 topic type object 一起做 |

## 5. 失效条件

任一变化都要重编探针 + 重跑 lane + 重新落快照：

- **基座漂移**：判定基座不是 `1b9efe84d`（或 oracle md5 变）⇒ 本文件整体作废，按新 hash 另起一份；
- `td/telegram/Usernames.{h,cpp}`（构造分类、六个改值操作、`check_validness`、打印格式任一处）；
- `td/telegram/MessageId.{h,cpp}` 或 `ServerMessageId`/`ScheduledServerMessageId` 的区间常量；
- `td/telegram/DialogId.{h,cpp}` 或 `UserId`/`ChatId`/`ChannelId`/`SecretChatId` 的 MAX/MIN 常量；
- `td/telegram/ForumTopicId.h` / `SavedMessagesTopicId.{h,cpp}`（含 `HIDDEN_AUTHOR_DIALOG_ID` 常量）；
- `telegram_api::username` 的字段集或其 booleans 的含义；
- `tdutils` 的 `check_utf8` 或 `StringBuilder` 的 vector 打印格式；
- oracle 重建（`libtdcore.a` / `libtdapi.a` 换构建）。