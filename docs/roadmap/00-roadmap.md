# 路线图：总览

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0001~0009；README 进度看板（阶段状态以 README 为准）

## 1. 阶段总览与看板（本表是**唯一进度看板**）

> 自 0.1.2 起，README 不再重复看板；阶段状态只在本表维护。状态变更必须同步 CHANGELOG 与 `.agents/CONTEXT.md`。**最近更新：2026-10-27。**

```mermaid
graph LR
    P0["Phase 0<br/>仓库与文档（已完成）"] --> P1["Phase 1<br/>内核 MVP"]
    P1 --> P2["Phase 2<br/>编排扩展宿主"]
    P1 --> P3["Phase 3<br/>桌面壳"]
    P2 --> P4["Phase 4<br/>GUI 插件系统"]
    P3 --> P4
    P4 -.-> B["Backlog<br/>Phase 5+ 候选"]
```

| 阶段 | 一句话目标 | 参考估时* | 状态 |
|---|---|---|---|
| Phase 0 | 仓库初始化、规范与设计定稿、文档先行落盘 | — | ✅ 完成（2026-10-26） |
| Phase 1 | 无界面的最小可用内核：2 种协议 + 循环 + 工具 + 会话 + MCP | 6~10 周 | ✅ 完成（2026-10-26，tag `phase-1.end`） |
| Phase 2 | pi 编排类扩展可装载生效（A 轨） | 3~6 周 | ✅ 完成（2026-10-26，tag `phase-2.end`；报告 `docs/design/phase-2/report.md`） |
| Phase 3 | gpui-kit 桌面壳日常可用 + NSH 嵌入 PoC | 4~8 周 | 进行中（设计定稿 2026-10-26；最小窗口、K0/K1、K2、内核桥接与会话/聊天流已完成 2026-10-27；下一步 diff 审阅/命令面板/设置） |
| Phase 4 | GUI 原生插件系统闭环（B 轨） | 4~8 周 | 未开始 |

\* 单人全职参考值，仅量级感知。Phase 2 与 Phase 3 可并行。

## 2. 每阶段通用流程（硬流程，详见 standards/07）

1. **文档定稿**：本阶段 phase 文档及其引用设计稿从草稿转定稿（所有者知情即可，重大技术变更须 ADR）。
2. **实现**：feature 分支开发；规范全程生效。
3. **验收**：逐条执行本阶段"验收场景"，结果写入《阶段进度报告》。
4. **收尾**：进度报告 + 更新 README 看板 + CHANGELOG + ADR/架构文档回填 + 打 tag（`phase-N.end`）。

## 3. Backlog（Phase 5+ 候选，立项须先写 ADR）

> 本节同时是**创新提案的唯一通道**（ADR-0011）：内核复刻期（Phase 1~2）一切"更好的做法"只记录于此、不实施；解禁条件见 ADR-0011 §6。

- 更多协议适配：openai-responses、google-generative-ai、bedrock（SigV4）
- 供应商 OAuth 与自定义 provider 完整面（`streamSimple` 等价物）
- 会话树/分叉/navigate 的用户功能面、compaction（`session.compaction` entry 已在格式中预留）
- codemode（模型写脚本调工具，QuickJS 已就位）、tool-search（deferred 工具按需加载）
- print/json/headless 批处理模式（timo-cli 扩展）
- A 轨 v2 成员逐个转正；B 轨 slot 扩展
- pi 会话导入工具；插件 marketplace；遥测（opt-in）
- ADR-0008 所列外围能力（chord/durable/env）——默认不做

## 4. 风险登记簿（跨阶段）

| 风险 | 等级 | 对策 |
|---|---|---|
| NSH 年轻（v0.2、公开 12 天）API 漂移 | 高 | 版本锁定；NSH 依赖隔离在 Phase 3/4 层，内核零依赖；升级窗口化 |
| 协议适配正确性（无上游 SDK 背书） | 高 | fixture 录制回放测试；以 pi 协议层行为为对照 |
| 估时偏差（AI 辅助开发节奏难预估） | 中 | 以验收场景为准不赶工期；阶段可并行降低阻塞 |
| QuickJS 兼容面不及预期（某扩展打包失败） | 中 | 诊断透明化；个案评估 deno_core 升级路径（ADR-0006 备选） |
| 单点开发（无第二双眼睛） | 中 | 规范+CI 硬门槛；每阶段进度报告披露风险 |
