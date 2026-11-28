# SPEC（as-built）— `tdjson` C ABI 面

> **版本**：基座 commit `1b9efe84d`（master，判定基座）× 同一份 oracle `libtdjson.dll` md5 `0398814038978aaeef7f8d6ac62bb8ff`。文件名里的 hash 就是版本：基座或 oracle 漂移 ⇒ 按新 hash 另起 `docs/SPEC-<新hash>-<area>.md`。
>
> **as-built 规格，不是 to-build。** 边界由 `docs/adr/0001-json-abi-as-the-rewrite-boundary.md` 钉死：一个进程里同时加载两个 dll，同一 JSON 请求，逐字节比较输出。
> 台账行：F001–F143、F184、F234–F238；`tasks/plan.md` T1–T70、T109。
> **注意**：`MATCH 1579` 只说明这些样本有效，不说明交付——上游每天更新，仓库只有快照态。

## 1. 定义域

| 项 | 值 |
|---|---|
| 出口 | `tdjson` 的 C ABI：`td_execute` 等 10 个导出符号（F003），`td/telegram/td_json_client.h` |
| C++ 源 | `td/telegram/`（392,287 行）中 `SynchronousRequests.cpp`、`MessageEntity.cpp`、`misc.cpp`、`TdDb.cpp`、`OptionManager.cpp`、`ThemeManager.cpp`、`DialogFilter.cpp`、`NotificationManager.cpp`、`QuickReplyManager.cpp`、`CountryInfoManager.cpp`、`TonWalletManager.cpp` 等被同步 action 触达的部分 |
| Rust 侧 | `rust/src/lib.rs`（8,100 行）+ 生成的 `api_functions.rs` / `tl_schema.rs` / `mime_tables.rs` / `emoji_table.rs` / `country_table.rs` / `client_options.rs` |
| 判据基座 | C++ `1b9efe84d`；oracle `C:/tmp/td-build/libtdjson.dll`，md5 `0398814038978aaeef7f8d6ac62bb8ff` |
| 台账 | F001–F143、F184 |

## 2. 可复现现状

```bash
cd C:/projects/td
uv run python tools/tdjson_diff.py                          # MATCH 1579，~25 s，failures=0
uv run python tools/tdjson_fuzz.py --seeds 20261126,7,99,2027   # 随机语料零分歧
uv run python tools/client_diff.py                          # 客户端四段（首启/第二次/未授权/遗留 C ABI/异步）
uv run python tools/gen_api_functions.py --check            # 6 个生成表门禁之一（全部 --check 逐字节）
```

## 3. 判据

| 面 | 判据形态 | 规模（实测） | fact |
|---|---|---|---|
| 同步 action | 逐字节 | 29/29（`SynchronousRequests.cpp` 白名单） | F016、F026 |
| 全部函数的参数解析 | 逐字节 | 1,114/1,114（`td_api.tl` 生成） | F081、F084、F091 |
| 实体解析（mention/hashtag/url/…） | 逐字节（UTF-16 偏移） | 见 coverage 的实体矩阵 | F005–F014、F051、F057 |
| Markdown v1/v2/v3 + HTML | 逐字节 | 见 `parseTextEntities`/`parseMarkdown`/`getMarkdownText` | F028–F035 |
| 转写路径（`searchStringsByPrefix`） | 逐字节 | 3 条 Cyrillic cell，MATCH 1561 → 1564 | F184 |
| 随机语料 | 零分歧（快照） | 15 个种子 19,550 条 + 300 段/6,758 步 | F121 |
| 客户端：首启 | 逐行（127 update） | `client_diff.py` | F076/F077 |
| 客户端：第二次运行 | 按集合 | 同上 | F078 |
| 客户端：未授权状态机 | 逐行 | 142 行 | F079 |
| 客户端：遗留 C ABI | 逐行 | 143 行 | F090 |
| 客户端：异步路径 | 三支分流 | 16 条请求 | F096–F098 |
| 生成表一致性 | 逐字节（`--check`） | 7 个生成器（`client_options`/`tl_schema`/`api_functions`/`mime`/`emoji`/`country`/`ton_address`） | F019/F020/F091 |
| 生成对象的 JSON 打印 + 三个装配函数 | 逐行（`json=` 逐字节），另一个进程里判 abort | 5 case + 1 check / 39 行语料 / 100 行输出，`PASS cases=5 checks=1`；`int64` 是字符串、`double` 六位、`bytes` base64、null 字段省略、`paths=[]` | F234–F237 |

## 4. 不可判项 + 回流条件

| 项 | 原因 | 回流条件 |
|---|---|---|
| 真实登录（`waitCode` → `waitPassword` → `ready`） | 需要 MTProto 引擎与真实验证码/2FA | 拿到测试账号或本地 MTProto 桩（B1） |
| `getOption commit_hash` | 两侧各烘焙自己的 HEAD | 上游改成可注入 |
| 同一进程里的第二个 client | oracle 会多 6 条全局 update 且顺序错位（F077） | 每进程一个 client（当前做法） |
| 第二次运行的 option 顺序 | oracle 的 run2/3/4 两两不同（首个差异第 4 行） | 按集合判（当前做法） |
| 非空号码 `setAuthenticationPhoneNumber` | oracle 联网挂起 | 离线桩 |
| `addLogMessage` level ≤ 0 | oracle abort | 不判 |
| 含非法 UTF-8 的语言包值 | `td::JsonBuilder` 是 CHECK | 上游改 |
| C ABI 之下的 `tddb`/`td/mtproto` 对象 API | 需 C++ ABI | 不判（改由各自 lane 单独判） |
| 其余 ~1100 个生成打印器与 TL `store`（`TlStorerToString`） | 生成代码面，本轮只判 `usernames`/`outline`/`point`/`closedVectorPath`/`vectorPathCommand*`/`chatPosition`/`error`/`data`/`ok` 这一族 | 新对象进语料即扩；`store` 需要 `TlStorer` 对拍 lane |

## 5. 失效条件

任一变化都要重编 oracle、更新 `tools/tdjson_diff.py:54` 的 md5 pin、重跑 64 项并重新落快照：

- **基座漂移**：判定基座不是 `1b9efe84d`（或 oracle md5 变）⇒ 本文件整体作废，按新 hash 另起一份；
- `td_api.tl` 的定义行（会先让 `gen_tl_schema`/`gen_api_functions` 自己变红）；
- mime / emoji / country / ton 数据表；
- `td/telegram/{SynchronousRequests,MessageEntity,misc,OptionManager,ThemeManager,DialogFilter,NotificationManager,QuickReplyManager,CountryInfoManager,TonWalletManager,Td}.cpp|h`；
- oracle 重建（md5 变）或 Rust 换 target/linker；
- `td/generate/auto/td/telegram/td_api_json*.cpp`（构建产物）或 `td/telegram/{Usernames,Outline,MessageId}.cpp`——对象面 lane 的探针直接链接 `tdjson_private` 的 `.obj`。