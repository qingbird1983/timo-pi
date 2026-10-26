# ADR-0002：桌面端采用 gpui-kit + NSH，不移植 pi 的 TUI

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0003、ADR-0004、ADR-0006；research/03

## 背景

pi 的界面是终端 TUI（pi-tui 约 2 万行 + 交互模式约 2.4 万行）。项目愿景是原生桌面端；TUI 扩展生态以 TUI 为前提，用户判断"到 GUI 端本就不适应"。

## 决策

- **不移植 pi 的 TUI**（pi-tui 与 interactive 模式均不在范围内）。
- 桌面壳用 **gpui-kit**（聊天流、diff 审查、命令面板、设置等主界面）。
- 动态/热更 UI 面（插件界面、主题、AI 生成面板）用 **NSH**（TSX + 签名热更包 + cap-std 沙箱，GPUI 后端嵌入 gpui-kit 元素树）。

## 理由

- gpui-kit 现成组件与编码代理桌面端高度契合（Message/MessageScroller/Diff/Command/Editor 等），且有 UI 集成测试基建。
- NSH 官方支持嵌入 gpui-kit（AppView 即 GPUI view，有 embed 示例），并自带本案插件系统所需的全部基础设施（QuickJS、rolldown/oxc、ed25519 签名包、cap-std）。

## 后果

- pi 的 TUI 扩展（渲染器、自定义编辑器、TUI 组件）**明确不兼容**，由 ADR-0003 的降级策略与 ADR-0004 的 GUI 插件面承接。
- NSH 公开仅约 12 天（v0.2.0），0.x API 演变快 → **版本锁定**、NSH 依赖收敛在插件系统与桌面壳的隔离层内，内核不依赖 NSH。
- 平台范围：gpui-kit/NSH 均覆盖 Windows/macOS/Linux；平台优先级待所有者决策（见 README）。
