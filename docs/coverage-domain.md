# 判据定义域（coverage domain）

覆盖 `tdjson` C ABI 的 `td_execute` 路径（29 个同步 action + 全部 1114 个函数的参数解析）、客户端 lane（首次运行、第二次运行、未授权状态机、真实登录记录）与 binlog 存储面（读侧）。`measured` 需 test + fact；`unmeasured` 只占回流条件，不占 fact。

## 判定基座

判据来自实现的一份快照，不跟随上游日历：每条 gate 都对应下面这组 C++ 源码与 oracle 二进制，快照的 `commit` / `worktree_dirty` / `cpp_oracle_md5` 由 `uv run python tools/verify.py --json` 现场写入。

| 项 | 值 |
|---|---|
| C++ 基座 | `1b9efe84d6e9a2e2f6eadeeea32b0726d2b8c25d`（2026-11-23） |
| 上游最新（记账时） | `e7ac61dd69409c31b3c8c7367b33b07eecce701f`（2026-11-26），领先 18 提交，本轮不同步（F173） |
| oracle | `C:/tmp/td-build/libtdjson.dll`，md5 pin 在 `tools/tdjson_diff.py:54`；协议面与纯函数面另用 `C:/tmp/td-probe/*.exe` |
| 重实现 | 分支 `rust` 的 HEAD；`worktree_dirty=false` 时 commit 才代表被测源码 |

同步（`git merge upstream/master` + `ninja tdjson` + 重编探针 + 更新 md5 pin + 重跑 48 项 + 重新落快照）只在下列任一成立时触发：

1. 上游改动与本文件点名的源文件相交；
2. 上游改动 `td_api.tl` 的定义行，或 mime / emoji / country 数据表；
3. 新开 lane 的源文件已被上游改过。

其余情况只在 `docs/facts.md` 追加一条漂移记录（F173 是第一条）。

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
| 8 个日志类 action | measured（F036, F037, F038, F053, F059, F094；`SEQUENCE` 段 6 段 51 步，含隔离目录下的文件流、三条错误路径与 junction 路径；另加随机序列 20 段/444 步） | `addLogMessage` 的 level ≤ 0 会让 oracle abort（F093），不可判 |
| `getLanguagePackString` | measured（F039：三道校验 + 无库 404；F087：成功路径 27 条 cell——ordinary / pluralized / deleted / 版本有无 / 表名不匹配 / 打不开的库，fixture 由 `tools/gen_language_pack.py` 现场生成） | — |
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
| 全部 1114 个函数的参数解析（含 29 个同步 action）：按 TL 定义顺序、缺失/`null` 取默认、多余字段忽略 | measured（F081：1114 个 argument cell，MATCH 1528） | — |
| 嵌套对象：抽象类字段必须带 `@type`（缺失 / 未知两种文案）、具体类字段不需要 | measured（F081：`sendMessage.input_message_content`、`getThemeParametersJsonString.theme` 两条样例 + 递归校验） | — |
| 嵌套 `@type` 用构造号（数字） | measured（F082：正确号接受、未知号 `Unknown constructor 0x00003039`，新增 `CONSTRUCTOR_IDS` 表） | — |
| `@type` 构造号与函数号取自**构建产物**的 id 表 | measured（F091：源码行 crc32 与规范化 crc32 在 422/3464 个定义上不同；两个生成器改读 `td/generate/auto/.../td_api.h` 后，21 个 action 的随机 id 形态零分歧） | — |
| 具体类字段：`@type` 不校验（可以是任意值或缺失），**但已声明的字段递归校验** | measured（F082 前半 + F084 更正：F082 的三条样本用的都是不存在的字段名 `bg_color`；`folder.name.text:true` → `Expected Object, but receive Boolean`） | — |
| `null` 在任何位置都是「取默认值」，包括 `vector<T>` 的元素自身与 vector 字段 | measured（F086） | — |
| `bool` 字段接受数字与数字字符串（回退 `from_json(int32)`），失败文案仍报原类型 | measured（F085：`include_contacts:"true"` → `Expected Boolean, but receive String`；`include_bots:1` 接受） | — |
| 随机语料（21 个无状态 action 的结构化生成 + 手写语料的字节变异） | measured（F088：12 个种子共 25,496 条零分歧；抓到的 4 类真实分歧——嵌套具体类字段、`null` 元素、`bool` 取数字、`return_none_for_empty_query`——已修，见 F084–F086） | 8 个有状态日志 action 只能按有序序列判 |

## 客户端 lane（首次运行、第二次运行、未授权状态机、真实登录记录）

