# 工作上下文交接卡

- 更新：2026-10-26（会话主题：ADR-0011 方向纪律确立；**当前等待所有者审阅文档后的开工通知**）

## 当前阶段与状态

- **Phase 0 已完成**（tag `phase-0.end`）；Phase 1 **未开工，等待所有者开工通知**。
- 产品名 **Timo-pi**，**MIT** 开源，**Windows 优先**（ADR-0010）。
- 看板唯一位置：`docs/roadmap/00-roadmap.md` §1。

## 上一轮完成了什么

- **ADR-0011（所有者硬约束）**：内核阶段（Phase 1~2）严格对齐 pi、忠实复刻；创新只记录不执行；偏离白名单三类；解禁需 Phase 4 验收 + 所有者同意。
- 联动：AGENTS.md 铁律 6"方向纪律"、standards/07 §6、phase-1/phase-2 方向纪律注记、roadmap/00 §3 Backlog 升格为创新提案唯一通道。
- 此前同日：构建产物治理（standards/08 §7）、pi-flash 调研（research/05）、gpui-kit 技能 vendored（.agents/skills）、根入口重组。

## 下一步（按优先级，收到开工通知后）

1. 产出 `docs/design/phase-1/` 详设并定稿（phase-1 §6），**全程按 ADR-0011 对齐 pi 行为语义**。
2. `docs/architecture/05-session-format.md` 草稿 → 定稿（§6 待决问题逐项决议）。
3. Phase 1 首批 crate 引入时建立 `docs/standards/04-dependencies.md` 准入表 + `.cargo/config.toml`（standards/08 §7）。
4. 以上齐备后进入 Phase 1 实现。

## 未决问题

- gpui-kit ↔ gpui-component 谱系待 Phase 3 前核实（不阻塞）。

## 关键提醒

- **方向纪律（ADR-0011）是最高优先约束之一**：遇到"想换更好的做法"→ 三问（pi 是否这么做 / 是否在白名单 / 是否有 ADR 授权），皆否则记 backlog、照 pi 做。
- GPL/LGPL 参考项目（PI-Desktop、pi-flash）只读思路禁抄码；pi 上游可读不可抄（AGENTS 铁律 7）。
- `.agents/skills/`（vendored 外部技能）与 `.agent/skills/`（项目沉淀）勿混。
- 写码纪律：写代码时遵循 `.agents/skills/ponytail`（MIT）——最小完整改动；它管"怎么写"，ADR-0011 管"做什么"，不冲突（standards/04 §8）。
- 根入口定位（standards/08 §2）：README 双份=纯说明；AGENTS.md=薄索引；状态唯一来源是本卡。
- 每阶段收尾：接缝核对（architecture/03）+ 体量核对（standards/08，含 target/）。
- 本卡下次由 Phase 1 设计会话整体重写（模板 A）。
