# ADR-0018：本地修补 accesskit_consumer 以消除 D4 无障碍崩溃

- 状态：定稿
- 日期：2026-10-30
- 关联：Phase 4 验收 D4（`docs/design/phase-4/report.md`）、P-022、standards/04（依赖准入）、standards/08（体量与豁免）

## 背景

Phase 4 真机验收中，插件面板在场且输入框聚焦时，只要系统无障碍接口被激活（UIA 客户端连接/断开、原生选目录对话框）并发生重绘，宿主在 `gpui_fast_windows` → `accesskit_windows` → `accesskit_consumer` 的 `State::update` 处 panic：`TreeUpdate includes duplicate child`，窗口卡死。

插桩定位（`target/` 临时副本，已删除）：

- 重复 child id 均是 NSH 合成节点（`hash(("nsh", surface_key, view_id))`），即 NSH 焦点代理与列表数据移动的组合产物；
- 崩溃当帧 gpui-fast 侧交给 consumer 的 `TreeUpdate` 内部就含重复（`seen_child_ids` 只遍历 `update.nodes`），builder 之外的持久树不是来源；
- 上游核查：`nsh-platform-gpui` 最新 0.3.0（当前锁定）；`gpui-fast` 0.1.12 的 a11y 代码与 0.1.10 相同、依赖 `accesskit 0.24` / `accesskit_windows 0.34`；`accesskit_consumer` 0.38.0 是这条依赖线的锁定版本。没有「升级即修复」路径。

所有者决定采用本地补丁（2026-10-30），并授权按最小改动实现。

## 决策

1. **修补在 consumer 边界，而不是 NSH 或 gpui-fast**：`State::update` 是唯一能看到整份 `TreeUpdate`（跨元素、跨根）的层；NSH 的 `synthetic` 看不到别的元素的节点，gpui-fast 要整包 vendor 5MB+。
2. **vendor `accesskit_consumer 0.38.0` 到 `vendor/accesskit_consumer/`**，用根 `Cargo.toml` 的 `[patch.crates-io]` 指向它；版本号保持不变，不做行为以外的 API 改动。
3. **补丁语义：消毒而非 panic**——更新前先裁掉重复的 child 引用（保留首个父节点），`log::warn!` 记录条数；上游后续若修复，本补丁可整目录删除。
4. **许可证**：上游双许可 MIT OR Apache-2.0，本副本按 MIT 使用，保留 `LICENSE-MIT` 与出处说明（`PATCH.md`）。
5. **门禁范围**：`vendor/accesskit_consumer` 通过 `workspace.exclude` 排除在 fmt/clippy/test 之外（第三方代码不参与自有门禁）；其自带测试用 `cargo test --manifest-path vendor/accesskit_consumer/Cargo.toml` 单独跑。

## 理由

- 崩溃是**进程级**的：读屏/自动化用户一旦触发，窗口卡死且需强杀，属于产品级缺陷，不能留到上游修复。
- consumer 的判重发生在应用更新前，消毒点唯一且确定；与 gpui-fast `repair_tree_update`（已有「先修复再交给 accesskit」的先例）同构。
- 改动面最小：一个 crate、一个函数、约 20 行；其余 9.8k 行原样 vendor，便于按 `PATCH.md` 重放过。
- 拒绝改 NSH 的直接原因：NSH 内部看不到跨根重复，且当前无法稳定复现到具体生成点；拒绝改 gpui-fast 的直接原因：整包 5MB 且 a11y 代码与锁定版本一致，没有更小的切入点。

## 后果

- 正面：D4 崩溃路径被确定性封住；不改变任何公开 API；升级 `accesskit_consumer` 时按 `PATCH.md` 重放即可。
- 代价：仓库新增约 412K / 9.9k 行第三方源码（不计入 standards/08 的 500 行与 `check-size` 扫描，按本 ADR 登记豁免）；被消毒的那一帧里，重复 child 只保留在首个父节点下，无障碍树可能缺一个分支，直到下一帧重建。
- 维护：`accesskit_consumer` 升级或 NSH/gpui 修复后复验 D4；上游仍未修时每次升级都要重放补丁（`PATCH.md` 记录步骤）。
- 仍建议向 NSH/gpui-fast 上游报 issue：本补丁防崩，不修正树本身。
