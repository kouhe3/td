# ADR-0001: JSON/C ABI 是重写边界

日期：2026-11-26 · 状态：accepted · 依据：F003、F004

## 背景

TDLib 有两层外部接口：C++ 对象 API（`td::td_api::object_ptr`、`Td::send`）与 C ABI（`td/telegram/td_json_client.h` 的 10 个导出符号）。`tdjson` 被 CMake 编成 SHARED（`CMakeLists.txt:1508`）。

## 决定

Rust 实现只替换 **`libtdjson` 这一层**：导出同一组 C 符号，输入输出是 JSON 字符串。等价的判据是同一进程加载两个 dll、同一 JSON 输入、逐字节比输出（`tools/tdjson_diff.py`）。

## 否掉的方案

- **C++ 对象 API 边界**：要匹配名字改编、`td::string`、模板实例与 vtable 布局，Rust 侧不可行；`test/tdclient.cpp` 的 139 处 `td::td_api::` 也说明这套测试根本无法横向复用。
- **内部模块级等价**（`tddb` / `td/mtproto` / `tdactor` / `tdutils`，合计 6.4 万行）：同样落在 C++ ABI 之下，见 backlog B2。

## 后果

- 好处：对象级可替换、无测试移植、双方都能被同一个 harness 驱动。
- 代价：覆盖只到 JSON 表面；有状态客户端路径（`td_send`/`td_receive`）需要测试 DC 账号，见 backlog B1。
- 互操作证明的是「可替换」，不是「行为相同」——行为仍要逐格比字节（F004 的 CONTROL 组保证仪器能区分）。