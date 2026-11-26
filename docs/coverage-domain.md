# 判据定义域（coverage domain）

覆盖 `tdjson` C ABI 的 `td_execute` 路径（29 个同步 action + 全部 1114 个函数的参数解析）与客户端 lane（首次运行、第二次运行、未授权状态机、真实登录记录）。`measured` 需 test + fact；`unmeasured` 只占回流条件，不占 fact。

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
| 具体类字段：`@type` 不校验（可以是任意值或缺失），**但已声明的字段递归校验** | measured（F082 前半 + F084 更正：F082 的三条样本用的都是不存在的字段名 `bg_color`；`folder.name.text:true` → `Expected Object, but receive Boolean`） | — |
| `null` 在任何位置都是「取默认值」，包括 `vector<T>` 的元素自身与 vector 字段 | measured（F086） | — |
| `bool` 字段接受数字与数字字符串（回退 `from_json(int32)`），失败文案仍报原类型 | measured（F085：`include_contacts:"true"` → `Expected Boolean, but receive String`；`include_bots:1` 接受） | — |
| 随机语料（21 个无状态 action 的结构化生成 + 手写语料的字节变异） | measured（F088：12 个种子共 25,496 条零分歧；抓到的 4 类真实分歧——嵌套具体类字段、`null` 元素、`bool` 取数字、`return_none_for_empty_query`——已修，见 F084–F086） | 8 个有状态日志 action 只能按有序序列判 |

## 客户端 lane（首次运行、第二次运行、未授权状态机、真实登录记录）

| 项 | 状态 | blocker / 回流条件 |
|---|---|---|
| 首启序列：`td_create_client_id` → `td_send(setTdlibParameters)` → drain 到 `waitPhoneNumber`（129 行 = 127 `updateOption` + 2 `updateAuthorizationState`） | measured（F076：`tools/client_diff.py`，A/A 地板 + 变异自检 + 两侧逐行相同） | — |
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

## 边界之外（永久不在本仪器范围）

| 模块 | 行数 | 原因 |
|---|---|---|
| `tddb` | 4,792 | 在 C ABI 之下，互操作需 C++ ABI |
| `td/mtproto` | 7,443 | 同上 |
| `tdactor` | 5,695 | 同上 |
| `tdutils` | 48,142 | 同上 |
| 客户端 lane 之下的 `tddb` 文件格式（binlog） | — | 判据是 `td_receive` 的输出，不是磁盘内容（F074），所以不实现 |