| 项 | 状态 | blocker / 回流条件 |
|---|---|---|
| 首启序列：`td_create_client_id` → `td_send(setTdlibParameters)` → drain 到 `waitPhoneNumber`（129 行 = 127 `updateOption` + 2 `updateAuthorizationState`） | measured（F076：`tools/client_diff.py`，A/A 地板 + 变异自检 + 两侧逐行相同） | — |
| 遗留 C ABI：`td_json_client_create/send/receive/destroy/execute`（auth lane 143 行 = 现代表去掉 `@client_id` + 1 条畸形请求回包） | measured（F090：`tools/client_diff.py` 的 legacy 段；oracle 内部一致性「遗留 lane == 现代 lane 去掉 `@client_id`」也一并断言） | `destroy` 之后再用句柄是 UB，不测 |
| 异步请求路径（`td_send` 任意函数）：本地可答的（解析失败、同步 action、同步 option）与进 Td 的（未授权 401）分三支，`@extra` 回显在 `@client_id` 之前 | measured（F096, F097：`tools/client_diff.py` 的 async 段，16 条请求逐条相同；`commit_hash` 两侧各自烘焙，按语义比较） | 进 Td 的函数只覆盖未授权态（`waitPhoneNumber`）；登录后的应答不在范围内 |
| 动态值 `unix_time` / `utc_time_offset` / `commit_hash` | measured（F076：语义校验——与实际时刻相差 ≤5 s、两侧时区偏移相同、各自可解释） | `commit_hash` 两侧各自烘焙 HEAD，不做相等比较 |
| `@client_id` 注入（id 从 1 起、每行携带） | measured（F076：两侧 id 都是 1 的断言） | — |
| 同一进程里的第二个 client | unmeasured | 会多 6 条全局 update 且顺序错位（F077）；判据按「一个进程一个 client」设计 |
| 第二次运行（同一 `database_directory`）：127 行 = 125 个 option + 2 状态，值取最终态（首启的 3/5 中间值不再出现） | measured（F078：`tools/client_diff.py` second-run 段，**按集合**比较 + A/A 集合地板 + 变异自检） | 逐行顺序见下一行 |
| 第二次运行的 option 顺序 | unmeasured | oracle 的 run2/3/4 两两不同（首个差异在第 4 行），并发更新顺序不可判；Rust 侧按名字排序输出，判据只比集合 |
| 首个非 `setTdlibParameters` 请求的 6 条全局 update 突发（一次性） | measured（F079：auth 段，`getMe` 与空号码都触发） | — |
| 未授权状态机的响应：空号码、`checkAuthenticationCode`、`checkAuthenticationPassword`、`getMe`、`logOut`→`Closed` | measured（F079：142 行逐行相同 + A/A 地板 + 变异自检） | — |
| 非空号码的 `setAuthenticationPhoneNumber` | unmeasured | oracle 在 0.8 s 内无响应（联网挂起），没有可判输出 |
| 真实登录（`waitCode` → `waitPassword` → `ready`） | unmeasured | oracle 侧形态已记录（F080：六步 222 行），但 Rust 侧没有 MTProto 引擎、不回放登录段，**等价性未测**；回流条件 = 决定实现 MTProto，或接受「只在 oracle 侧留记录」 |
| `setTdlibParameters` 的设备 / 语言 / 版本 / 加密 key（5 个变体） | measured（F083：首启仍 129 行且逐行相同，两次独立运行一致） | — |
| `use_test_dc:false` 的首启 | measured（F080：127 行 = 125 option + 2 状态，随后的登录段另有 60 行） | 登录段本身见「真实登录」行 |

## 存储面（binlog + SQLite KV + TQueue）

