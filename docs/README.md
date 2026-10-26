# 文档地图

本目录是项目的**唯一事实来源**。任何会话、任何人都应能只靠这些文档理解项目并继续工作。

每份文档头部有状态标记：**草稿 → 评审中 → 定稿**。阶段开工的前提是相关设计文档已达「定稿」（流程见 `standards/03-documentation.md`）。

## 阅读顺序建议

| 你是谁 | 读什么 |
|---|---|
| 项目所有者（不读代码） | [README.zh-CN.md](../README.zh-CN.md) → `roadmap/00-roadmap.md`（看板）→ CHANGELOG → `adr/`（感兴趣时） |
| 新加入的 AI 会话 | `.agent/CONTEXT.md` → [AGENTS.md](../AGENTS.md) → 本页 → 当前 phase 文档 → `standards/` 全部 → 相关架构文档 |
| 未来的人类协作者 | 同 AI 会话路径 |

## 目录结构

```
.agent/                AI 知识库与交接中枢（仓库根；规范见其 README 与 standards/08）
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
| adr/ADR-0001 ~ 0011 | 定稿 | 11 项重大决策（见各文件） |
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
| roadmap/00-roadmap.md | 定稿 | 总路线图与阶段依赖 |
| roadmap/phase-0 ~ phase-4 | 定稿（各阶段细节可在开工前修订） | 阶段范围、交付物、验收场景 |
| standards/01 ~ 07 | 定稿 | 仓库/提交/文档/代码/测试/安全/协作规范 |
| standards/08-size-control.md | 定稿 | 体量与防膨胀管理（docs/.agent/代码/依赖的预算与清理机制） |
| standards/09-standards-review.md | 定稿 | 规范自身的复查与进化（漏洞/过严/过松的例行与触发复查、修正分级） |
| glossary.md | 定稿 | 术语表 |

> 规则：上表新增文档必须同步登记；改状态必须同步本表与 CHANGELOG。
