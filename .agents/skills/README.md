# .agents/skills —— 官方工具技能（vendored）

- **用途**：存放外部工具的**官方技能文档**，供本项目 AI 会话直接阅读，不依赖开发者机器上的用户级安装。当前收录 gpui-kit 官方两件套——**Phase 3（桌面壳）的核心参考资料**。
- **来源**：2026-10-26 自用户级 `~/.agents/skills/` 复制（gpui-kit 官方 skill，文档源 gpui-kit.com，对应 gpui-kit crate 生态，Apache-2.0）。
- **收录清单**：
  - `gpui-kit/`：框架技能——组件目录、编码规范（coding-guides）、家族约定（conventions）、应用配方（recipes）、GPUI 底层机制 22 篇（element/entity/event/async/testing…）。
  - `gpui-kit-design-guides/`：设计规范总纲（视觉/布局/交互的规范性指南）。
- **同步策略**：视同依赖升级（standards/08 §5）——Phase 3 开工前与上游重新同步一次并核对与所锁 gpui-kit crate 版本的匹配性；升级独立成提交并登记 CHANGELOG。
- **预算豁免**：本目录属 vendored 外部资产，**不计入** docs/.agent 体量预算（standards/08）；但**禁止**在目录内新增项目自创内容——项目自己的经验沉淀放 `.agent/skills/`（注意两者是不同目录、不同语义）。
- **关联提醒**：pi-flash 的 gpui 陷阱笔记针对 gpui-component 0.2.x；Phase 3 开工前需按 research/05 §2.2 核对 gpui-kit 与 gpui-component 的谱系，再以本目录技能为准写桌面代码。
