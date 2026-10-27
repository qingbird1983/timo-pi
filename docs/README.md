# 文档地图

本目录是项目的**唯一事实来源**。任何会话、任何人都应能只靠这些文档理解项目并继续工作。

每份文档头部有状态标记：**草稿 → 评审中 → 定稿**。阶段开工的前提是相关设计文档已达「定稿」（流程见 `standards/03-documentation.md`）。

## 阅读顺序建议

| 你是谁 | 读什么 |
|---|---|
| 项目所有者（不读代码） | [README.zh-CN.md](../README.zh-CN.md) → `roadmap/00-roadmap.md`（看板）→ CHANGELOG → `adr/`（感兴趣时） |
| 新加入的 AI 会话 | `.agents/CONTEXT.md` → [AGENTS.md](../AGENTS.md) → 本页 → 当前 phase 文档 → `standards/` 全部 → 相关架构文档 |
| 未来的人类协作者 | 同 AI 会话路径 |

## 目录结构

```
.agents/                AI 知识库与交接中枢（仓库根；含 vendored 技能；规范见其 README 与 standards/08）
docs/
├── README.md            本页：文档地图
├── glossary.md          术语表（面向非技术读者）
├── research/            调研沉淀（事实与数据，决策的依据）
├── adr/                 决策记录（为什么这么做，编号递增，只增不改）
├── architecture/        架构设计（系统是什么样、关键约束）
├── roadmap/             路线图（什么时候做什么、怎么验收）
├── standards/           仓库规范（怎么做事的规则，9 篇）
├── design/              （Phase 1 起使用）各阶段详细设计稿
└── （各阶段进度报告也存于 design/phase-N/report.md）
```

## 文档清单与状态

