# 工作上下文交接卡

- 更新：2026-10-26（会话主题：决策落定 + .agent 知识库建立 + 根入口文档重组）

## 当前阶段与状态

- **Phase 0 已完成**（tag `phase-0.end`）；Phase 1 未开工。
- 产品名 **Timo-pi**，**MIT** 开源（LICENSE 已建），**Windows 优先**（ADR-0010）。
- 看板唯一位置：`docs/roadmap/00-roadmap.md` §1（README 已不再放看板）。

## 上一轮完成了什么

- ADR-0010 决策落盘；`.agent/` 知识库建立（README + CONTEXT + handoff/pitfalls/skills）。
- 新增 `docs/standards/08` 体量与防膨胀管理（docs/知识库/代码/依赖四类预算）。
- 根入口重组：README 中英双份（纯项目说明）、AGENTS.md 改薄索引（禁流水账）、看板收拢至 roadmap/00。

## 下一步（按优先级）

1. Phase 1 开工前置：产出 `docs/design/phase-1/` 详设并定稿（phase-1 §6 清单）。
2. `docs/architecture/05-session-format.md` 草稿 → 定稿（§6 待决问题逐项决议）。
3. Phase 1 首批 crate 引入时建立 `docs/standards/04-dependencies.md` 准入表。
4. 以上齐备后方可写实现代码（AGENTS.md 铁律 1）。

## 未决问题

- 无阻塞项。

## 关键提醒

- 根入口定位（standards/08 §2）：README 双份=纯项目说明；AGENTS.md=薄索引禁流水；状态唯一来源是本卡。
- PI-Desktop 源码禁引用（LGPL）；NSH/gpui-kit 锁版本、内核零 NSH 依赖。
- 每阶段收尾：architecture/03 七接缝 + standards/08 体量核对（进度报告模板已含）。
- 本卡下次由 Phase 1 设计会话整体重写（模板 A）。
