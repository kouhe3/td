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

## 待验

- 第二条的可行性尚未实测：grammers 的生成器面向 Telegram MTProto layer，TDLib 的 `.tl` 有额外构造（`SplitSource` 的三文件拆分、自定义 writer、`td_api` 与 `telegram_api` 的命名空间约定）。验证判据是「同一 `.tl` 生成的 Rust 类型能被 JSON 层直接使用」，见 plan T11。

## 后果

- 避免把 49 万行机器输出当作工作量。
- 风险集中在第二阶段：若 grammers 不适用，退路是移植 7.7k 行生成器，而不是手写 49 万行。