| 项 | 状态 | blocker / 回流条件 |
|---|---|---|
| binlog 文件解析（帧头、`format::escaped`、handler 汇总、key trie） | measured（F100–F102：整份输出与 `binlog_dump` 逐行相同；14 个 fixture，10,128–24,576 B，含真实登录生成的库） | — |
| binlog 写侧（从 KV 生成的库） | measured（F185/F186：27 case 里 20 个明文 case 文件逐字节相同；F189/F190：加密 7 case 结构自洽 + 双向交叉读；F191/F192：KV 层 21 case 文件逐字节 + 双向交叉读。原先记 unmeasured 的理由「salt / IV 不可注入」只对**加密**形态成立，而它已由交叉读与结构判据绕开） | 加密形态的**逐字节**仍不可判（IV 随机，两次运行只共享 33 字节前缀），见 F189 |
| `td::SqliteKeyValue`（`set`/`get`/`get_all`/`get_by_prefix`/`get_by_range`/`erase`/`erase_batch`/`erase_by_prefix`/`drop` + `SqliteDb` 的事务计数 + `SqliteStatement` 的 blob 绑定与视图） | measured（F196–F198：11 case + 2 check 例 / 376,832 B，`PASS cases=11 checks=2`，8.3 s；判据 = narration 逐行 + **双向交叉读**（C++ 读 Rust 库、Rust 读 C++ 库）+ FLOOR + abort 对。判据事实：同一批数据经 `getall` 是 **rowid 序**、经 `range`/`prefix` 是 **PK 索引序**（`REPLACE` 把 key 移到 rowid 序末尾）；`get_by_range("", till)` 忽略 `till`；`eraseprefix -` 清空全表；空 `Slice` 绑成 zero-length blob 而非 NULL；`commit` 无 begin 报 `No matching begin for commit` 且嵌套 begin 计两层） | 文件字节（sqlite 3.31 vs 3.53.1）、SQLCipher（`DbKey` 非空）、`explain()`、`trace()`、`last_error` 的 CORRUPT 删库、多元素 `set_all` 的顺序，逐条理由与回流条件见 F196 |
| `td::TQueue`（`TQueueImpl` 的 `push`/`do_push`/`get`/`forget`/`clear`/`run_gc`/`get_head`/`get_tail`/`get_size` + `TQueueMemoryStorage` 的 log_event_id 分配/`pop`/`replay` + `EventId` 边界与 `operator<<`） | measured（F199–F201：12 case + 2 check 例 / 语料 109 行 / 输出 198 行，`PASS cases=12 checks=2`，7.0 s；判据 = narration 逐行 + FLOOR + abort 对 + `replay` 重建。判据事实：`get_size` **不数 data 为空的末尾槽位**，这条同时决定 `clear(q,0)` 只返回 `size-1` 个、`pop` 对最后一个事件走 rewrite（清 data 留 id）、`get` 返回的是 size 而不是事件数；`max_events=0` 仍会把过期事件从队头 pop 掉；`push` 只在 `tail_id.empty()` 时采纳 hint；错误检查顺序里「太大」先于「已过期」；`EventId::from_int32(0)` 成功且 `empty()`） | `Random::fast` 的初值（语料全程给 hint，无 hint 只判区间）、`run_gc` 的 50 ms 预算与 `& 128` 分片、`Time::now()`、`TQueueBinlog` 的 2314/2315 帧、`close(Promise)` 需要 actor，逐条回流条件见 F199 |
| `td::TQueueBinlog<Binlog>`（`push` 的 `add`/`rewrite`、`pop` 的 `erase`、`pop_batch` 的 `erase_batch`、`replay`） | measured（F202–F204：10 case / 4,016 B，`PASS cases=10 bytes=4016`，1.2 s；判据 = **binlog 文件逐字节** + narration + **双向交叉 replay**（C++ 读 Rust 文件、Rust 读 C++ 文件）+ FLOOR，selftest 另用**独立的 Python 帧解析**反算 type/flags/size/body。判据事实：`magic = 2314 + (extra != 0)`、帧长 = 32 + body、rewrite **只清 data 不清 extra**（2315 清空后仍 2315）、erase 是 32 字节 `-2/1` 帧且 id 指向被删的 `log_event_id`、`erase_batch` 先 `next_event_id(0)`、**replay 走 processor 的 live 集**（6 帧 → 1 个）、300 字节 payload 用 `0xFE` 长度形式） | 加密（`DbKey` 非空）、reindex（语料 4 KB）、`TQueue::close(Promise)`，逐条回流条件见 F202 |
| `td::SqliteConnectionSafe` + `td::SqliteKeyValueSafe`（懒打开 / `get` 复用 / `close` / `close_and_destroy` / 删库后 `get` 的致命路径 / 同步 KV） | measured（F217–F219：8 case（6 + 2 abort）/ 65 行语料 / 122 行输出，`PASS cases=8 aborts=2`，1.9 s；判据 = narration 逐行 + FLOOR + 两条 abort（解析致命文案里的 `state N`）+ 10 条手算 cell。判据事实：构造不碰库、`close` 保文件、`destroy` 删文件、`close_state_` 经文案可判（0 / 65537）、kv 在 close 后仍可读） | `SqliteKeyValueAsync`（要串行驱动）、per-thread 连接槽只用到 0 号、真实 sqlite 字节 |
| `td::SqliteKeyValueAsync`（串行驱动：`set`/`get`/`erase`/`erase_by_prefix`/`set_all`/`close`，buffer/flush 与 promise 载荷） | measured（F229–F231：6 case（4 + 2 abort）/ 39 行语料 / 70 行输出，`PASS cases=6 aborts=2`，1.8 s；判据 = narration 逐行 + FLOOR + 两条 abort（空 key）+ 11 条手算 cell。判据事实：promise 在 10 ms 定时 flush 后兑现、`erase_by_prefix`/`set_all`/`close` 强制 flush、close 后只能经 kv-safe 读） | 延迟 flush 的时序本身、多元素 `set_all` 的遍历序、多个 buffered op 的 promise 合批 |

## 值类型面（`td/telegram` 的纯值类型）

