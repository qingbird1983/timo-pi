# 更新日志（CHANGELOG）

本文件是**面向非技术读者的进展流水**，每次文档或阶段变更都必须在此登记一条（格式规范见 `docs/standards/03-documentation.md`）。技术读者另见 git 历史。

格式参照 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)；版本号在本项目同时用于标记「文档/阶段里程碑」，与未来代码发布版本共用一个序列。

---

## [0.1.6] - 2026-10-26

### 新增（所有者硬约束）

- **ADR-0011：内核阶段严格对齐 pi，忠实复刻；创新延后至项目稳定**——Phase 1~2 行为语义以 pi 为唯一蓝本（语义对齐而非逐行照抄），允许偏离仅限三类白名单（语言必然差异 / 既有 ADR 载体替换 / standards 硬要求）；创新冲动一律记录不执行，解禁需 Phase 4 验收通过 + 所有者同意。
- 联动：AGENTS.md 铁律新增"方向纪律"（第 6 条）、standards/07 §6、roadmap/phase-1 与 phase-2 各加方向纪律注记、roadmap/00 §3 Backlog 升格为创新提案唯一通道。

### 当前状态

- **等待所有者审阅文档后下达开工通知**；Phase 1 开工前置条件不变（phase-1 §6）。

---

## [0.1.5] - 2026-10-26

### 新增（standards/08 §7：构建产物治理）

- 针对"Rust 仓库膨胀到几十上百 G"的老大难，定下治理方案：**单 workspace 单 target/ 红线**、瘦身 profile（`debug = "line-tables-only"` 等，预期砍 60~80% 体积）、按龄清扫不全清（cargo sweep + sccache 兜底）、target/ 15G 软上限纳入阶段核对、git 禁止构建产物与 >1MB 二进制。
- Phase 1 交付物补充：`.cargo/config.toml` 瘦身 profile + `scripts/clean-stale` + `check-size` 盘点 target/。

---

## [0.1.4] - 2026-10-26

### 新增

- **`.agents/skills/`**：vendored 官方工具技能——gpui-kit 框架技能（组件/编码规范/GPUI 机制 22 篇）+ gpui-kit 设计规范总纲，共 29 个文件，自用户级安装复制、逐一比对一致。Phase 3（桌面壳）核心参考。
- 其 `README.md` 登记来源、同步策略（视同依赖升级，Phase 3 开工前重同步）与预算豁免声明；与项目自创的 `.agent/skills/`（知识沉淀）明确区分。

### 联动更新

- standards/01 目录布局新增 `.agents/skills/`；AGENTS.md 文档索引新增桌面开发入口。

---

## [0.1.3] - 2026-10-26

### 新增

- **调研 05：pi-flash 分析**（`docs/research/05`）——Windows 专用 pi 桌面 GUI 壳（Rust + GPUI + gpui-component，GPL-3.0）。结论：内核线无参考价值（它复用 pi 无内核）；**gpui 桌面线价值最高**（8.7 万行生产级 GPUI 应用 + 陷阱全集 + vendor/性能/架构守卫方法论）；插件系统线次之但对口（QuickJS + qjsc 字节码 + ed25519 签名闭环）。

### 联动更新

- AGENTS.md 铁律 6 与 standards/07 §5：外部代码合规范围扩大——pi-flash（GPL-3.0）与 PI-Desktop（LGPL）同级，只读思路、严禁抄码。
- roadmap/phase-3 风险注记：开工前需核对 **gpui-kit 与 gpui-component（0.2.x）的版本谱系与 API 延续性**。

---

## [0.1.2] - 2026-10-26

### 变更（仓库根入口重组）

- **README 改为纯项目说明，出中英双份**：`README.md`（英文）+ `README.zh-CN.md`（中文），内容一致、互链；进度看板从 README 移除。
- **进度看板收拢**：`docs/roadmap/00-roadmap.md` 的阶段总览表成为**唯一看板**（消除原先 README 与 roadmap 两份重复）。
- **AGENTS.md 重构为薄索引入口**：只保留导航表 + 铁律链接；明确禁止记录状态/进度/阶段指针等流水账；原"当前阶段指针"职责移交 `.agent/CONTEXT.md`。

### 规范修订

- `standards/01`：根文件清单与 README/AGENTS 定位约束。
- `standards/08`：§2 新增"入口文件定位约束"（README 双语一致性、AGENTS 薄索引、状态唯一来源）。
- `docs/README.md`：所有者与 AI 的阅读路径更新。

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
