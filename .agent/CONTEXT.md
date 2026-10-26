# 工作上下文交接卡

- 更新：2026-10-26（会话主题：Phase 0 收尾 + 三项决策落定 + .agent 知识库建立）

## 当前阶段与状态

- **Phase 0 已完成**（tag `phase-0.end`，commit `e5e7f9e`）；Phase 1 未开工。
- 产品正式名 **Timo-pi**，开源、许可证 **MIT**，**Windows 优先**（ADR-0010）。

## 上一轮完成了什么

- 仓库初始化，36 份文档落盘（research 4 / ADR 9 / architecture 5 / roadmap 6 / standards 7 / 顶层 5）。
- 决策落定并写入 ADR-0010：命名、开源（MIT，LICENSE 已建）、平台优先级。
- 建立 `.agent/` 知识库机制与 `docs/standards/08` 防膨胀规范。

## 下一步（按优先级）

1. Phase 1 开工前置（phase-1 文档 §6）：产出 `docs/design/phase-1/` 详设并定稿。
2. `docs/architecture/05-session-format.md` 草稿 → 定稿（§6 待决问题逐项决议）。
3. 建立 `docs/standards/04-dependencies.md` 依赖准入登记表（Phase 1 引入首批 crate 时）。
4. 以上齐备后方可开始 Phase 1 实现（文档先行，AGENTS.md 硬规则 1）。

## 未决问题

- 无阻塞项。待办提醒：文档地图（docs/README.md）若新增文档必须同步登记。

## 关键提醒

- PI-Desktop 源码（LGPL）禁引用，只参考思路（AGENTS.md 硬规则 5）。
- NSH / gpui-kit 锁版本；NSH 依赖只允许出现在 Phase 3/4 层，内核零依赖。
- 每阶段收尾核对 architecture/03 七条接缝 + standards/08 体量核对（进度报告模板已含）。
- 本卡下次由 Phase 1 设计会话整体重写（模板 A）。
