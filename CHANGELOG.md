# 更新日志（CHANGELOG）

本文件是**面向非技术读者的进展流水**，每次文档或阶段变更都必须在此登记一条（格式规范见 `docs/standards/03-documentation.md`）。技术读者另见 git 历史。

格式参照 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)；版本号在本项目同时用于标记「文档/阶段里程碑」，与未来代码发布版本共用一个序列。

---

## [0.1.1] - 2026-10-26

### 决策落定（ADR-0010）

- 产品正式命名 **Timo-pi**；项目**开源，许可证 MIT**（新增根目录 LICENSE）；**Windows 优先**（Phase 3 主攻，macOS/Linux 保证可构建）。

### 新增

- **`.agent/` 知识库与交接中枢**：当前上下文交接卡（CONTEXT.md）+ handoff 归档 + pitfalls 踩坑沉淀 + skills 技术沉淀；自带硬性体量预算（防无限膨胀）。
- **规范 08：体量与防膨胀管理**（`docs/standards/08`）——覆盖四类"只增不减"集合：docs 文档（流水账禁令、单篇行数上限、孤儿清零）、.agent 知识库、代码（单文件 ≤500 行、crate 拆分评审、死代码零容忍）、依赖。

### 联动更新

- README（标题/许可证/决策表）、AGENTS.md（.agent 纳入开工必读与硬规则 9）、standards/01（目录布局）、standards/03（进度报告模板新增体量核对）、standards/07（新增 §7 交接沉淀流程）、roadmap/phase-3（Windows 优先）、docs/README（登记 08）。

---

## [0.1.0] - 2026-10-26

### 新增（Phase 0：仓库初始化与文档定稿）

- 初始化 git 仓库（`main` 分支）。
- **顶层**：README（含进度看板与待决策事项）、AGENTS.md（AI 协作入口）、CHANGELOG、.gitignore。
- **调研沉淀 4 篇**（`docs/research/`）：pi 内核规模与架构分析、pi 扩展系统分析、gpui-kit + NSH 技术栈评估、参考项目 PI-Desktop 分析。
- **决策记录 9 篇**（`docs/adr/`）：Rust 全量自研、桌面栈选型、插件编排类兼容策略、GUI 插件 TSX 自建、rmcp、QuickJS 扩展宿主、自研会话格式、范围排除、内核即库。
- **架构设计 5 篇**（`docs/architecture/`）：总体架构、内核设计、关键接缝清单、插件系统（双轨制）、会话格式草案。
- **路线图 6 篇**（`docs/roadmap/`）：总路线图 + Phase 0~4 阶段文档（各含验收场景）。
- **仓库规范 7 篇**（`docs/standards/`）：仓库结构、Git 工作流、文档规范、代码规范、测试规范、安全规范、AI 协作规范。
- **术语表**（`docs/glossary.md`）。

### 待决策（不阻塞 Phase 1）

- 开源许可证、正式产品名、目标平台优先级（见 README「待所有者决策的事项」）。
