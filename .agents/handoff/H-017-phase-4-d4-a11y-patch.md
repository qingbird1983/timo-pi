# H-017 D4 无障碍崩溃：本地补丁（ADR-0018）与真机复验

- 日期：2026-10-30
- 主题：所有者拍板「打本地补丁」，修复 D4 崩溃并复验；Phase 4 余项待补。

## 本轮做了什么

- **判重层确认**：读 `accesskit_consumer 0.38.0` 源码，`seen_child_ids` 只遍历当次 `TreeUpdate` → 重复在**更新内部**；更正了上一轮「持久树残留」与更早「重复 id 由 GPUI 生成」的判断。
- **补丁**：`vendor/accesskit_consumer/`（0.38.0 副本，`PATCH.md` 记录出处/改动/维护；MIT 许可）；根 `Cargo.toml` `[patch.crates-io]` + `workspace.exclude`；`State::update` 先 `strip_duplicate_children`（保留首个父节点）+ `log::warn!`；新增单测。ADR-0018 已定稿并登记 `docs/README.md`。
- **复验**：探针版构建（临时 `std::eprintln!`，已移除）在面板聚焦 + UIA 反复连接/断开 + 查询串逐字变化 + 最小化/还原时记录 `stripped 3 duplicate child reference(s)`——同一更新旧版必 panic；补丁版无 panic、进程存活。正式构建无探针输出。
- **门禁**：fmt、clippy `-D warnings`、workspace **526 tests**、`check-size` 全绿；vendor 自带测试 `206 passed`（单独跑）。
- **文档同步**：report/D4、P-022、CHANGELOG 0.1.118、CONTEXT、两份 roadmap、docs/README。

## 遗留与下一步

1. 补 D4 其余子项：看门狗超时（E742）与 60s 两次崩溃自动停用的真机触发；卸载后无残留进程读数。
2. 补 D1「无重启即时替换」干净录像；然后复跑 D1~D4，再打 `phase-4.end`。
3. 上游（NSH/gpui-fast）仍建议报 issue；上游修复后按 `PATCH.md` 移除补丁。
