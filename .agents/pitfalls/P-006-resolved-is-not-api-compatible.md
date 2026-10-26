# P-006：依赖解析成功不等于 API 兼容

## 现象

仓外探针用 `cargo generate-lockfile` + `cargo tree` 确认 gpui-kit、gpui-fast、NSH 可以解析到同一依赖图，看起来版本共存成立；真正 `cargo check` 时 NSH 编译失败：`nsh-platform-gpui 0.2.0/src/chrome.rs:1211`（0.3.0 为 `:1316`）E0716，`KeyBinding::new` 在 gpui-fast 0.1.12 中要求 `'static`，传入临时字符串。

## 根因

- Cargo 的版本约束（`^0.1.9` 允许 0.1.12）只保证**能解析**，不保证跨 crate 的公开 API 仍匹配。
- 快速演进的 0.x 依赖常在小版本改签名；参考项目锁的 0.2/0.1.9 与当前解析结果不同。
- 把“依赖树单一版本”误当成“可以一起编译”。

## 修复

- 把 gpui-fast 钉到 `0.1.10`（其 `KeyBinding::new` 仍接受 `&str`）；Cargo 解析器随之选择 gpui-kit 0.8.0 家族（0.8.1 家族要求 0.1.12）。该组合的 `cargo check` 通过。
- 失败版本、行号与错误码写入设计 00/03 与阶段风险表，避免后续会话再试 0.8.1/0.1.12。

## 预防

- 任何跨 crate 的关键组合（UI 框架 + 嵌入运行时、SDK + 传输层）必须在仓外做**编译探针**，不是只做 `cargo tree`。
- 探针要引用真实类型/函数（本例 `AppView::new` + `Button`），空 `main` 只能验证依赖下载。
- 快速演进依赖的版本结论必须带日期与失败证据；升级时重跑探针。
