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
| adr/ADR-0001 ~ 0009 | 定稿 | 9 项重大决策（见各文件） |
| architecture/01-overview.md | 定稿 | 总体架构与分层原则 |
| architecture/02-kernel.md | 定稿 | 内核模块划分与关键设计 |
| architecture/03-critical-seams.md | 定稿 | MVP 必须预留的接缝清单（防返工核心） |
| architecture/04-plugin-system.md | 定稿 | 插件系统双轨制设计（编排兼容 + GUI 原生） |
| architecture/05-session-format.md | **草稿** | 会话磁盘格式（Phase 1 内定稿） |
| roadmap/00-roadmap.md | 定稿 | 总路线图与阶段依赖 |
| roadmap/phase-0 ~ phase-4 | 定稿（各阶段细节可在开工前修订） | 阶段范围、交付物、验收场景 |
| standards/01 ~ 07 | 定稿 | 仓库/提交/文档/代码/测试/安全/协作规范 |
| standards/08-size-control.md | 定稿 | 体量与防膨胀管理（docs/.agent/代码/依赖的预算与清理机制） |
| standards/09-standards-review.md | 定稿 | 规范自身的复查与进化（漏洞/过严/过松的例行与触发复查、修正分级） |
| glossary.md | 定稿 | 术语表 |

> 规则：上表新增文档必须同步登记；改状态必须同步本表与 CHANGELOG。
