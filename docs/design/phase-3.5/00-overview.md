# 设计 00：Phase 3.5 总览与跨模块决策

- 状态：草案（2026-10-27；P0 详设 01~03 定稿时一并转定稿）
- 关联：roadmap/phase-3.5；ADR-0011/0013；research/06；design/phase-1/02~05
- 行为蓝本：pi 1.4.0（本地只读，不复制；铁律 7）

## 1. 定位

本目录是 Phase 3.5「内核深化」的实现级详设：模块边界、接口契约、加载时序、默认值与验收断言。范围以阶段文档 §3 为准，本目录不扩范围；"更好的做法"只进 backlog（ADR-0011）。

与 Phase 2 的差别：Phase 2 新增一个 crate（`timo-ext`）；本阶段**不新建 crate**——提示词与资源属于内核装配层，落在 `timo-kernel` 内部模块（见 K1）。

## 2. 一句话方案

把系统提示从一段内嵌字符串升级为**分节装配物**：上下文文件、skills、命令注册面、用量聚合由内核统一发现与组装，CLI 与桌面只消费结果；P1/P2 的压缩、缓存、无头模式、模板与模型路由随后按批落，每一批先定稿对应详设再写码。

## 3. 模块布局

| 位置 | 职责 | 批次 | 详设 |
|---|---|---|---|
| `timo-kernel/src/prompt/mod.rs` | 分节模型、渲染、节名校验、整段替换 | P0 | 01 |
| `timo-kernel/src/prompt/context.rs` | 上下文文件发现、顺序、去重 | P0 | 01 |
| `timo-kernel/src/prompt/skills.rs` | 技能发现、校验、提示格式、`/skill:` 展开 | P0 | 02 |
| `timo-kernel/src/commands.rs` | 内建 + 模板 + 技能 + 扩展命令合并 | P0（模板 P2 接） | 02 |
| `timo-kernel/src/usage.rs` | 用量与成本聚合、分桶、嵌套合并入口 | P0 | 03 |
| `timo-config/src/settings.rs` | `skills` / `prompts` / `enableSkillCommands` 键 | P0 | 01/02 |
| `timo-agent` | 不改：系统提示仍是整段字符串（K2） | — | — |
| `apps/timo-cli` / `apps/timo-desktop` | 消费命令清单与用量读数 | P0 末 | 02/03 |

后续批次（P1/P2）：`compaction/`、`cache.rs`、`headless`（mode json）、会话按项目分区、`session-export`、`prompt_templates`、`provider-composer`、`nested-tool-calls`、`trust`。各自详设编号在开工前补，不在本文预设签名。

## 4. 批次与提交序列

1. **本批**：00 总览 + P0 详设 01~03 定稿；阶段文档/看板/CHANGELOG 同步。
2. 提示分节 + 上下文文件（单测：顺序、候选名、去 BOM、工作树去重）。
3. skills + 命令注册面（单测：发现规则、校验告警、冲突先到先得、隐藏技能、`/skill:` 展开）。
4. 用量聚合（单测：分桶键、成本、缺失字段、工具/摘要桶）。
5. CLI 与桌面接线（命令菜单列出技能；状态区显示用量；D1 的桌面半场可验）。
6. P1 详设定稿 → 实现（压缩、缓存统计与预热、无头模式、会话分区与导出）。
7. P2 详设定稿 → 实现（模板、路由、codemode、信任）。
8. 验收 D1~D7 → `report.md` → 看板/CHANGELOG → tag `phase-3.5.end`。

> 每批门禁四连不变（fmt / clippy / workspace tests / check-size）；新增提示输出用快照式断言（固定输入 → 逐字期望），不只 `contains`。

## 5. 关键实现决策

| # | 决策 | 理由 / 约束 |
|---|---|---|
| K1 | 不新建 crate，模块落在 `timo-kernel`；单文件超 500 行就拆文件 | 内核是唯一装配点；新增 crate 会带来依赖边与版本负担；skills/命令只有内核与壳两个消费者 |
| K2 | 系统提示**不写入会话**；每轮按节重建并渲染为整段字符串发给模型 | 现有 wire 只接受整段 system；相同输入渲染出相同字节，提示缓存等价；省去"按节补丁"的持久化面（偏离 D30） |
| K3 | 分节规则逐条对齐 pi：`preamble` 裸文本、其余 `<节名>` 包裹、节名正则 `^[a-z][a-z0-9_-]*$`、空节跳过、`forceSystemPrompt` 整段替换 | 扩展改写面（Phase 4）依赖同一套名字与标签；无节名规则会直接拒绝 |
| K4 | 上下文文件候选名、全局优先、根到 cwd 顺序、去 BOM、工作树去重按 pi | 属于"提示词工程"复刻范围（ADR-0013）；顺序错会改变模型行为 |
| K5 | 技能目录：`<data_dir>/skills`、项目 `.timo/skills`、`.pi/skills`、`.agents/skills`（cwd→仓库根）、`settings.skills` 显式路径；同名先到先得 | 兼容 pi 生态位置，另加本项目 `.timo`；先到先得与告警对齐 pi（偏离 D31 仅目录集） |
| K6 | 技能正文不进提示；`/skill:name` 把正文展开进**用户消息**，参数追加在后 | 与 pi 一致；未知技能原样透传，不报错 |
| K7 | 命令注册面在内核：内建名 + 模板 + 技能 + 扩展命令合并，扩展仍带 `plugin_<id>_` 前缀 | 单一来源，CLI 与桌面消费同一清单；P0 只要求内建命令的**语义**，不要求 `share/bug/import/llama` 实现（D32） |
| K8 | 用量从会话 entries 推导（assistant 消息、`usage` entry、摘要 entry），按 `provider/model` 分桶，工具与摘要进 `Tools/summaries` 桶 | 与 pi 的 `usage-totals` 对齐；不新增落盘格式（`usage` entry 已存在） |
| K9 | 设置键沿用 pi 名：`skills`、`prompts`、`enableSkillCommands`；P1/P2 的 `compaction`、`defaultProjectTrust` 同样沿用 | 迁移与文档对照成本最低；未知键仍忽略 |

