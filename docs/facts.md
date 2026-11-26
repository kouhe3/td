# Facts

fact id 只增不减；行内改状态或划删除线，不重写行。

| id | claim | 等级 | 出处 | 状态 | 影响 |
|---|---|---|---|---|---|
| F001 | 该分支手写 C++ 面 = **488,184 行 / 1,566 文件**（排除 vendored `sqlite/`）；`td/` 392,287/1,077，`td/telegram` 376,066/992，最大单文件 `MessagesManager.cpp` 37,084 行 | L2 | `python -c "os.walk 统计 .cpp/.h/.hpp/.cc 行数"`；快照指纹 HEAD `1b9efe84d6e9a2e2f6eadeeea32b0726d2b8c25d` | 快照 | T0、T4 |
| F002 | **构建期生成面 = 493,956 行 / 44 文件**，由 `td/generate` 写入源码树 `td/generate/auto/`（被 `.gitignore` 的 `**/auto/` 覆盖），未构建时不存在 | L2 | `ninja tdjson` 后 `os.walk('td/generate/auto')`；同 F001 指纹 | 快照 | T0、T5 |
| F003 | `tdjson` 的 C ABI 共 **10 个导出符号**（`td_execute`/`td_send`/`td_receive`/`td_create_client_id`/`td_set_log_message_callback`/`td_json_client_create|send|receive|execute|destroy`），且 CMake 把它编成 **SHARED**，故 Rust `cdylib` 导出同符号集即对象级可替换 | L2/L4 | `td/telegram/td_json_client.h:62-183`；`CMakeLists.txt:1508`；实测 `libtdjson.dll` 与 `tdjson_rs.dll` 同进程加载成功 | 已证（机制） | T1、T2、T3 |
| F004 | 差分仪器可区分两实现：同一进程内 ctypes 加载两个 dll，**MATCH 4/4 逐字节一致**、**CONTROL 2/2 判为不同**（C++ 返回 `1.8.71` / email 实体，Rust 返回未实现 / 空） | L4 | `python tools/tdjson_diff.py`；oracle md5 `0398814038978aaeef7f8d6ac62bb8ff` | 已证（机制） | T1 |
| F005 | `getTextEntities` 丢弃**不足 4 字符**的 mention（`@a` → 空实体），长于等于 4 的保留；符号本身用 UTF-16 偏移 | L2/L4 | `td/telegram/MessageEntity.cpp:1388-1399`（`mention.size() >= 4` 门）；oracle 实测 `{"@type":"getTextEntities","text":"@a"}` → `entities:[]` | 已证（行为） | T3 |
| F006 | 同一 `request` 在同一 dll 上重复调用逐字节稳定（确定性地板 = 0 差异） | L4 | `python tools/tdjson_diff.py` 的 floor 段（每条 case 重复 3 次） | 已证（现象） | T1 |
| F007 | emoji/中文文本下 Rust 与 C++ 的 `offset`/`length` 在 UTF-16 语义上一致 | L4 | `python tools/tdjson_diff.py`：3 条非 ASCII cell 由 FAIL→PASS（`😀 @abcd` → offset 3/length 5；`中文 #标签` → 3/3；`#abc😀` → 0/4），MATCH 7/7 | 已证（行为） | T2 |
| F008 | 待证：`url` 与 `emailAddress` 两类实体尚未在 Rust 侧实现，CONTROL 组持续报差异（缺口可见）。原行覆盖六类，其中四类已由 F011 结清，故收窄 | 待定 | `python tools/tdjson_diff.py` CONTROL 段 → PASS=两侧不同 | 待证 | T7 |
| F009 | `#` 实体（hashtag）的字符集不是 ASCII：`is_hashtag_letter` = Unicode `Letter` 或 `DecimalNumber`，外加 `_`、U+200C、U+00B7；正文 1..256 字符，须含至少一个字母，且前一个字符不能是 hashtag 字母 | L2/L4 | `td/telegram/MessageEntity.cpp:392-402`（regex 注释 + `is_hashtag_letter`）；oracle 实测 `中文 #标签` → hashtag | 已证（机制） | T3 |
| F010 | `@` 实体（mention）规则：前后都必须是词边界、名字体 `[A-Za-z0-9_]` 长 2..32，且名字不足 4 字符时除非在短用户名白名单内否则丢弃 | L2/L4 | `td/telegram/MessageEntity.cpp:275-311` + `:1388-1399`；oracle 实测 `@a` → 空、`@abcd` → 保留 | 已证（机制） | T3 |
| F011 | cashtag / botCommand / bankCardNumber / mediaTimestamp / hashtag 的 `@username` 后缀五类，在 ASCII 语料上与 oracle **逐字节一致**（含 `$btc` 因小写被拒、`4276 3800 1234 5678` 因非 Luhn 被拒、`media_timestamp` 字段值 5025 与 83） | L2/L4 | `td/telegram/MessageEntity.cpp`（`match_cashtags` / `match_bot_commands` / `match_bank_card_numbers`+`is_valid_bank_card` / `match_media_timestamps`）；`python tools/tdjson_diff.py` MATCH 14/14；探针 `work/probe_entities.py` | 已证（行为） | T3 |
| F012 | 待证：多类实体同时出现时的排序与重叠裁剪（offset 升序、同行按 length 降序、重叠时保留先出现者）与 oracle 一致；需要 url/email 才能构造混合 cell | 待定 | 判据：`python tools/tdjson_diff.py` 加入混合 cell 后 MATCH 组逐字节相同 | 待证 | T7 |