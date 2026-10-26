# 工作上下文交接卡

- 更新：2026-10-27（会话主题：**Phase 3 实现第 1 批：桌面壳最小窗口**）

## 当前阶段与状态

- **Phase 1 ✅**（tag `phase-1.end`）；**Phase 2 ✅**（tag `phase-2.end`）。
- **Phase 3 进行中**：设计定稿（tag `phase-3.start`）；实现第 1 批完成——`apps/timo-desktop` 最小窗口可运行。
- 设计：`docs/design/phase-3/00-overview.md` ~ `04-tool-review.md` + `prototype.html`；决策 ADR-0012。
- 看板：`docs/roadmap/00-roadmap.md` §1；阶段文档：`docs/roadmap/phase-3-desktop-shell.md`。
- 远端：`github.com/qingbird1983/timo-pi`（私有）。

## 上一轮完成了什么

- **桌面壳骨架**：`apps/timo-desktop` 进 workspace；`gpui-kit 0.8.0` + `gpui-fast/gpui-fast-platform =0.1.10` 实锁；NSH 0.3.0 仅 `nsh-poc` feature（默认不编译）；rust-i18n 4.2.4。
- **最小窗口**：自绘标题栏（窗口标题 `Timo`）、248 侧栏、聊天空态、底部状态栏；1280×800，最小 960×600；明暗切换走主题 token。
- **文案**：`locales/zh-CN.yml`、`en.yml`，默认中文；代码不写死中文。
- **UI 冒烟**：`tests/ui.rs` 无头渲染、侧栏宽度 248、明暗往返回位、中英文案切换；全套 186 tests 绿。
- **真机核对**：1280×800 明/暗两态截图；修正窗口标题显示 exe 路径、状态栏中置的问题。
- **坑**：P-007（测试文件 glob 导入导致 `#[gpui_kit::test]` 自递归）。

## 下一步（按优先级）

1. **K0/K1**：`run_agent_loop` 返回完整上下文；`AgentHandle::run` 公共返回值不变；Kernel 保存完整上下文。公开 `new_session()`（草稿不落盘）、`history()`、`delete_session(id)`。既有测试一字不改全绿。
2. **K2**：审阅接缝 + CLI `None` 等价性测试（ADR-0012）。
3. 内核桥接 + 会话侧栏/聊天流/工具 trace/输入取消（设计 00 §8 第 4 步）。
4. diff 审阅面板 + 命令面板 + 设置/钥匙串。
5. NSH PoC（`nsh-poc` feature）+ C1~C6 验收 → `report.md` → tag `phase-3.end`。

> 施工顺序唯一来源：`docs/design/phase-3/00-overview.md` §8。设计错误先改文档再写码。

## 未决问题

- NSH 0.3.0 + gpui-fast 0.1.10 的**运行期**行为（渲染/热更）尚未验证，只有编译探针。
- C4 官方 CLI 生成 v1/v2 包的具体命令参数（`nsh keygen/certify/publish/verify`，实现期验证）。
- 审阅“本会话始终允许”是否跨会话持久化：本阶段只做会话内。
- 助手 Markdown 长文/代码块体验、会话重命名：留待真实使用驱动。
- `target/` 已 23GB（软上限 15GB），`cargo-sweep` 未安装；下次空闲时按龄清扫。

## 关键提醒

- **内核零 GUI**：`crates/` 不得出现 gpui-kit/NSH；桌面只 import `timo-kernel`。
- **审阅前无副作用**：拒绝必须不写文件、不 spawn；等待中取消/关窗 → 拒绝。
- **CLI 等价**：`tool_review = None` 时 Phase 1/2 测试一字不改全绿。
- **版本只认 registry 源码签名**；0.8.1/0.1.12 下 NSH 报 E0716，锁定 0.8.0 家族 + 0.1.10。
- **UI 测试禁用 `use gpui_kit::*`**（P-007），照抄 `apps/timo-desktop/tests/ui.rs` 导入块。
- **门禁四连**：fmt / clippy / `cargo test --workspace` / `check-size`；全量测试慢，用 `CARGO_BUILD_JOBS=1`。
- **写中文文件后扫隐形字符**：`grep -nP '[\x{3040}-\x{30ff}\x{200b}-\x{200f}\x{feff}]'`。
- **收尾四件事**：重写本卡、归档 handoff、沉淀 pitfalls/skills、体量核对。
- 踩坑索引：P-001 流事件静默回归；P-002 编辑重复行；P-003 Windows bash 是 WSL；P-004 上游摘要当合同；P-005 模块格式与运行时不符；P-006 依赖解析成功 ≠ API 兼容；P-007 gpui-kit 测试 glob 导入。
