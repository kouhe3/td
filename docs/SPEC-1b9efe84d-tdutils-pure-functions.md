# SPEC（as-built）— `tdutils` 纯函数面

> **版本**：基座 commit `1b9efe84d`（master，判定基座）× oracle `libtdjson.dll` md5 `0398814038978aaeef7f8d6ac62bb8ff`。文件名里的 hash 就是版本：基座或 oracle 漂移 ⇒ 按新 hash 另起 `docs/SPEC-<新hash>-<area>.md`。
>
> **这份是 as-built 规格，不是 to-build 规格。** 它描述的是「已经被差分判据测过、数字可复现」的现状，供接手方（spec-driven 流程）当测绘底图；`待证` 状态的东西不在这里，见文末「不可判项」。
> 每个判断都要能由文中命令复现；台账行（`docs/facts.md`）是同一事实的证据来源。

## 1. 定义域

| 项 | 值 |
|---|---|
| C++ 源 | `tdutils/td/utils/` 下的手写函数（22 个文件、6,152 行被判；`tdutils` 全目录 48,142 行） |
| 语言/构建 | C++（oracle，`C:/tmp/td-build/tdutils/libtdutils.a`）对 Rust（`rust/src/bin/*_probe.rs`，crate `tdjson_rs` 之外的独立 bin） |
| 判定基座 | C++ 提交 `1b9efe84d`；oracle md5 由 `tools/tdjson_diff.py:54` 钉死 |
| 台账 | F144–F183；`tasks/plan.md` T71–T91 |

## 2. 可复现现状

```bash
cd C:/projects/td
uv run python tools/verify.py            # 57 项（本文件涉及其中 18 项），约 160–330 s（含 build），failures=0
```

每一行判据都自带命令；主命令是 `uv run python tools/<lane>_diff.py --selftest`。

## 3. 判据

| lane | 判据形态 | 规模（实测） | fact |
|---|---|---|---|
| `buffer_slice` | 逐字节 + 别名可见性 | 34 行 | F144/F145 |
| `shared_slice` | 逐字节 + 写穿/偷/深拷贝 | 69 行 | F174/F175 |
| `dc_id` | 逐字节 | 273 行 | F147/F148 |
| `slice` | 逐字节 + `--check`（abort 与否） | 119 行 + 17 check | F149/F150 |
| `slice_cmp` | 逐字节 | 595 行 | F151/F152 |
| `slice_prefix` | 逐字节 | 578 行 | F153/F154 |
| `slice_ptr` | 逐字节 + 指针偏移一致性 + 5 check | 106 行 + 5 check | F155/F156 |
| `hash` | 逐字节（`TD_HAVE_ABSL 0`） | 70 行 | F157/F158 |
| `slice_hash` | 逐字节 + 独立 Python 公式反算 | 28 行 | F159/F160 |
| `path_view` | 逐字节（输出全 hex） | 54 行 | F161/F162 |
| `string` | 逐字节 | 88 行 + 32 手算 cell | F163/F164 |
| `number` | 逐字节 | 296 行 + 26 手算 cell | F165/F166 |
| `base64` | 逐字节 | 203 行 + 23 手算 cell | F167/F168 |
| `utf8` | 逐字节 | 309 行 + 34 手算 cell | F169/F170 |
| `hints` | 逐字节（有状态单实例） | 156 行 + 43 手算 cell | F171/F172、F182/F183 |
| `unicode` | 逐字节（全码点扫描） | 1,127,703 行（`md5=c823b29b`） | F176/F177 |
| `string_builder` | 逐字节 + 7 个 abort 例 | 355 行（`md5=8d52ac92`） | F178/F179 |
| `translit` | 逐字节（返回顺序也算） | 124,186 行（`md5=e2e26092`） | F180/F181 |

每条 lane 的验收形状都相同：`corpus md5=… lines=…` → 双侧 stdout/文件逐字节比较 → 首个差异打印 `DIFF line N` → oracle 重跑（FLOOR）→ `PASS …`，退出码 0 才算通过。变异纪律：只改 Rust 实现、语料冻结，每条 lane 至少三条 RED。

## 4. 不可判项 + 回流条件

| 项 | 原因 | 回流条件 |
|---|---|---|
| `Hash.h` 的 absl 配置 | 本机无 absl（`TD_HAVE_ABSL 0`）；开 absl 后带进程级随机种子 | 只能判不变式，或换构建 |
| `get_unicode_simple_category` 的 0x120000+ | 跳表越界读（UB） | 上游把跳表定义域扩大后再判 |
| `utf8_to_lower` / `utf8_get_search_words` 的截断尾字节 | `next_utf8_unsafe` 迈过 slice → oracle `0xC0000409` | 上游加长度检查 |
| `utf8` 的 `FE`/`FF` 首字节 | `UNREACHABLE` | 同上 |
| `binary_search_ranges` 的 `LOG(FATAL)` 兜底 | 四张表内不可达 | 新增表项且越界时 |
| `ChainBufferReader` 跨节点读 | `prepare_read` 只给当前节点；`advance`/`move_as_buffer_slice` 在 clone 上 CHECK 失败（F146） | 造出跨节点读取入口 |
| `Slice::operator[]` 越界 | UB | 不判 |
| `SharedSlice::is_unique` | 只在 `detail::` 里，不公开 | 上游公开 |
| `fix_words` 的前缀丢弃 | 已证明不可观察（匹配集不变） | 不判 |

## 5. 失效条件

下列任一变化都要重编探针、重跑对应 lane 并重新落快照（`uv run python tools/verify.py --json > docs/gate-<date>.json`）：

- **基座漂移**：判定基座不是 `1b9efe84d`（或 oracle md5 变）⇒ 本文件整体作废，按新 hash 另起一份；
- `tdutils/td/utils/`：`Slice.h`、`Hash.h`、`PathView.{h,cpp}`、`Hints.{h,cpp}`、`translit.cpp`、`unicode.{h,cpp}`、`utf8.{h,cpp}`、`base64.{h,cpp}`、`misc.{h,cpp}`、`StringBuilder.{h,cpp}`、`SharedSlice.h`、`buffer.h`、`crypto.{h,cpp}`；
- oracle 重建（md5 变）或 Rust 换 target/linker；
- 生成器输入：`unicode.cpp` 的七张表（`tools/gen_unicode_tables.py --check` 会自己变红）。