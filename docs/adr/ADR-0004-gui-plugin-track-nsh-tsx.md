# ADR-0004：GUI 原生插件面（B 轨）采用 NSH TSX 自建

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0002、ADR-0003；research/03、research/04；architecture/04

## 背景

编排兼容（A 轨）之外，GUI 端需要自己的插件 UI 能力（自定义面板、工具卡片渲染、主题）。参考项目 PI-Desktop 的 renderer slot 依赖 Web DOM + React import-map，**不可平移到 GPUI**。

## 决策

自建 GUI 插件面，技术选型全部复用 NSH 基础设施：

- 插件 UI 用 **TSX** 编写，经 **rolldown/oxc** 打包，运行于 **QuickJS** 沙箱，经 NSH GPUI 后端渲染进 gpui-kit 窗口。
- 分发用 NSH **ed25519 签名热更包**；能力授予用 **cap-std** 模型。
- 借鉴 PI-Desktop 的接口收紧思路：**宿主窗口内 slot 白名单**（toolCard / blockRenderer / composerTrigger / panel / theme 等有限槽位）+ **渲染动作词汇表白名单**，越界即拒。

## 理由

- 与 A 轨共用同一套 QuickJS/打包/签名/沙箱基础设施，维护一份。
- "TSX 写界面 + 沙箱 + 热更"恰好填补 pi TUI 扩展留下的生态位，且天然适配"AI 生成 UI"场景。

## 后果

- pi 的 UI 扩展不能直接用，需按 B 轨 API 用 TSX 重写（多为薄渲染层，重写量小）——这是明确的取舍而非遗漏。
- B 轨 API 属于本项目自有公共契约，从 Phase 4 设计起即要求版本化与弃用流程（pi 未做到 semver，本项目要做到）。
- Phase 4 需先出 B 轨详细设计文档（manifest schema、slot 清单、权限矩阵）并定稿后才动代码。
