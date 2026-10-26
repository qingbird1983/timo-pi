# 研究 03：桌面技术栈评估（gpui-kit + NSH）

- 状态：定稿
- 日期：2026-10-26
- 调查对象：gpui-kit 技能文档；`F:\Warehouse\Github\nsh`（GitHub: longbridge/nsh）

## 1. gpui-kit

- Zed 的 GPUI 框架之上的组件库，应用只依赖 `gpui-kit` 一个 crate，分层：`component`（样式组件）/ `base`（无样式行为）/ `assets` / `platform`。
- 与编码代理桌面端高度契合的现成组件：`Message`/`Bubble`/`MessageScroller`/`Attachment`（聊天流）、`Diff`（diff 审查）、`DataTable`/`Tree`/`List`、代码 `Editor`（tree-sitter）、`Command` 命令面板、`Dialog`/`Sheet`/`Notification`、`Settings`、`Resizable`、图表。
- 自带 UI 集成测试基建（`#[gpui_kit::test]`、无头窗口、输入模拟）。
- 设计规约严格（状态所有权、ElementId 稳定标识、框架管行为/应用管呈现），有配套 design/coding guides，适合 AI 协作开发。

## 2. NSH（Native Shell）

- **定位**：Longbridge 出品的跨平台原生 UI 框架（Rust，约 46 万行），"Build once, native everywhere"：iOS/Android/macOS/Windows/Linux/Web/**GPUI**。**与 pi 无任何关系**（全仓无关联痕迹）。
- **对本案有价值的三件事**：
  1. **内嵌 QuickJS**（quickjs-jit + LLRT 衍生 stdlib：buffer/crypto/path/zlib）——本项目扩展宿主的运行时基础；
  2. **TSX 工具链**：rolldown + oxc 打包器（无需 Node）、运行时下发热更新的 **ed25519 签名包**、LayerFS、**cap-std 能力沙箱**——本项目 GUI 插件面的分发/沙箱基础，也是编排扩展"安装期打包"的现成工具；
  3. **GPUI 后端**：`nsh-platform-gpui` 基于 gpui-kit 生态同款 `gpui-fast`，`AppView` 可作为 GPUI view 直接嵌入 gpui-kit 应用元素树（官方 `examples/embed-gpui` 即 gpui-kit 应用内嵌 NSH 的示例）。
- **风险**：公开 git 历史仅约 12 天（2026-10-14 首次提交），v0.2.0，API 处于 0.x 快速演变期；CI/测试完善（13 workflows、无头测试框架、截图测试）。**对策**：锁定版本使用；NSH 相关面收敛在 Phase 4 与插件系统内，内核不依赖 NSH。

## 3. 组合结论

| 层 | 选型 | 职责 |
|---|---|---|
| 应用骨架 / 聊天流 / diff 审查 | gpui-kit | 主界面（成熟、组件齐） |
| 动态/热更 UI 面 | NSH | 插件界面、主题、AI 生成面板（TSX 签名热更包） |
| 扩展 JS 运行时 | NSH 同款 QuickJS | 编排扩展宿主 + TSX 插件运行时 |

- 集成路径官方支持（AppView 嵌入），风险点为 NSH 年轻 → 用版本锁定 + 隔离层控制。
- 决策记录见 ADR-0002、ADR-0004、ADR-0006。
