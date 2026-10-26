# Phase 3：gpui-kit 桌面壳

- 状态：进行中（设计定稿 2026-10-26；实现第 1 批：最小窗口已跑通 2026-10-27；下一步 K0/K1）
- 参考估时：4~8 周
- 关联：ADR-0002/0009/0012；architecture/01~03；`docs/design/phase-3/`

## 1. 目标（一句话）

一个日常可用的桌面应用：开在窗口里的编码代理，会话、流式对话、工具过程、代码变更审查全部可视化；并验证 NSH 可嵌入。

## 2. 范围

### 做
- timo-desktop（gpui-kit）：会话列表/新建/切换/删除；消息流渲染（Markdown、流式增量）；工具调用卡片（状态/参数/结果/时长）；**diff 审查**（接受/拒绝编辑，基于 gpui-kit Diff 组件）；命令面板；设置（供应商/模型/API key——密钥入系统钥匙串，standards/06）；取消按钮；错误展示
- 内核事件订阅层：GUI 作为事件总线订阅者的参考实现（ADR-0009 原则的样板）
- NSH 嵌入 PoC：AppView 渲染一个静态 TSX 面板于 gpui-kit 窗口内 + 一次签名热更包加载演示（仅为 Phase 4 铺路，不做插件系统）
- 平台：**Windows 优先**（ADR-0010）——主攻打磨；macOS/Linux 保证可构建，首发体验不打包票

### 不做
B 轨插件系统（Phase 4）；多窗口/复杂布局；主题系统（token 覆盖留待 Phase 4）；自动更新；安装器分发（打包格式 Phase 4 后定）

## 3. 文档交付物

- [00 总览与跨层决策](../design/phase-3/00-overview.md)：版本边界、模块布局、内核前置改动 K0~K3、提交序列
- [01 界面信息架构与状态流](../design/phase-3/01-ui-architecture.md)：窗口骨架、事件映射、工具卡片、diff 审阅、键盘与状态
- [02 凭证、钥匙串与桌面设置](../design/phase-3/02-credentials-and-settings.md)：引用语法、平台后端、脱敏与 C3 验收
- [03 NSH 嵌入与签名包 PoC](../design/phase-3/03-nsh-embedding.md)：AppView 隔离层、信任链、热更/回退、C4 证据
- [04 工具审阅接缝与 diff 审查](../design/phase-3/04-tool-review.md)：执行前审阅、拒绝语义、并发与 CLI 兼容
- [原型（方向稿）](../design/phase-3/prototype.html)：会话侧栏、工具过程 trace、diff 审阅
- 决策：[ADR-0012](../adr/ADR-0012-tool-review-seam.md)

## 4. 验收场景

| # | 场景 | 通过标准 |
|---|---|---|
| C1 | 日常使用清单 | 新建会话→对话（流式逐字）→观察工具卡片→diff 审查接受/拒绝→关闭重开应用后会话恢复 |
| C2 | 权限交互 | 内核的工具信任/权限检查在 GUI 有对应确认对话框（Phase 1 CLI 简版升级） |
| C3 | 密钥安全 | 设置 API key 后检查：密钥在钥匙串中、配置文件与日志中无明文 |
| C4 | NSH PoC | TSX 面板渲染于 gpui-kit 窗口内；替换签名包后面板内容热更成功；未签名包被拒 |
| C5 | 崩溃恢复 | 强杀应用后重开：会话无损（Phase 1 A3 在 GUI 下复验） |
| C6 | 接缝核对 + CI 全绿 | architecture/03 达标；gpui-kit UI 集成测试（#[gpui_kit::test]）覆盖核心交互 |

## 5. 风险与对策

- gpui-kit/NSH 任一上游破坏性变更 → 双依赖均版本锁定；隔离层（仅 Phase 3/4 代码触达）控制升级面。**版本谱系已核对（2026-10-26）**：gpui-kit 0.8.1 与 gpui-component 0.8.1 同源；NSH 0.2/0.3 在 gpui-fast 0.1.12 下均编译失败（E0716），锁 gpui-fast 0.1.10 后解析到 0.8.0 家族，编译探针已通过；pi-flash 的 0.2.x 补丁不适用。
- 平台差异（钥匙串/路径/终端）→ 平台抽象点在 design 文档中先列出。