| 项 | 状态 | blocker / 回流条件 |
|---|---|---|
| `td::Usernames`（构造分类 + `change_editable_username`/`toggle`/`add_secondary`/`delete_secondary`/`deactivate_all`/`reorder_to` + `check_validness` + `can_*` + `operator<<`） | measured（F205–F207 + F211：21 case + 1 check / 146 行语料 / 242 行输出，`PASS cases=21 checks=1`，0.9 s；判据 = narration 逐行 + FLOOR + abort 对 + 8 条手算 cell。两条现状只能靠差分发现：无 editable 时打印仍有前导逗号；`reorder_to` 不复制 disabled 标志 → 整对象作废） | `store`/`parse` 序列化不在范围内；`get_usernames_object()` 的 td_api 对象已由 `td_api_objects` lane 判（F236） |
| `td::MessageId`（三种构造 + 两个有效性区间 + `get_type` 七分支 + `prev/next` 进位 + 调度 date/server 拆解 + `operator<<` 七形态 + 三个向量转换） | measured（F208–F210 + F211：8 case + 4 check / 268 行语料 / 430 行输出，`PASS cases=8 checks=4`，1.1 s；判据 = narration 逐行 + FLOOR + 四条 abort 对 + 10 条手算 cell。判据事实：打印尾巴是低 20 位原值、scheduled 的 next 不跨 server 边界、`get_scheduled_server_message_id` 先 CHECK `is_scheduled_server`） | 两条 order 谓词、storer/parser 不在范围内；`get_message_id(telegram_api::Message*)` 已由 `td_api_objects` lane 判（F236） |
| `td::DialogId`（六个构造 + 五段负区间与 User 正区间 + 四个 CHECK 取值 + 五个 `get_dialog_ids` 重载 + `remove_secret_chat_dialog_ids` + `operator<<`） | measured（F212–F214：7 case + 4 check / 199 行语料 / 302 行输出，`PASS cases=7 checks=4`，0.9 s；判据 = narration 逐行 + FLOOR + 四条 abort 对 + 11 条手算 cell。判据事实：区间端点开闭是核心——`-2×10^12` 不是 secret chat，`-2002147483649` 反而是 Channel；构造器不 CHECK） | `get_message_dialog_id`、`Peer`/`DialogPeer` 构造、storer/parser 不在范围内（需要 TL 对象） |
| `td::ForumTopicId` + `td::SavedMessagesTopicId`（纯值部分：`is_valid`/`get`/`==`/`operator<<`、`general()`、与 `MessageId` 往返、`get_unique_id`/`is_author_hidden`、三种打印） | measured（F221–F223：5 case（4 + 1 abort）/ 34 行语料 / 56 行输出，`PASS cases=5 aborts=1`，0.4 s；判据 = narration 逐行 + FLOOR + 一条 abort + 12 条手算 cell。判据事实：topic id 就是 server id、`id==0` 允许而非 server 其它值 abort、隐藏作者常量 `DialogId(2666000)`） | 吃 `Td` 的方法（topic type object / input peer）、`MessageForwardInfo` 构造、storer/parser 不在范围内 |

## 文本变换面（`td/telegram` 的纯字符串函数）

| 项 | 状态 | blocker / 回流条件 |
|---|---|---|
| `get_outline_svg_path`（紧凑编码展开 + `M/L/H/V/C/S/Z` 归一化 + `int` 截断缩放 + `z` 回闭） | measured（F225–F227：6 case / 33 行语料 / 54 行输出，`PASS cases=6`，0.2 s；输出逐字节 + FLOOR + 9 条手算 cell。判据事实：函数自补 `M`/`z`、`z` 仅在偏离起点时补 `L`、`int` 截断、`S` 反射、未知命令空串） | 紧凑编码的 `>=64`/`>=128` 简写不在范围内；`get_outline_object`（td_api 对象）已由 `td_api_objects` lane 判（F236） |
| 生成对象面：`td_api` 的 JSON 打印（`td_api_json_*` 的生成 `to_json`）与三个装配函数 `Usernames::get_usernames_object()`、`get_outline_object(path, zoom, source)`、`MessageId::get_message_id(const telegram_api::Message*, bool)` | measured（F234–F237：5 case + 1 check / 39 行语料 / 100 行输出，`PASS cases=5 checks=1`，0.4 s；判据 = narration 逐行（`json=` 逐字节）+ FLOOR + 非法 UTF-8 的 abort 对 + 13 条手算 cell。判据事实：`int64` 打成 JSON 字符串、`double` 六位小数（`nan`/`inf` 原样）、`bytes` base64、null 的 `object_ptr` 字段整条省略、vector 里的 null 打 `null`、空 vector 打 `[]` | TL `store`（`TlStorerToString`）与其余 ~1100 个生成打印器不在范围内 |
| 媒体几何面：`get_dimensions` / `get_dimensions_pixel_count` / `Dimensions` 的 `==` 与 `<<`；`MediaAreaCoordinates` 的两个构造器、`init` 的夹取与归零、`is_valid`、`get_area_position_object`、`get_input_media_area_coordinates`、`==`/`!=`、`<<` | measured（F239–F242：3 case + 2 check / 32 行语料 / 102 行输出，`PASS cases=3 checks=2`，0.55 s；判据 = narration 逐行（64 位位型 + 六位小数 + `areaPosition` JSON）+ FLOOR + 两条 abort 对 + 12 条手算 cell。判据事实：单边越界整对清零、非有限值先归 0、rotation 夹到 `[-360, 360]` 后负数加 360、TL 构造器丢 `flags` 而 getter 从 `radius > 0` 重算、`==` 是逐字段 `|Δ| < 1e-6` | `store`/`parse` 与 `PhotoSizeSource` 一类几何相邻值类型不在范围内 |
| 静态加密面：`secure_storage::ValueHash` / `calc_value_hash`（`Slice` 与 `DataView` 两条）/ `calc_aes_cbc_state_sha512|pbkdf2` / `Secret`（校验和、`get_hash`、`clone`）/ `Secret::encrypt`·`EncryptedSecret::decrypt`（两种算法）/ `gen_random_prefix` / `Encryptor`·`Decryptor` / `encrypt_value`·`decrypt_value` | measured（F244–F247：5 case / 60 行语料 / 94 行输出，`PASS cases=5 lines=94`，0.72 s；判据 = narration 逐行 + FLOOR + **双向交叉读** + 篡改后的 `Hash mismatch` + 15 条手算 cell。判据事实：`DataView` 每 128 KiB 一次 `pread`、`AesCbcState` 的 key/iv 切分、`(255 + 239 - sum % 255) % 255` 校验和、`get_hash` 是小端、`to_skip_` 超长时置 0、`finish` 哈希覆盖含前缀的整段明文、`Encryptor` 只接受连续 offset | 随机前缀的**内容**、`create_new` 的随机性、`encrypt_file`/`decrypt_file`（文件路径与 OS 错误）不在范围内 |

