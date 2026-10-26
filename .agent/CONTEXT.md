# 工作上下文交接卡

- 更新：2026-10-26（会话主题：**Phase 1 开工**——详设定稿 + 基础设施，实现按 crate 序推进）

## 当前阶段与状态

- **Phase 1 实现中**（2026-10-26 所有者下达"开工吧"；tag `phase-1.start`）。Phase 0 已完成（tag `phase-0.end`）。
- 看板唯一位置：`docs/roadmap/00-roadmap.md` §1。

## 最近完成了什么

- **详设 8 篇定稿**（docs/design/phase-1/00~07）：行为语义全部以 pi 为蓝本（各篇内标注 pi 参照路径）；偏离逐项登记于 00 §5（D1~D10，全部落 ADR-0011 三类白名单）。
- **会话格式定稿**（architecture/05）：ULID+单调工厂防回拨；不轮转；payload 内联；平台数据目录。草案修订：四种消息 entry 合并为单一 `message` 型；取消 `system.notice`。
- **依赖准入表初版**（standards/04 附录 A，锁版本线）；顺带修正 §5 笔误（原引用不存在的 04-dependencies.md——standards/09"矛盾当轮修"首例）。
- pi 侧语义规格已提取（agent 循环/两协议适配/工具与配置/MCP），来自 D:\pi 源码研读，要点已固化进详设，无需重读 pi（除非详设未覆盖处）。

## 实现提交序列（本阶段计划）

1. ✅ 详设 + 定稿 + 开工登记（`phase-1.start`）
2. workspace 骨架：根 Cargo.toml（workspace members、[workspace.lints]）、.cargo/config.toml（standards/08 §7 原文落地）、.gitignore 补 target/、scripts/check-size + clean-stale、.github/workflows/ci.yml、fixtures/ 空架子随首批夹具建
3. timo-config → 4. timo-ai（类型/trait/SSE/两协议/目录/auth/retry/fixtures）→ 5. timo-session → 6. timo-tools → 7. timo-mcp → 8. timo-agent → 9. timo-kernel → 10. timo-cli
11. 中期：CHANGELOG/CONTEXT 同步；收尾：进度报告（接缝核对 architecture/03 + 体量核对 standards/08 + **规范复查 standards/09**）→ 看板 → tag `phase-1.end`

## 下一步（按优先级）

1. workspace 骨架提交（含 .cargo/config.toml + scripts + CI）。
2. timo-config（最简、无内部依赖，作为编译链验证）。
3. timo-ai types + trait + SSE 解码器 + anthropic 适配（最大单项，先 fixture 后实现）。

## 未决问题

- 无阻塞项。（gpui-kit ↔ gpui-component 谱系待 Phase 3 前核实，不阻塞。）

## 关键提醒

- **方向纪律（ADR-0011）**：遇到"想换更好的做法"→ 三问（pi 是否这么做 / 是否在白名单 / 是否有 ADR 授权），皆否则记 backlog、照 pi 做。详设 00 §5 的 D1~D10 是已授权偏离的**完整清单**，之外不得新增偏离。
- pi 源码在本机 D:\pi（MIT 许可，可复制数据/fixture/文案，须注明来源）；PI-Desktop/pi-flash 仅思路。
- 铁律提醒：禁 unsafe/unwrap（库层）、单文件 ≤500 行、公共 API 100% doc comment、Cargo.lock 入库、每 crate 一个实现提交含测试。
- 写码纪律：`.agents/skills/ponytail`（standards/04 §8）——最小完整改动；绝不削减校验/错误处理/安全/测试。
- 根入口定位（standards/08 §2）：README 双份=纯说明；AGENTS.md=薄索引；状态唯一来源是本卡。
- 每阶段收尾：接缝核对（architecture/03）+ 体量核对（standards/08，含 target/）+ **规范复查**（standards/09 四问）。
