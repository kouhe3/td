# SPEC（as-built）— `td/telegram` 值类型面（`Usernames`、`MessageId`）

> **版本**：基座 commit `1b9efe84d`（master，判定基座）× oracle `libtdjson.dll` md5 `0398814038978aaeef7f8d6ac62bb8ff`（同一基座的 C++ 构建）。文件名里的 hash 就是版本：基座或 oracle 漂移 ⇒ 按新 hash 另起 `docs/SPEC-<新hash>-<area>.md`，本文件置「过期」，不原地改数字。
>
> **as-built 规格，不是 to-build。** 它只记录「已判过、数字可复现」的现状，供接手方当底图。
> 台账行：`docs/facts.md` F205–F210。

## 1. 定义域

| 项 | 值 |
|---|---|
| C++ 源 | `td/telegram/Usernames.{h,cpp}`（409 行）：构造分类、六个改值操作、`check_validness`、五个 `can_*` 谓词、`operator<<` |
| C++ 源（`MessageId`） | `td/telegram/MessageId.{h,cpp}`（253 行）+ `ServerMessageId.h` / `ScheduledServerMessageId.h` 的区间常量：三种构造、两个有效性区间、`get_type`、`prev/next` 进位、调度 date/server 拆解、`operator<<` |
| 传入结构 | `telegram_api::username`（生成类，判据只读 `username_`/`active_`/`editable_`/`deletable_`/`expired_` 五个字段） |
| 依赖 | `tdutils` 的 `check_utf8` 与 `StringBuilder`（vector 打 `{a, b}`）；**不碰** `Td`、actor、scheduler |
| 语言/构建 | C++（oracle：`C:/tmp/td-build/libtdcore.a` + `libtdapi.a`）对 Rust（`rust/src/bin/usernames_probe.rs`） |
| oracle 探针 | `C:/tmp/td-probe/usernames_probe.exe`，源码 `tools/fixtures/usernames_probe.cpp`，完整 g++ 命令行在文件头 |
| 判定基座 | C++ 提交 `1b9efe84d`；oracle 指纹即基座构建（`libtdjson.dll` md5） |

## 2. 可复现现状

```bash
cd C:/projects/td
uv run python tools/usernames_diff.py --selftest    # 21 case + 1 check / 146 行语料 / 242 行输出，7.3 s
uv run python tools/message_id_diff.py --selftest   # 8 case + 4 check / 268 行语料 / 430 行输出，28.8 s
```

实测（2026-11-28）：`PASS cases=21 checks=1 lines=242`，`corpus md5=c0cb8ba7e1d426f9cac0550c443a8a43`；冷构建后首跑 16.6 s。四条判据全跑不短路（narration 逐行、FLOOR、abort 对、8 条手算 cell）。

## 3. 判据

| lane | 判据形态 | 规模（实测） | fact |
|---|---|---|---|
| `usernames` | narration 逐行 + FLOOR + abort 对 + 手算 cell | 21 case + 1 check / 242 行 | F205–F207 |
| `message_id` | narration 逐行 + FLOOR + 四条 abort 对 + 手算 cell | 8 case + 4 check / 430 行 | F208–F210 |

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

## 4. 不可判项 + 回流条件

| 项 | 原因 | 回流条件 |
|---|---|---|
| `store`/`parse` 序列化 | 需要 TL storer 与版本化字段布局，本 lane 只判值语义 | 接 `TlStorer` 的对拍 lane（与 `BinlogKeyValue` 的 store 同型） |
| `get_usernames_object()` | 产出的是生成类 `td_api::usernames`，其 `to_string`/`store` 属生成代码面 | 要做 ABI 层对象判据时另立 lane（可借 `tdjson_diff` 的 `to_string` 面） |
| `Td` 依赖的调用方（`UserManager` 装配、`check_username` 流程） | 需要 `Td` 与网络/状态机 | 造出 `Td` 的 seam 或按 ABI 面 indirect 判 |
| 并发/跨线程可见性 | `Usernames` 是值类型，调用方负责同步 | 不需要（纯值语义） |
| `get_message_id(telegram_api::Message*)` 与两条 order 谓词 | 需要 TL `message`/`messageService`/`messageEmpty` 对象与 `to_string` 输出 | 接 `telegram_api` 对象的构造 lane（可复用 `tdjson_diff` 的 dump 面） |
| `MessageId` 的 storer/parser 模板 | 序列化只有 `store_long`/`fetch_long`，但要跨版本布局 | 与 `Usernames` 的 store/parse 一起做 |

## 5. 失效条件

任一变化都要重编探针 + 重跑 lane + 重新落快照：

- **基座漂移**：判定基座不是 `1b9efe84d`（或 oracle md5 变）⇒ 本文件整体作废，按新 hash 另起一份；
- `td/telegram/Usernames.{h,cpp}`（构造分类、六个改值操作、`check_validness`、打印格式任一处）；
- `td/telegram/MessageId.{h,cpp}` 或 `ServerMessageId`/`ScheduledServerMessageId` 的区间常量；
- `telegram_api::username` 的字段集或其 booleans 的含义；
- `tdutils` 的 `check_utf8` 或 `StringBuilder` 的 vector 打印格式；
- oracle 重建（`libtdcore.a` / `libtdapi.a` 换构建）。