# SPEC（as-built）— `td/telegram` 文本变换面（outline SVG path）

> **版本**：基座 commit `1b9efe84d`（master，判定基座）× oracle `libtdjson.dll` md5 `0398814038978aaeef7f8d6ac62bb8ff`（同一基座的 C++ 构建）。文件名里的 hash 就是版本：基座或 oracle 漂移 ⇒ 按新 hash 另起 `docs/SPEC-<新hash>-<area>.md`，本文件置「过期」，不原地改数字。
>
> **as-built 规格，不是 to-build。** 只记录「已判过、数字可复现」的现状。
> 台账行：`docs/facts.md` F225–F227。

## 1. 定义域

| 项 | 值 |
|---|---|
| C++ 源 | `td/telegram/Outline.cpp` 的 `get_outline_svg_path(CSlice path, double zoom, Slice source)`（同文件的 `get_outline_object` 不在范围内） |
| 输入 | TDLib 的**紧凑路径编码**：`< 192` 是命令字节（`>= 128` 先补 `,`、`>= 64` 先补 `-`、再取 `c & 63`），`>= 192` 查表 `AACAAAAHAAALMAAAQASTAVAAAZaacaaaahaaalmaaaqastava.az0123456789-,` |
| 输出 | 归一化 SVG 路径串，例如 `M 10 10 L 20 20 L 10 10z` |
| 语言/构建 | C++（`libtdcore.a`）对 Rust（`rust/src/bin/outline_probe.rs`，纯字符串实现） |
| oracle 探针 | `C:/tmp/td-probe/outline_probe.exe`，源码 `tools/fixtures/outline_probe.cpp`（探针负责把坐标串编码成紧凑形式），完整 g++ 命令行在文件头 |
| 判定基座 | C++ 提交 `1b9efe84d`；oracle 指纹即基座构建 |

## 2. 可复现现状

```bash
cd C:/projects/td
uv run python tools/outline_diff.py --selftest    # 6 case / 33 行语料 / 54 行输出，0.2 s
```

实测（2026-11-28）：`PASS cases=6 lines=54`，`corpus md5=5b526137f03641527ada9f5e79b735f4`。

## 3. 判据

| lane | 判据形态 | 规模（实测） | fact |
|---|---|---|---|
| `outline` | 输出逐字节 + FLOOR + 手算 cell | 6 case / 54 行 | F225–F227 |
| `outline` 的对象面（`get_outline_object`） | JSON 逐字节 + FLOOR + abort 对 | 5 case（与值类型面共用 `td_api_objects` lane）/ 100 行 | F234–F237 |

判据句：`uv run python tools/outline_diff.py --selftest` → `PASS cases=6 lines=54`。

判据事实（F226）：

1. 函数**自己**在紧凑路径前补 `M`、末尾补 `z`；输入是坐标串（`10,10 L20,20`），不是完整 SVG。
2. `z` 只在当前点 ≠ 子路径起点时补一条 ` L`，否则直接闭合（`M 30 30z`）。
3. 坐标是 `' ' + int(x*zoom) + ' ' + int(y*zoom)`，**`int` 强转是截断**（`1.5*10 → 15`，`.25` 丢掉）。
4. `S/s` 的首控制点是上一段终点的反射 `2*x - last_cx`；没有上一段时退化为当前点。
5. 隐式重复命令（`L1,1,2,2`、`h5,5`）照常生效。
6. 未知命令（`Q`/`T`）→ 返回空串（探针打 `out=-`，不是 abort）。
7. 子路径边界的 `m` 会 `pos--` 退回，从而以 `z` 分支闭合上一段。

## 4. 不可判项 + 回流条件

| 项 | 原因 | 回流条件 |
|---|---|---|
| 紧凑编码的 `>= 64` / `>= 128` 简写 | 探针只用查表字节（`192+index`）编码；简写形式没有语料生成器 | 写一个「按 JNI 规则压缩」的生成器，把简写形态也纳入语料 |
| 非有限 zoom / 超大坐标 | `int` 强转在 C++ 是 UB，Rust 是饱和 | 不判；语料限制在可表示范围 |

## 5. 失效条件

任一变化都要重编探针 + 重跑 lane + 重新落快照：

- **基座漂移**：判定基座不是 `1b9efe84d`（或 oracle md5 变）⇒ 本文件整体作废，按新 hash 另起一份；
- `td/telegram/Outline.cpp` 的编码表、命令集（`M/L/H/V/C/S/Z` 及其小写）、`int` 截断或输出格式任一处；
- 紧凑编码的约定（`>= 192` 查表、`>= 128`/`>= 64` 前缀）；
- oracle 重建（`libtdcore.a` 换构建）。