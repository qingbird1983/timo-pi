# ADR-0009：内核即库，GUI 与 CLI 皆为薄壳

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0001、ADR-0002；architecture/01

## 背景

pi 的多模式（interactive TUI / rpc / print / json）与内核耦合在同一 CLI 包内。本项目主交付物是桌面端，但仍希望保留无界面自动化（脚本、CI、未来 headless）能力。

## 决策

- 内核实现为纯库 crate（`timo-kernel` 门面 + 各功能 crate），**不依赖任何界面框架**。
- `timo-desktop`（gpui-kit）与 `timo-cli`（非交互执行雏形）都只是内核 API 的薄壳；GUI 特有状态不进入内核。
- 事件总线是内核状态外泄的**唯一**通道：GUI、CLI 日志、扩展宿主一律作为订阅者消费（architecture/02）。

## 理由

- 保证内核可独立测试（无头 CI）、可被未来 print/json/headless 模式复用（backlog）。
- 防止 UI 需求反向污染内核数据模型（pi 的 ctx.ui 与内核耦合是其扩展难以移植的根源之一）。

## 后果

- 凡"内核需要知道界面"的设计提案一律否决；界面诉求经事件/订阅或 GUI 壳自行状态解决。
- Phase 1 的 timo-cli 仅作冒烟验收载体，不投入打磨交互体验。