## 协议面与 RPC 面

| 项 | 状态 | 判据形态 |
|---|---|---|
| auth_key 交换步骤 1–9（resPQ、pq 分解、p_q_inner_data、server_DH、g_b/auth_key/KDF） | measured（F103–F106：官方向量 14 字段） | 逐字节 |
| 消息加密（msg_key、KDF2、AES-256-IGE） | measured（F107：4 字段） | 逐字节 |
| 完整包（salt/session_id/msg_id/seq_no/length/padding） | measured（F108：6 字段，TDLib 写 → Rust 读） | 字段（padding 随机） |
| intermediate framing | measured（F109：3 case；padded 只比 magic） | 逐字节 / 字段 |
| seq_no 状态机 | measured（F110：6 值逐值相同） | 逐字节 |
| msg_id 生成 | measured（F110：三条不变式） | 不变式（`Random::secure_int32`） |
| RPC 请求头（invokeWithLayer + initConnection） | measured（F111：96 B） | 逐字节 |
| `rpc_error` 编解码 | measured（F112：24 B 字段，双向 roundtrip） | 逐字节 |
| AuthData 的 salt 状态机（`set_server_salt` / `set_future_salts` / `update_salt` / `is_server_salt_valid` / `has_salt` / `need_future_salts`） | measured（F113：与时间差、窗口一起 3,026 步；边界 `valid_until == server_time + 60`、`valid_since == server_time` 都覆盖） | 逐字节（注入 `now`，双精度按位型） |
| 服务端时间差（`update_server_time_difference` 的 `cur + 1e-4` 迟滞、`reset`）与入站/出站 msg_id 窗口 | measured（F113：`(st−150, st+30)` / `(st−300, st+30)` 四侧边界） | 逐字节 |
| `check_packet`：session 校验、奇偶、1000 槽 duplicate checker（插入、2N 压缩、duplicate、very old） | measured（F113：含 2,001 条 id 的压缩段与三种错误文案） | 逐字节 |
| `check_packet` 的 `Ignore too old or too new` 分支 | unmeasured | 时间差被 update 改成 msg_id 高 32 位后该判据恒真，脚本构造不到；回流条件 = 找到能让 update 返回 true 且窗口不成立的输入（或改成可注入的比较器） |
| 维护类服务消息编解码（`bad_msg_notification` / `bad_server_salt` / `new_session_created` / `pong` / `msg_resend_req` / `msg_detailed_info` / `msg_new_detailed_info`） | measured（F114：41 case / 344 字段字节，含抽象分派双向 roundtrip 与三种解析错误文案） | 逐字节 |
| MTProxy secret（`ProxySecret`：`from_link` 三级回退、`from_binary` 长度与 truncate、`get_encoded_secret`、`get_proxy_secret`、padding / TLS 判定、domain） | measured（F116：128 case，41 手写 + 87 随机；三类错误文案与拒绝原因） | 逐字节 |
| `MtprotoHeader` 选项矩阵（代理类型 × emulator × 语言包/语言码 × tz_offset × 非 ASCII/长字符串；default 与 anonymous 两份头） | measured（F118：85 case，含 flags 三位、代理块与 Web 固定 443、`X` 自定义语言码、TL 长字符串 253/254/300；F119 记录 argv 编码的仪器坑） | 逐字节 |
| `rpc_result` / `msg_container` 解析、`req_msg_id` 匹配 | measured（F124：容器与单包、`rpc_error`、`req_msg_id == 0` 的拒绝、默认对象透传的 `original/size` 都逐字节相同；`msgs_ack` / `new_session_created` / detailed-info 族 / `msgs_all_info` / `msgs_state_info` 也已对齐；gzip_packed 两层、`pong`、`future_salts` 已在 F130 补齐） | 逐字节 |
| `bad_msg_notification` 的 `error_code`→动作表与 `bad_server_salt` 的换盐策略 | measured（F124：11 个 code 加 48 换盐，文案与关闭时机全等；F125 变异证明逐条在判据里） | 逐字节 |
| Web 代理纯函数（`get_web_proxy_secret` / `get_web_proxy_link_secret` / address / 标记 link secret / HMAC bridge capability） | measured（F126：26 case，含 0x70 标记的三种底层 secret、path 有无的分支、非 ASCII server；F127 变异证明上下文常量在判据里） | 逐字节（未覆盖 host 归一化的 IDN 分支） |
| `misc.cpp` 轻量清洗与用户名判定（`clean_username` / `clean_phone_number` / `replace_offending_characters` / `is_valid_username` / `is_allowed_username`） | measured（F128：44 case，含 trim 六个字符、非 ASCII、连续段与截断序列、32/33 字节边界、保留前缀表；F129 两次变异证明小写映射与前缀表都在判据里；F134 把 lane 扩到 80 case） | 逐字节 |
| `clean_name` / `strip_empty_characters` / `clean_input_string` | measured（F134/F135：74 条 argv + 6 条 stdin，含 34996/34997/35000 字节边界、19 个空格字符与 `U+F0000..U+F00FF` 块、Höhrmann UTF-8 检查；F135 三次变异证明 NBSP 折叠、长度上限与空格表首字节集都在判据里） | 逐字节 |
| `DhHandshake::check_config` 的四段判定（2048-bit 位数、`p mod 4g` 二次剩余表、`p` 素性、`(p-1)/2` 素性；`DhCallback` 为 `nullptr` 时跳过缓存分支） | measured（F136：32 case，含 RFC 3526 group 14 与一个 `p` 素数但 `(p-1)/2` 合数的 2048-bit 例；F137 三次变异分别打中模表、半素数检查、位数检查） | 判定（分支枚举 + 随机模数；素性结论与 OpenSSL 对齐） |
| `RSA` 的 PEM 解析（SPKI 与 PKCS#1、base64 + DER）、三种失败文案、`get_fingerprint`、裸 `x^e mod n` 的 encrypt / decrypt_signature | measured（F138–F140：23 case，PEM 由仪器现场构造；坏 base64 / 空文件 / 非 SPKI / EC key / 2040-bit / `x >= n` 都覆盖） | 逐字节 |
| `PublicRsaKeySharedMain` 的 main/test key 表（PKCS#1 PEM 常量、`{...}` 失败文案、请求指纹的外层顺序） | measured（F140/F141：9 case；`create(false)` = main、`create(true)` = test；F141 三次变异） | 逐字节 |
| `PublicRsaKeySharedCdn` 的 add / get / drop / has 生命周期 | measured（F142/F143：21 行脚本输出；`add_rsa` 的按指纹去重在现有接口下不可观察，写在 F142 里） | 逐字节（去重分支 unmeasured） |
| `tdutils::BufferSlice` 的所有权语义（`clone` 共享 / `copy` 深拷贝 / `as_mutable_slice` 无 COW / `confirm_read` / `truncate` / `after`） | measured（F144/F145：34 行脚本，三次变异分别打中别名、窗口推进、`begin_` 偏移） | 别名（写一处读两处） |
| `td/telegram/net/DcId` 的值面（静态 `is_valid` / `create` / `from_value`、`is_empty` / `is_main` / `is_exact`、`==` / `<`、`operator<<` 的五个分支） | measured（F147/F148：273 行，含 13 个 int32 边界 + 40 个固定随机值；`get_raw_id` / `internal` / `external` 的 CHECK 分支 unmeasured） | 逐字节 |
| `tdutils::Slice` 的视图操作（`remove_prefix` / `remove_suffix` / `substr` 的静默截断 / `truncate` 只缩不涨） | measured（F149/F150：119 行 + 17 个 `--check` 例，其中 12 个越界 abort、5 个 `n == size` 对照；F150 三次变异） | 逐字节 + 判定（abort 与否） |
| `tdutils::Slice` 的查找与比较（`find` / `rfind` 的 `npos`、`==` / `!=`、`<` 的 unsigned 字节序与前缀规则） | measured（F151/F152：595 行，17 载荷 × 9 字节 × 2 + 17×17 比较；F152 三次变异分别打中末字节、signedness、前缀长度） | 逐字节 |
| `tdutils` 的前后缀判定（`begins_with` / `ends_with`，`misc.h` 的自由函数） | measured（F153/F154：578 行，17 个载荷的两两有序对 × 2；超长 affix 短路与空 affix 的 one-past-end 零长 slice 都覆盖；F154 三次变异） | 逐字节 |
| `tdutils::Slice` / `MutableSlice` 的 `back()` / `operator[]`（合法域）与 `data`/`begin`/`end`/`ubegin`/`uend` 的位移一致性 | measured（F155/F156：106 行 + 5 个 `--check` 例，8 个载荷 × 两类视图 × 两种移位；空 slice 的 `back()` abort 单独判，越界 `operator[]` 是 UB 记 unmeasured） | 逐字节 + 判定（abort 与否） |
| `tdutils` 的 `Hash.h`（`AbslHash` / `TdHash` / `Hasher`，`TD_HAVE_ABSL 0` 配置） | measured（F157/F158：70 行，i32/i64/u32/u64/bool/char/uchar/pair 与 Hasher 链；符号扩展、`char` 有符号、pair 无序性是判据核心；absl 配置与 Slice/string 特化 unmeasured） | 逐字节 |
| `tdutils` 的 `SliceHash`（`uint32` 累加器，`result * 123456789 + c`） | measured（F159/F160：28 行，14 个载荷 × `Slice` 与 `MutableSlice → Slice` 两条路；2^32 回绕、`char` 符号扩展、空 slice = 0、顺序敏感都在判据里；另用独立 Python 公式反算 oracle） | 逐字节 |
| `tdutils::PathView`（构造 + 五个名字/扩展名访问器 + `parent_dir`/`parent_dir_noslash` + `is_dir`/`is_absolute` + `relative` + `dir_and_file`） | measured（F161/F162：54 行，34 条路径 + 9 组 relative + 11 组 dir_and_file；`TD_DIR_SLASH` 取 Windows 构建的 `\`，`relative` 的字符串前缀语义与 `dir_and_file` 的两段语义都在判据里） | 逐字节 |
| `tdutils` 的字符串助手（`to_lower`/`to_upper`、`trim`、`lpad`/`lpad0`/`rpad`、`oneline`、`replace_with_spaces`、`implode`、`split`/`full_split`、字节谓词） | measured（F163/F164：88 行，输入输出全 hex；`is_space` 不含 `\f`、`oneline` 的 CRLF 合并与行首空格丢弃、`full_split` 的 `max_parts` 都是判据核心；`to_integer` 家族不在本行） | 逐字节 |
| `tdutils` 的整数解析（`to_integer` 四宽度、`to_integer_safe`、`hex_to_integer(_safe)`、`hex_decode`/`hex_encode`） | measured（F165/F166：296 行；有符号回绕/取反/夹到 MIN、safe 的逐字节往返、hex 的三条错误文案与 `MAX/16` 溢出检查都在判据里） | 逐字节 |
| `tdutils` 的 base64/base32（标准与 url 编码、解码的五条错误文案、四个 `is_*` 谓词、`base64_filter`、`base32_encode`/`base32_decode`） | measured（F167/F168：203 行；url 不补 `=`、未用位必须为 0、长度检查先于字符检查、base32 的 `Nonzero padding` 都在判据里） | 逐字节 |
| `tdutils` 的结构性 UTF-8（`check_utf8` 的 DFA、`utf8_length`、首字节谓词、`next_utf8_unsafe`、`append_utf8_character`、`utf8_truncate`/`utf8_substr`、`utf8_utf16_length`/`truncate`/`substr`） | measured（F169/F170：309 行；overlong/代理/>U+10FFFF/截断的 DFA 边界、字节-字符预算混用、`F8..FF` 在 UTF-16 计数里的不一致都在判据里；`utf8_to_lower`/`utf8_get_search_words` 现在由下方的 unicode 行判（F176），不在本行） | 逐字节 |
| `tdutils::Hints`（`add`/`remove`/改名/`set_rating`/`search`/`search_empty`/`has_key`/`key_to_string`/`size`，含两张词表） | measured（F171/F172 + F182/F183：160 行脚本 / 156 行输出，手算 cell 43 条；`(rating 升序, key 升序)`、交集、`limit` 三种形状、改名删旧词与旧转写、折叠先于转写、`search_word` 是并集而非交集、查询侧 `allow_partial=true` 都在判据里；`fix_words` 的前缀丢弃不可观察） | 逐字节 |
| `tdutils` 的 unicode 表与复合函数（`get_unicode_simple_category`、`prepare_search_character`、`unicode_to_lower`、`remove_diacritics` 的全码点扫描，`utf8_to_lower`、`utf8_get_search_words`） | measured（F176/F177：1,114,112 码点全扫 + 四张表区间端点 1653/1563/192/3490 个 + 21 `utfl` / 28 `words` 载荷 = 1,127,703 行输出，2.6 s，26 条手算 cell；`cat` 跳表只定义到 0x11FFFF、`> 0x10FFFF` 先返回 0、标点→空格 32、全角在 `remove_diacritics` 才变窄都在判据里；`utfl`/`words` 的截断尾字节越界（oracle 0xC0000409）是范围限制） | 逐字节 |

| `tdutils` 的所有权类型（`SharedSlice` 的 clone、`UniqueSharedSlice` 构造函数的偷 / 深拷贝两分支、`SharedSlice(UniqueSharedSlice)`、`UniqueSlice` / `SecureString` 的 copy / clear、`clone_as_buffer_slice` 的独立拷贝） | measured（F174/F175：69 行，14 个状态 × 5 种类型；默认构造与零长分配在公开接口上不可区分，写进 F174） | 逐字节 + 别名（写一处读两处） |

| `tdutils` 的 `StringBuilder`（整数六宽度含 MIN 进位、`char`/`signed char`/`unsigned char`/`bool`/`Slice`/字面量、`pair`/`vector`/`vector<bool>`、`FixedDouble` 的 classic-locale 定点格式化、`%p`、`push_back`/`append_char`/`pop_back`/`clear`/`size`/`as_cslice`、增长与定长截断 + error 标志） | measured（F178/F179：389 行脚本 / 355 行输出 + 7 个 abort 例，1.5 s；`%p` 是 16 位小写、负 NaN 不带符号、负 precision 等价 6 位、非可增长 builder 上整数与 `%p` 越 `end` 不报错都在判据里；Rust `{:.p}` 与 `std::ostream` 在本语料逐字节全等，非有限值与负 precision 是仅有的两处显式改写） | 逐字节 |
| `tdutils` 的 `get_word_transliterations`（en↔ru 四张表、复杂规则表序首匹配、`allow_partial` 前缀枚举、`td::unique` 排序去重） | measured（F180/F181：18,442 载荷 × 2 个 `allow_partial` 值 / 124,186 行输出，4.9 s，19 条手算 cell；`shch` 先于 `sh`、`sch` 与 `sh` 不竞争、partial 在当前位置追加且被完整匹配压掉、大写原样通过、`ь` 的空替换被 `!s.empty()` 吞掉都在判据里；另在 ABI 面有 3 条 `searchStringsByPrefix` cell，F184） | 逐字节 |
| `tdutils` 的密码学原语（`pbkdf2_sha256`、`hmac_sha256`、`AesCtrState`、`crc32`） | measured（F187/F188：458 行脚本 / 457 行输出，6.7 s；另用 `hashlib`/`hmac`/`zlib` 反算 457 个值 + RFC 7914 / RFC 4231 / NIST F.5.5 公开向量。三条 API 事实：`pbkdf2_sha256` 把 dkLen CHECK 成 32 且 `iterations > 0`（0 次是 `0xC0000409`）、`AesCtrState` 只接受 32B key + 16B iv、计数器 128 位大端带进位（全零 iv 下第 4096 字节才进位）；`crc32` 是 zlib 的反射实现） | 逐字节 |
| `tddb` binlog **写侧**（未加密：`add`/`rewrite`/`erase`/`erase_batch`/`lazy_sync`/flush/close/reopen） | measured（F185/F186：27 case 中的 20 个明文 case / 345,272 字节文件逐字节相同，1.9 s；帧几何、crc32 覆盖 `size-4`、`size%4!=0` 的 FATAL、TL 字符串 1/10/253/254→4/12/256/260、空载荷仍写 32 字节帧、`rewrite` 写目标 id、计数器不落盘（replay 只数追加事件）、**>50 KiB + 5:1 死比自动 reindex**（文件不是 append-only）都在判据里；另用 Python `zlib.crc32` 反算帧尾） | 逐字节 |
| `tddb` binlog **写侧**（加密：`AesCtrEncryption` header + 逐帧 AES-256-CTR + `change_key`） | measured（F189/F190：7 个加密 case / 201,168 字节，0.9 s；每 case 五条判据——解码帧序列、C++ `binlog_dump` 交叉读、Rust `binlog_head` 交叉读、长度自洽、Python 从 salt 重算 `key`/`key_hash`。范围限制：随机 IV 使两次运行共享恰好 33 字节前缀（28 字节帧头 + 清零 flags + `0x20` salt 长度），密文其余部分每次不同；对同对象二次 `init` 会崩（`0xC0000005`），按 `TdDb` 的用法用新对象重开） | 结构 + 交叉读 |
| `tddb` KV 层（`BinlogKeyValue<Binlog>` 的 set/erase/erase_batch/erase_by_prefix/访问器/external_init_* + 三档 magic，`SeqKeyValue` 序号语义） | measured（F191/F192：21 case / 884,208 字节（含 4 份交叉读副本），1.84 s；每 case 四条判据——文件逐字节、narration 逐行、C++ `attach` 读 Rust 文件、Rust `attach` 读 C++ 文件。判据事实：帧 id ≠ 序号、`Rewrite` 目标是该 key 上一次事件的 id、`get` 对「缺/空/删」都返回 `""`（只有 `isset` 与帧类型能分）、`init` **不过滤 magic**（外来类型会变活条目，TdDb 的 `default: LOG(FATAL)` 才是闸门）、magic 是 per-init 的。范围限制：`erase_by_prefix` 按 `WaitFreeHashMap` 遍历序写帧，语料限制成每次最多一个匹配） | 逐字节 + 交叉读 |
| `TdDb` 路径与 key 派生（`as_db_key`、`check_parameters`、两个匿名命名空间的路径模板） | measured（F193/F194：37 行脚本 / 35 行输出，0.2 s；判据 = stdout 逐字节 + **目录树快照**。判据事实：`check_parameters` 是 private（oracle 用 `#define private public`）、`mkpath` 只按 `\\` 切分、`mkdir` 把 183 当成功所以普通文件能当目录、`realpath` 是纯词法 `GetFullPathNameW`；两个路径模板没有 oracle，由 selftest 按源码断言） | 逐字节 + 目录树 |

## 边界之外（永久不在本仪器范围）

| 模块 | 行数 | 原因 |
|---|---|---|
| `tddb` | 4,792 | 在 C ABI 之下，互操作需 C++ ABI；binlog 读侧已由上方「存储面」单独覆盖 |
| `td/mtproto` | 7,443 | 同上 |
| `tdactor` | 5,695 | 同上 |
| `tdutils` | 48,142 | 同上 |