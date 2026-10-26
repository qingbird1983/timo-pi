# ADR-0008：范围排除——chord / durable / env / telemetry 不纳入内核

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0001；research/01；roadmap/00（backlog）

## 背景

pi monorepo 中除内核与 TUI 外还有约 4.7 万行外围运行时：chord（插件宿主运行时，1.5 万行）、pi-durable（持久化会话/任务运行时，2.8 万行）、pi-env（SSH 远程执行 daemon）、pi-telemetry。

## 决策

以上四项**不纳入**本项目任何已排期阶段（Phase 0~4）。是否启动列入 backlog，触发条件（满足其一才立项，且先写 ADR）：

1. 出现"多机/团队共享会话与任务"的真实需求（durable/chord）；
2. 出现"远程机器执行工具"的真实需求（env，且其 daemon 本就是 Rust，可单独评估复用**思路**）；
3. 出现遥测分发需求（telemetry，且默认关闭、opt-in）。

## 理由

- 与"单机桌面编码代理"愿景无直接依赖；排除后内核 MVP 范围收窄约 3.5 万行对等功能。
- 过早引入会拖垮 Phase 1~4 的焦点。

## 后果

- 本项目不含多机协同、远程执行、云端持久化能力。
- 若未来立项，按规范走 ADR + 新阶段文档，不允许"顺手实现"。
