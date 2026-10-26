# AGENTS.md —— AI 协作入口

本项目由 AI 会话协作开发。项目所有者不读代码、只看文档进度，因此**一切知识与决策必须落盘到文档**，任何会话都应能仅凭本仓库文档冷启动。

## 开工前必读（按顺序）

1. `docs/README.md` —— 文档地图与状态约定
2. `docs/roadmap/00-roadmap.md` + **当前阶段**的 phase 文档
3. `docs/standards/` 全部 7 篇规范
4. 与当前工作相关的 `docs/architecture/`、`docs/adr/` 文档

## 硬规则

1. **文档先行**：当前阶段的设计文档状态未达「定稿」前，禁止写实现代码（规范、文档类提交除外）。
2. **决策留痕**：重大技术决策（选型变更、不可逆设计、放弃某能力）必须新增 ADR 到 `docs/adr/`，编号递增，不得只做不留记录。
3. **语言**：文档与对用户的沟通一律中文；代码、标识符、commit message 用英文（commit body 可中文）。
4. **阶段收尾**：每阶段结束必须产出《阶段进度报告》（模板见 `docs/standards/03-documentation.md` 附录），并同步更新 `README.md` 进度看板与 `CHANGELOG.md`。
5. **PI-Desktop 隔离**：严禁引用 `F:\Warehouse\Github\PI-Desktop` 的任何源码（LGPL-3.0）。只允许参考其公开文档描述的架构思路与设计模式。
6. **pi 上游**：仅用于理解行为、协议与兼容面，不复制源码文件。pi 的扩展 API 无 semver 承诺，兼容目标以 `docs/architecture/04-plugin-system.md` 的矩阵为准，不承诺逐版本追平。
7. **安全**：API key、密钥、个人文件路径不得写入仓库；新增依赖须按 `docs/standards/04-coding.md` 的准入流程登记。
8. **取消与格式不可逆项**：涉及会话格式、公共 API、事件契约的改动，必须先改文档与 ADR，再动代码，并保证向后兼容测试通过。

## 当前阶段指针

- **Phase 0 已完成**（2026-10-26）：仓库与全部文档落盘。
- **下一阶段 Phase 1（内核 MVP）**：开工前置条件 = `docs/roadmap/phase-1-kernel-mvp.md` 及其引用的设计文档从「草稿」转「定稿」。
- 阶段完成后，更新本指针。
