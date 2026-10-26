# 工作上下文交接卡

- 更新：2026-10-26（会话主题：pi-flash 调研与入库；此前同日完成决策落定 / .agent 建立 / 根入口重组）

## 当前阶段与状态

- **Phase 0 已完成**（tag `phase-0.end`）；Phase 1 未开工。
- 产品名 **Timo-pi**，**MIT** 开源，**Windows 优先**（ADR-0010）。
- 看板唯一位置：`docs/roadmap/00-roadmap.md` §1。

## 上一轮完成了什么

- 新增 `docs/research/05-reference-pi-flash.md`：pi-flash（GPL-3.0，GPUI 桌面壳）分析。三条线结论——内核线无价值、**gpui 桌面线价值最高**（陷阱全集 + vendor/性能方法论）、插件线对口（QuickJS+签名闭环）。
- 合规范围扩大：AGENTS.md 铁律 6、standards/07 §5 现覆盖 pi-flash（GPL，只读思路禁抄码）。
- phase-3 增加开工前置检查：核对 **gpui-kit ↔ gpui-component 0.2.x 版本谱系**（pi-flash 用的是 gpui-component，不是 gpui-kit）。

## 下一步（按优先级）

1. Phase 1 开工前置：产出 `docs/design/phase-1/` 详设并定稿（phase-1 §6）。
2. `docs/architecture/05-session-format.md` 草稿 → 定稿（§6 待决问题逐项决议）。
3. Phase 1 首批 crate 引入时建立 `docs/standards/04-dependencies.md` 准入表。
4. 参考项目行动项见 research/05 §4（Phase 2/3/4 各有时点）。

## 未决问题

- gpui-kit 与 gpui-component 的谱系关系待 Phase 3 前核实（不阻塞 Phase 1/2）。

## 关键提醒

- GPL/LGPL 参考项目（PI-Desktop、pi-flash）只读思路禁抄码；pi 上游（MIT）可读不可抄（AGENTS 铁律 6）。
- 根入口定位（standards/08 §2）：README 双份=纯说明；AGENTS.md=薄索引；状态唯一来源是本卡。
- NSH/gpui-kit 锁版本，内核零 NSH 依赖；每阶段收尾做接缝核对（architecture/03）+ 体量核对（standards/08）。
- 本卡下次由 Phase 1 设计会话整体重写（模板 A）。
