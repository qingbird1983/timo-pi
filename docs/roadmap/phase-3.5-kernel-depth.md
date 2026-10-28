# Phase 3.5：内核深化——编排层与交互数据层对齐 pi

- 状态：✅ 完成（2026-10-28，tag `phase-3.5.end`）。P0（批 1~5，D1/D2/D4/D7）、P1（P1.1~P1.4，D3/D5/D4 缓存半场）、D6（详设 08 + 四批）与 P2 三题（09 模板、10 provider composer/虚拟模型、11 codemode 批 A~C）全部落地并真机验收；P2 第四题（项目信任门控）撤回（[ADR-0015](../adr/ADR-0015-no-project-trust.md)）。报告 `docs/design/phase-3.5/report.md`。**对应批次设计文档定稿前禁止写实现代码**（standards/03）。
- 前置：Phase 3 整体验收通过（tag `phase-3.end`）。Phase 4（GUI 插件）顺延至本阶段之后，编号不变（ADR-0013 §决策 2）。
- 参考估时：3~6 周
- 关联：ADR-0011/0013；research/01 §5、research/06；architecture/02/03

## 1. 目标（一句话）

把 pi coding-agent 内核侧的"灵魂件"逐件复刻到位——项目上下文、提示词工程机器、skills、用量与缓存、compaction、无头模式——使内核与 pi 的行为语义对齐到"pi 生态扩展可移植"的程度；**TUI 的皮不要，交互逻辑与数据流要**（ADR-0013）。

## 2. 背景与依据

2026-10-27 缺口审计（对照 pi 1.4.0 `coding-agent/src/core/`）结论：Phase 1/2 复刻了循环、工具管线、会话、协议、MCP、扩展钩子；但以下功能面缺失，其中 skills、cache-warmer、provider-composer、prompt-templates 四件此前**未登记 backlog**（治理漏洞，本轮以 ADR-0013 归位）。pi 官方 README 明言不内置 sub-agents——子代理是其扩展示例（`examples/extensions/subagent`），建立在无头模式、registerTool、模板文件等接缝之上；这些接缝正是本阶段要补的。

## 3. 范围（三级优先 + 前置调查）

### R1 前置调查（开工第一批，产出转定稿后才进详设）

- research/06：pi 交互逻辑 / 数据流向 / 提示词工程调查（含 TUI 交互逻辑层，换皮不换理）。

### P0——没有它就不像 agent

| 项 | pi 参照（packages/coding-agent/src/） | 说明 |
|---|---|---|
| 项目上下文加载 | core/system-prompt.ts + config.ts | AGENTS.md / README.md / `.pi/` 文档 → contextFiles 注入系统提示 |
| system-prompt 组装机器 | core/system-prompt.ts（229 行） | sections / custom / force / append / 工具 loadout（selected、hidden、snippets、guidelines）；对齐扩展可改写面 |
| skills | core/skills.ts（514 行） | SKILL.md 规范加载（名称/描述/ignore 规则）+ 提示词注入 |
| usage-totals | core/usage-totals.ts | 用量聚合（含嵌套工具调用合并） |
| slash-commands 注册面 | core/slash-commands.ts | 内建命令语义对齐（扩展 registerCommand 已在 Phase 2 覆盖） |

### P1——长会话必需

| 项 | pi 参照 | 说明 |
|---|---|---|
| compaction | core/compaction/（3,400+ 行） | 自动上下文压缩 + 分支摘要；`session.compaction` entry 格式已预留（ADR-0007） |
| cache-stats / cache-warmer | core/cache-stats.ts、cache-warmer.ts | 前缀缓存命中统计与主动预热（Phase 1 仅透传 usage 字段） |
| session-export | core/session-export.ts | 会话导出 |
| 会话存储按项目分区 | core/session-manager.ts（`getDefaultSessionDirPath`） | 会话目录按 cwd 编码分区（对齐 pi `~/.pi/agent/sessions/<encoded-cwd>/`）；timo-session 目前只记录 cwd 不分区存储 |
| 无头模式 | modes/print-mode.ts、json-event.ts | `--mode json -p` 等价物；ADR-0009（内核即库）兑现；也是 D6 的子进程载体 |

### P2——高级编排

| 项 | pi 参照 | 说明 |
|---|---|---|
| prompt-templates | core/prompt-templates.ts（325 行） | frontmatter 命令模板（D6 工作流预设依赖） |
| provider-composer / virtual-models | core/provider-composer.ts（737 行）、virtual-models.ts | 模型路由编排、虚拟模型 |
| codemode + nested-tool-calls | core/nested-tool-calls.ts | 模型写脚本调工具；QuickJS 底座已就位（pi-codemode 参照） |
| ~~trust-manager / project-trust~~ | core/trust-manager.ts | **撤回（ADR-0015）**：产品不做 VS Code 式工作区信任，项目资源无条件加载 |

### 不做

- TUI 任何皮肤（ADR-0002）；GUI 原生插件（Phase 4 / B 轨）；会话树/分叉的用户功能面（backlog，属 GUI 轨）；chord/durable/env（ADR-0008）。

## 4. 文档交付物

- research/06 定稿（R1 完成，2026-10-27：交互语义清单、数据流图、提示词工程合同、会话工作流、审批对照）。
- `design/phase-3.5/00-overview.md` + 按模块拆分详设（单文件 ≤500 行），全部定稿后开工。P0 详设 00~03 已定稿（2026-10-27）；P1 详设 04~07 已定稿（compaction/缓存/无头/会话存储）；D6 标尺详设 08 已定稿（subagent 扩展）；P2 详设随批次补写：09 已定稿（prompt 模板，含 D56 workflow 预设），10 已定稿（provider composer / 虚拟模型），11 已定稿（codemode 与嵌套工具调用）；12（项目信任门控）已撤回（ADR-0015）。

## 5. 验收场景

| # | 场景 | 通过标准 |
|---|---|---|
| D1 | 放有 AGENTS.md 的项目里开会话（CLI 与桌面各一次） | 系统提示词包含项目上下文，回答行为受其约束 |
| D2 | 加载符合 pi skills 规范的 SKILL.md | 技能进入系统提示，按描述可被触发 |
| D3 | 长会话触发 compaction | 自动压缩；摘要按 `session.compaction` entry 入会话树；恢复后行为一致 |
| D4 | 用量与缓存统计 | 每轮 usage 与缓存命中可查询，嵌套调用用量正确合并 |
| D5 | 无头模式 | `timo --mode json -p "<任务>"` 输出 JSON 事件流；中断/退出码语义对齐 pi |
| D6 | **subagent 式扩展标尺**：以 pi 官方 `examples/extensions/subagent` 为行为蓝本自研等价扩展（MIT 只读不抄，铁律 7） | 子代理工具经扩展注册生效；子进程经无头模式隔离上下文运行；.md 代理定义（tools 白名单 + model）生效；子代理用量汇报回主会话 |
| D7 | CLI 回归 | Phase 1/2 全部既有测试不改一字全绿 |

## 6. 方向纪律

同 Phase 1/2（ADR-0011）：一切行为语义以 pi 1.4.0 为唯一蓝本，偏离逐项登记；"更好的做法"只进 backlog。本阶段新增纪律（ADR-0013）：**交互逻辑、数据流向、提示词工程属于复刻范围**，与代码行为同等对齐。
