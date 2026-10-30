# Timo patch: accesskit_consumer 0.38.0

- 来源：crates.io `accesskit_consumer 0.38.0`（sha256 `5d10a236f96f87d70732e44520046785431ef01d5bcd6b041317bfadd2f88245`）
- 上游：<https://github.com/AccessKit/accesskit>
- 许可证：MIT OR Apache-2.0（见上游 `Cargo.toml`）；本副本按 MIT 使用，全文见 `LICENSE-MIT`
- 决策：`docs/adr/ADR-0018-vendored-accesskit-consumer-patch.md`
- 仅在根 `Cargo.toml` 的 `[patch.crates-io]` 生效，不参与 fmt/clippy/test 门禁

## 改动

1. `src/tree.rs` `State::update`：先调用 `strip_duplicate_children`，把同一次 `TreeUpdate`
   中重复出现的 child 引用裁掉（保留首个父节点），并 `log::warn!` 记录次数；
   替代原先在 `seen_child_ids.insert` 处的 panic。
2. `src/tree.rs` 新增 `strip_duplicate_children` 与其单元测试（`strip_duplicate_children_keeps_first_parent`）。
3. `Cargo.toml`：新增 `log` 依赖（`default-features = false`，保持 no_std）。

## 维护

- 升级 `accesskit_consumer` 时：取新版本源码覆盖本目录，重新应用上述改动，跑
  `cargo test --manifest-path vendor/accesskit_consumer/Cargo.toml`，并复验 D4 场景。
- 移除条件：上游在 `State::update` 中容忍重复 child（或 NSH/gpui 不再产生重复），
  届时删除本目录与 `[patch]` 条目，并在 CHANGELOG 登记。
