# 调研 06：pi 交互逻辑、数据流向与提示词工程调查（计划 + 初步地图）

- 状态：**草稿**（调查计划已立项 ADR-0013；Phase 3.5 开工首批执行，完成后本篇转定稿）
- 日期：2026-10-27
- 关联：ADR-0011/0013；roadmap/phase-3.5 §3-R1；research/01（代码分层）、research/02（扩展机制）

## 1. 为什么调查 TUI：换皮不换理

本项目不移植 pi 的 TUI 皮肤（ADR-0002），但桌面壳必须继承 pi 的**产品交互逻辑**：消息如何排队与中断、工具过程如何呈现、何时压缩上下文、模型/技能如何切换。这些语义目前只存在于 pi 的 TUI 代码与会话编排层里。不调查清楚，GUI 壳就会各做各的，偏离"以 pi 为蓝本"的方向纪律（ADR-0013）。

## 2. 调查维度与产出物定义

| 维度 | 要回答的问题 | 产出物 |
|---|---|---|
| 交互逻辑 | 用户输入何时 steer / 何时 followUp？工具过程卡片如何聚合与展开？diff 审阅的确认/拒绝/取消语义？compaction 由什么触发？slash 命令、模型切换、扩展注入的交互面（extension-input/selector/custom-editor）如何工作？ | 《交互语义清单》：逐条"pi 行为 → timo GUI 对应面"，每条挂 Phase 3.5 或 GUI 壳的验收点 |
| 数据流向 | 输入 → 队列 → agent-session → 事件 → 渲染的完整链路？流事件如何驱动视口（滚动、节流）？工具执行数据如何变成卡片内容？会话持久化与恢复在交互中的落点？ | 数据流图（Mermaid，2~3 张：主链路、工具执行、compaction） |
| 提示词工程 | system-prompt 的组装顺序与每个扩展点？skills 如何注入与触发？模板 frontmatter 字段全集？扩展改写提示词的钩子时序？ | 《提示词工程清单》：组装管道图 + 扩展点表，作为 P0 详设的对照合同 |
| 会话工作流 | `/tree` `/fork` `/clone` 的交互语义（探索另一方案不丢主线）；分支摘要（branch_summary）如何呈现；会话按项目目录分区的用户体验 | 会话工作流语义清单（pi docs/sessions.md 为纲），供树形 UI 与存储分区设计 |
| 审批交互 | pi 内核**零审批**哲学（security.md：不逐调用审批，安全靠隔离，策略靠扩展）；官方示例的确认交互（permission-gate 的 ui.select、confirm-destructive、protected-paths、timed-confirm、sandbox）；对照主流 agent 的档位设计（单次/会话/永久 × 询问/自动/完全） | 《审批交互对照表》：pi 蓝本 + 档位建议，供桌面壳审批 UI 与 allowlist 持久化设计（承接 backlog"审批档位与权限模式"） |

产出物直接回填 `design/phase-3.5/` 详设与桌面壳改进清单；**禁止复制 pi 代码与文案**（铁律 7，MIT 只读）。

## 3. 源码地图（pi 1.4.0，初扫于 2026-10-27）

### 3.1 交互层（modes/interactive/，62 个 ts 文件）

| 文件/目录 | 规模 | 职责 | 调查优先级 |
|---|---|---|---|
| interactive-mode.ts | 7,105 行 | 主编排：输入处理、命令分发、渲染调度（pi 扩展难移植的根源之一，standards/08 §4 已记） | 高——按主题切片读，不整读 |
| tui-renderer.ts | — | 组件树 → 终端渲染；视口管理 | 高 |
| chat-viewport.ts | — | 聊天视口：滚动语义、截断、锚定 | 高（GUI 壳聊天流的直接蓝本） |
| components/ | 40+ 组件 | assistant-message、bash-execution、diff、branch-summary、compaction-summary、model-selector、login-dialog、footer、extension-input/selector/editor、custom-editor 等 | 中——挑交互承载件，跳过装饰件（easter-egg、logo 等） |
| external-editor.ts | — | 外部编辑器交互 | 低 |
| session-share.ts / model-search.ts / model-catalog-refresh.ts | — | 分享、模型搜索 | 中 |

### 3.2 交互数据层（core/）

| 文件 | 规模 | 要点 |
|---|---|---|
| agent-session.ts | 4,458 行 | steering / followUp / asides 三种队列；`QueuedInputDisposition`；运行中输入的路由规则——**timo-agent 的 PendingMessageQueue 已对齐其中之一，需全面核对** |
| compaction/ | 3,400+ 行 | 触发时机、分支摘要与 UI 的联动 |
| slash-commands.ts / prompt-templates.ts / skills.ts | 325/514 行 | 命令与技能的加载、触发、提示词注入 |
| system-prompt.ts | 229 行 | 组装管道与扩展点（P0 合同） |

## 4. 已知初步事实（避免重复劳动）

- steering/followUp 队列：timo-agent 已实现（Phase 1），交互层缺口在"UI 如何呈现 pending 消息"。
- usage/缓存字段已在 timo-ai 透传，缺的是聚合统计与预热（P1）。
- 会话格式自有（ADR-0007），数据流图的持久化段需按 timo 格式重画，不照抄 pi。
- pi 的 subagent 扩展示例证明：无头模式 + registerTool + md 定义是子代理的全部接缝（Phase 3.5 D6 验收）。

## 5. 执行方式

Phase 3.5 开工第一批执行（1 周量级）：按 §2 三维度分片调查 → 产出物写入本篇（替换 §4"初步事实"并扩为正式结果，或拆 06a/06b）→ 转定稿 → 进入详设。调查期间遵守体量预算（本篇 ≤300 行，超出即拆分登记）。
