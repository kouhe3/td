# 判据定义域（coverage domain）

覆盖 `tdjson` C ABI 的 `td_execute` 路径（29 个同步 action + 全部 1114 个函数的参数解析）、客户端 lane（首次运行、第二次运行、未授权状态机、真实登录记录）与 binlog 存储面（读侧）。`measured` 需 test + fact；`unmeasured` 只占回流条件，不占 fact。

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

## 存储面（binlog 读侧）

| 项 | 状态 | blocker / 回流条件 |
|---|---|---|
| binlog 文件解析（帧头、`format::escaped`、handler 汇总、key trie） | measured（F100–F102：整份输出与 `binlog_dump` 逐行相同；14 个 fixture，10,128–24,576 B，含真实登录生成的库） | — |
| binlog 写侧（从 KV 生成字节相同的库） | unmeasured | salt / IV 由 `Random::secure_bytes` 生成、不可注入，判不了「同输入同字节」；回流条件 = 找到可固定随机源的重写路径 |

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

## 边界之外（永久不在本仪器范围）

| 模块 | 行数 | 原因 |
|---|---|---|
| `tddb` | 4,792 | 在 C ABI 之下，互操作需 C++ ABI；binlog 读侧已由上方「存储面」单独覆盖 |
| `td/mtproto` | 7,443 | 同上 |
| `tdactor` | 5,695 | 同上 |
| `tdutils` | 48,142 | 同上 |