## 6. 与 pi 的偏离登记（接续 Phase 2 的 D29）

> 类别：a=语言/运行时必然差异；b=既有 ADR 已定载体；c=本仓库规范硬要求；d=范围收敛（验收前必须登记）。

| # | 偏离 | 类别 | 说明 |
|---|---|---|---|
| D30 | 系统提示不落会话、不走"按节补丁" | b（自有会话格式，ADR-0007） | 每轮重建整段；内容等价，缓存等价；代价是改动后首轮缓存冷启动 |
| D31 | 项目技能目录增加 `.timo/skills`，并保留 `.pi/skills`、`.agents/skills` | c（本项目配置目录） | 发现顺序：用户 → 项目 `.timo` → 项目 `.pi` → `.agents`（近到远）→ 显式路径 |
| D32 | `share`/`bug`/`import`/`llama`/`scoped-models` 等命令不实现 | d（范围收敛） | 命令注册面如实标注"未实现"，不静默消失；语义对齐只覆盖可执行命令 |
| D33 | 项目技能在 P0 无信任门控（pi 门控 `.pi/skills` 与项目 `.agents/skills`） | c（无信任管理器） | 与技术相关风险：技能是文本指令，与不受门控的上下文文件同级；本项目的审阅接缝是安全垫；信任门控在 P2 补齐后收紧 |
| D34 | 用量分桶暂不含工具结果的内嵌用量 | d（嵌套调用在 P2） | P0 只统计 assistant 消息、`usage` entry、压缩摘要（P1 后出现）；P2 接线时补 `Tools/summaries` 的嵌套合并 |
| D35 | 提示中的文档索引指向本仓库文档结构，而非 pi 安装路径 | a（自有文档） | 结构同 pi（主文档/附加文档/示例 + 主题索引），内容按本项目 |

## 7. 接缝核对（architecture/03）

| 接缝 | 本阶段落点 |
|---|---|
| #2 事件总线 | skills/命令/用量都不进 agent 循环；内核装配完毕后作为普通数据暴露 |
| #4 工具元模型 | 提示 loadout 只读 `ToolDefinition` 的 `exposure`/`prompt_snippet`/`prompt_guidelines`，不引入第二套工具模型 |
| #5 JSON Schema | 无变化 |
| #6 富流事件 | 用量来自既有 `KernelEvent` 与 `AssistantStreamEvent`，不新增裸字符串流 |
| #7 取消贯穿 | 技能/上下文加载是启动期同步 I/O，不参与取消；`/skill:` 展开在 run 前置阶段完成 |

## 8. 验收映射

| 场景 | 本阶段落点 | 批次 |
|---|---|---|
| D1 项目上下文 | `prompt/context.rs` + 提示装配；CLI 与桌面各验一次 | P0 |
| D2 skills | `prompt/skills.rs` + 命令注册面 | P0 |
| D3 compaction | 独立详设（P1） | P1 |
| D4 用量与缓存 | `usage.rs`（用量）；缓存统计 P1 | P0 / P1 |
| D5 无头模式 | 独立详设（P1） | P1 |
| D6 subagent 式扩展 | 依赖 D5 + 模板 + 扩展工具注册 | P2 |
| D7 CLI 回归 | Phase 1/2 测试一字不改全绿 | 每批 |

## 9. 体量预算

- 新增代码文件遵守 500 行上限；`prompt/` 按职责拆三个文件，单测与实现同文件或独立 `tests/`。
- docs 总数已超软限（65 > 60）：本阶段新增设计稿属于硬流程交付物，按 standards/08 登记豁免；**整合评审**仍排在实现开工前，先出候选清单再动删除（删除面需所有者确认）。
- 无新增依赖预期；若 `skills` 需要 glob/ignore 语义，优先用标准库手写的最小匹配（与 ext discovery 同风格），不引入 `ignore` crate 除非必要。

## 10. 未决问题

- **docs 整合评审**：需要先给候选清单（合并/归档/删除），删除面待确认后才动。
- **项目技能信任**：P0 按 D33 无门控；P2 的 trust-manager 定稿时回调本决策。
- **命令注册面的模板接缝**：P0 先留接口，模板与技能共用"名称 → 展开"两步；P2 的实现不得改这两步的顺序。
- **提示缓存冷启动**：提示结构升级会让每台机器首轮缓存失效一次，属预期影响，不设兼容层。
