# .agents/skills —— 外部技能（vendored）

- **用途**：存放外部**上游技能文档**，供本项目 AI 会话直接阅读，不依赖开发者机器上的用户级安装。收录两类：桌面开发参考（gpui-kit 两件套，**Phase 3 的核心参考资料**）与写码纪律（ponytail，**Phase 1 起写代码时的默认纪律**）。
- **收录清单**：
  - `gpui-kit/`：框架技能——组件目录、编码规范（coding-guides）、家族约定（conventions）、应用配方（recipes）、GPUI 底层机制 22 篇（element/entity/event/async/testing…）。
  - `gpui-kit-design-guides/`：设计规范总纲（视觉/布局/交互的规范性指南）。
  - `ponytail/`：**"最懒资深工程师"最小改动纪律**（SKILL.md 52 行 + 上游 LICENSE）——用最少的新代码完整解决问题：最小完整改动、优先复用仓库既有结构与依赖、不造没人要的抽象；**绝不削减**边界校验、防数据丢失的错误处理、安全与必要测试。纪律管"怎么写"，方向纪律（ADR-0011）管"做什么"，互不冲突且互相加强（其"沿用代码库既有结构"规则正是忠实复刻的要求）。
- **来源与许可**：
  - gpui-kit 两件套：2026-10-26 自用户级 `~/.agents/skills/` 复制（gpui-kit 官方 skill，文档源 gpui-kit.com，对应 gpui-kit crate 生态，Apache-2.0）。
  - ponytail：2026-10-26 自 GitHub `DietrichGebert/ponytail`（commit `09c434a`）复制 `skills/ponytail/SKILL.md` 与 `LICENSE`，逐一比对字节一致；**MIT**。
- **同步策略**：两者均视同依赖升级（standards/08 §5）——gpui-kit 于 Phase 3 开工前与上游重新同步并核对与所锁 crate 版本的匹配性；ponytail 在各阶段开工前可顺手核对一次上游；升级独立成提交并登记 CHANGELOG。
- **预算豁免与命名**：vendored 子目录（`gpui-kit/`、`gpui-kit-design-guides/`、`ponytail/`）属外部资产，**不计入** `.agents/` 体量预算（standards/08）；项目自创经验放同目录的 `S-NNN-*.md`，与上游子目录按命名区分，**禁止**改动 vendored 子目录内容。
- **关联提醒**：pi-flash 的 gpui 陷阱笔记针对 gpui-component 0.2.x；Phase 3 开工前需按 research/05 §2.2 核对 gpui-kit 与 gpui-component 的谱系，再以本目录技能为准写桌面代码。写码冲突时以 docs/standards/04 为准（见其 §8）。
