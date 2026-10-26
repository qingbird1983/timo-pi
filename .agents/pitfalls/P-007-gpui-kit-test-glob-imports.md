# P-007：gpui-kit 测试里 `use gpui_kit::*` 会撑爆 `#[gpui_kit::test]`

- 发现：2026-10-27，`apps/timo-desktop/tests/ui.rs` 首次接入 UI 集成测试。
- 现象：编译报 `recursion limit reached while expanding #[test]`，把 `recursion_limit` 从 256 一路加到 512/1024 仍报同一错，指向 `#[gpui_kit::test]` 本身。
- 根因：`use gpui_kit::*;` 把 GPUI 的 `test` 宏带进作用域，覆盖了 Rust 内置 `#[test]`；`#[gpui_kit::test]` 展开出的 `#[test]` 被解析成 GPUI 的测试宏，自递归。加大递归上限只是把爆栈推迟，方向就错了。
- 修复：测试模块**逐项显式导入**（`AppContext`、`Bounds`、`WindowOptions` 等），不用 glob；并保留 `use gpui_kit::test::TestWindowExt as _;`。
- 预防：gpui-kit 官方 test 指南已写明此禁令；新增 UI 测试文件时先照抄 `apps/timo-desktop/tests/ui.rs` 的导入块。同类“错误信息指向宏、根因在 import”的情形，先查作用域再调编译器参数。
