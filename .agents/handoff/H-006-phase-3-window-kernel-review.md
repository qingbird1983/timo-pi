# H-006：Phase 3 实现第 1~3 批（窗口 · K0/K1 · K2）

- 日期：2026-10-27
- 主题：桌面壳最小窗口、内核多轮上下文与会话 API、工具审阅接缝。

## 本轮做了什么

1. **桌面壳最小窗口**：新增 `apps/timo-desktop`；gpui-kit 0.8.0 + gpui-fast 0.1.10 实锁，NSH 0.3.0 挂在 `nsh-poc` feature；rust-i18n 4.2.4 中英文案。标题栏/248 侧栏/聊天空态/状态栏、1280×800（最小 960×600）、明暗切换。
2. **UI 验证**：`tests/ui.rs` 无头渲染 + 侧栏宽度 + 明暗往返 + 双语切换；真机 1280×800 明/暗截图核对，修掉窗口标题显示 exe 路径、状态栏中置。
3. **K0**：`run_agent_loop` 返回 `RunOutcome`（新增消息 + 完整上下文），`AgentHandle::run` 返回形状不变；Kernel 先 clone 历史、成功后回填，前置失败不丢历史。agent 级两轮上下文测试 + Kernel 回填单测。
4. **K1**：`new_session()`（草稿不落盘）、`history()`、`delete_session(id)`（防路径穿越；删当前会话回草稿）。集成测试覆盖 resume→删除→草稿与非法 id。
5. **K2**：`ToolReview`/`ReviewKind`/`ReviewDecision`；`KernelOptions.tool_review` → `LoopConfig.review` → `ToolCallCtx.review`。write/edit 在路径锁内算 after 后审阅，命令在 spawn 前审阅；拒绝/取消/panic 无副作用；CLI `None` 走原路径。新增接受/拒绝/取消/panic/二进制/匹配失败不审阅/命令未 spawn/接线测试。
6. **体量**：kernel 会话 API、装配选项、edit 匹配逻辑拆到子模块，全部 ≤500 行。
7. **门禁**：fmt / clippy / 200 tests / check-size 全绿（`target/` 27GB 触发软上限 warn）。

## 遗留与下一步

- 内核桥接（专用 tokio 线程、命令通道、事件转发、审阅 oneshot）→ 会话侧栏 + 聊天流 + 工具 trace + 输入/取消。
- diff 审阅面板（`similar` → `DiffFile::parse`）、命令面板、设置/钥匙串。
- NSH PoC（`nsh-poc` feature）与 C1~C6 验收。
- `cargo-sweep` 未安装，按龄清扫待做。

## 已提取的沉淀

- P-007：gpui-kit 测试文件禁用 `use gpui_kit::*`（会让 `#[gpui_kit::test]` 自递归）。