| 文档 | 状态 | 一句话说明 |
|---|---|---|
| research/01-pi-analysis.md | 定稿 | pi 是什么、多大、内部怎么分层 |
| research/02-pi-extensions.md | 定稿 | pi 插件机制全貌与兼容可行性依据 |
| research/03-stack-nsh-gpui-kit.md | 定稿 | 桌面技术栈（gpui-kit + NSH）评估结论 |
| research/04-reference-pi-desktop.md | 定稿 | 参考项目 PI-Desktop 的做法与教训 |
| research/05-reference-pi-flash.md | 定稿 | 参考项目 pi-flash（GPUI 桌面壳/插件系统）的做法与教训 |
| research/06-pi-interaction-logic.md | 定稿 | pi 交互逻辑/数据流向/提示词工程调查（Phase 3.5 R1 产出：提示词合同、数据流图、17 条交互对照） |
| research/07-ui-references.md | **草稿** | 桌面壳 UI 参考清单（三类产品图链+设计语言笔记；视觉精修批次前置素材，随时增补） |
| adr/ADR-0001 ~ 0013 | 定稿 | 13 项重大决策（见各文件；0013 = 内核深化阶段立项） |
| architecture/01-overview.md | 定稿 | 总体架构与分层原则 |
| architecture/02-kernel.md | 定稿 | 内核模块划分与关键设计 |
| architecture/03-critical-seams.md | 定稿 | MVP 必须预留的接缝清单（防返工核心） |
| architecture/04-plugin-system.md | 定稿 | 插件系统双轨制设计（编排兼容 + GUI 原生） |
| architecture/05-session-format.md | **定稿** | 会话磁盘格式（v1，2026-10-26 定稿） |
| design/phase-1/00-overview.md | 定稿 | Phase 1 总览：crate 布局、跨 crate 决策、pi 偏离登记 |
| design/phase-1/01-timo-ai.md | 定稿 | 协议层：消息/流事件/trait/两协议映射/目录/认证/重试 |
| design/phase-1/02-timo-agent.md | 定稿 | agent 循环、事件总线与取消 |
| design/phase-1/03-timo-tools.md | 定稿 | 工具元模型与内置工具契约 |
| design/phase-1/04-timo-session.md | 定稿 | 会话格式实现设计（崩溃安全/恢复投影） |
| design/phase-1/05-timo-config.md | 定稿 | 数据目录、settings/models.json、模型解析 |
| design/phase-1/06-timo-mcp-kernel-cli.md | 定稿 | MCP 并入、kernel 门面、CLI 冒烟壳 |
| design/phase-1/07-testing-ci.md | 定稿 | fixture 计划、接缝自动化、CI 与构建卫生脚本 |
| design/phase-1/report.md | 定稿 | Phase 1 阶段进度报告（A1~A6 全通过，CI run 40308144023 全绿） |
| design/phase-2/report.md | 定稿 | Phase 2 阶段进度报告（B1~B6 全通过；真实 DeepSeek 会话验收） |
| design/phase-2/00-overview.md | 定稿 | Phase 2 总览：模块布局、装载管线、关键决策、D11~D25 偏离、接缝核对 |
| design/phase-2/01-shim-api.md | 定稿 | shim API 对照表：模块/注册/动作/ctx/工具契约/JS↔Rust 类型映射 |
| design/phase-2/02-packaging.md | 定稿 | 安装期打包管线、错误分类总表 E1xx~E4xx、诊断报告格式 |
| design/phase-2/03-permissions.md | 定稿 | 权限能力清单 v1、双闸授予/失效、强制点与硬拒路径 |
| design/phase-2/04-hook-semantics.md | 定稿 | 钩子派发、30s 预算、快照隔离、abort 退休、错误隔离 |
| design/phase-2/05-shim-events.md | 定稿 | pi 1.4.0 全量事件矩阵（41 项逐项分类与发射点） |
| design/phase-3/00-overview.md | 定稿 | Phase 3 总览：版本边界、模块布局、内核前置 K0~K3、提交序列 |
| design/phase-3/01-ui-architecture.md | 定稿 | 窗口信息架构、事件→界面状态、工具卡片、diff 审阅、键盘与测试 |
| design/phase-3/02-credentials-and-settings.md | 定稿 | 钥匙串引用、桌面设置、脱敏边界与 C3 验收 |
| design/phase-3/03-nsh-embedding.md | 定稿 | AppView 隔离层、签名包信任链、热更/回退与 C4 证据 |
| design/phase-3/04-tool-review.md | 定稿 | 执行前审阅接缝、拒绝语义、并发与 CLI 兼容 |
| design/phase-3/prototype.html | 定稿（方向稿） | 桌面壳布局、工具过程 trace 与 diff 审阅原型 |
| design/phase-3/report.md | 定稿 | Phase 3 阶段进度报告（C1~C6 全通过；NSH 热更与拒绝路径真机验收） |
| design/phase-3.5/00-overview.md | 定稿 | Phase 3.5 总览：模块布局、批次序列、K1~K9 决策、D30~D38 偏离 |
| design/phase-3.5/01-system-prompt.md | 定稿 | 提示分节装配、项目上下文发现、SYSTEM/APPEND 文件、重载与测试 |
| design/phase-3.5/02-skills-and-commands.md | 定稿 | 技能发现/校验/注入/展开、命令注册面与输入分发 |
| design/phase-3.5/03-usage.md | 定稿 | 用量与成本聚合、分桶规则与内核 API |
| design/phase-3.5/report.md | 草稿（P0） | Phase 3.5 进度报告：P0 批 1~5、D1/D2/D4/D7 真机验收与体量核对 |
| adr/ADR-0012-tool-review-seam.md | 定稿 | 工具执行前的显式审阅接缝 |
| roadmap/00-roadmap.md | 定稿 | 总路线图与阶段依赖 |
| roadmap/phase-0 ~ phase-4 | phase-3.5 进行中（P0 完成）；其余定稿（细节可在开工前修订） | 阶段范围、交付物、验收场景 |
| standards/01 ~ 07 | 定稿 | 仓库/提交/文档/代码/测试/安全/协作规范 |
| standards/08-size-control.md | 定稿 | 体量与防膨胀管理（docs/.agents/代码/依赖的预算与清理机制） |
| standards/09-standards-review.md | 定稿 | 规范自身的复查与进化（漏洞/过严/过松的例行与触发复查、修正分级） |
| glossary.md | 定稿 | 术语表 |

> 规则：上表新增文档必须同步登记；改状态必须同步本表与 CHANGELOG。
