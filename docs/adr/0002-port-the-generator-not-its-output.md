# ADR-0002: 生成代码不手写移植，移植生成器

日期：2026-11-26 · 状态：accepted（第一阶段），第二阶段待验 · 依据：F001、F002

## 背景

构建会从 schema 生成 **490,656 行 C++**（`td/generate/auto/td/telegram`：`td_api*` 300,511 行 / 23 文件，`telegram_api*` 180,577 行 / 12 文件，`secret_api*` 6,849 行，`e2e_api*` 2,719 行）。这些不是手写代码：

- 权威来源是 **22,255 行 `.tl` schema**（`td_api.tl` 18,588、`telegram_api.tl` 3,361、`secret_api.tl` 131、`mtproto_api.tl` 87）
- 加工逻辑是 **7,152 行 C++ 生成器 + 554 行 PHP（`SplitSource.php`）**

## 决定

1. **不手写移植生成产物**。目标不是把 49 万行译成 Rust，而是让 Rust 侧从同一批 schema 产出等价结构。
2. **第一阶段（当前）**：只为 `td_execute` 实际用到的 action 手写所需对象序列化（已完成 4 个 action）。这层很薄，因为 JSON 边界只需要「解析请求 → 算 → 写 JSON」。
3. **第二阶段（进入有状态客户端之前）**：优先复用现成的 Rust TL 代码生成器（`grammers-tl-gen` + `grammers-tl-parser`，用 TDLib 的 `.tl` 作输入），只有在 schema 构造不被支持时才考虑移植 TDLib 自己的生成器。

## 验证结果（2026-11-26，F040）

实测 `grammers-tl-parser` 2.0.0 + `grammers-tl-gen` 0.12.0 吃 `td_api.tl`：

- 原文件直接喂会 **panic**（注释里的非 ASCII 让它切到 UTF-8 码点中间）；去掉注释行后可解析。
- 3473 条定义里有 3470 条被接受，3 条 `NotImplemented`：`vector {t:Type} # [ t ] = Vector t;`、`double ? = Double;`、`string ? = String;`——都是 TL 的泛型/裸可空标记，TDLib 自己的生成器支持。
- 生成器对 3470 条定义产出 3.47 MB `types` + 1.00 MB `functions`。

**结论：可行，但不是 drop-in**——需要（a）注释预处理或修 parser 的边界 bug，（b）为 3 条构造补支持或改写输入。这比移植 7.7k 行 C++ 生成器便宜得多，维持第二阶段选型。

## 后果

- 避免把 49 万行机器输出当作工作量。
- 风险集中在第二阶段：若 grammers 不适用，退路是移植 7.7k 行生成器，而不是手写 49 万行。