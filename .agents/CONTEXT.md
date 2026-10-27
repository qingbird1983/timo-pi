# 工作上下文交接卡

- 更新：2026-10-27（会话主题：**Phase 3 收尾：NSH PoC 与 C1~C6 验收**）

## 当前阶段与状态

- **Phase 1 ✅**（tag `phase-1.end`）；**Phase 2 ✅**（tag `phase-2.end`）；**Phase 3 ✅**（tag `phase-3.end`，报告 `docs/design/phase-3/report.md`）。
- Phase 3 六批全部完成：最小窗口与版本锁 → K0/K1 → K2 审阅接缝 → 会话/聊天流 → diff 审阅/命令面板/设置/钥匙串 → NSH PoC + C1~C6。
- **Phase 3.5 设计进行中（ADR-0013）**：R1 调查已定稿（`docs/research/06-pi-interaction-logic.md`）；下一步写 `design/phase-3.5/00-overview.md` 与 P0 详设，定稿前禁止写实现代码。
- 看板：`docs/roadmap/00-roadmap.md` §1；阶段文档：`docs/roadmap/phase-3-desktop-shell.md`（§6 已知限制）。
- 远端：`github.com/qingbird1983/timo-pi`（私有）。

## 上一轮完成了什么

- **NSH 隔离层**（`features/nsh_poc/`，`nsh-poc` feature）：`verify_package` → 暂存第二个 `Entity<AppView>` → 1 秒健康检查 → `accept_package` 后交换；失败保留旧面板。`TrustStore` 持久化已接受构建，降级包被拒。
- **预览页**：侧栏入口；v1/v2/崩溃/未签名/篡改、重置信任、停止；显示 id/version/build/sha 与状态；明暗跟随；主窗口 `inactive_frame_interval = None`。
- **打包**：`scripts/build-nsh-poc` 只调官方 `nsh 0.3.0 (76fa8a7)` 的 `keygen/certify/publish/verify`；源码 `fixtures/nsh-poc/panel.tsx`；密钥与包只落数据目录。
- **真机验收（DeepSeek，临时 `TIMO_AGENT_DIR`）**：C1 会话/工具/接受写 `proof.txt`/拒绝写 `rejected.txt`/强杀重开恢复；C2 diff 审阅；C3 沿用 H-008 钥匙串证据 + 本轮 `sk-` 扫描 0 命中；C4 热更与四类拒绝；C5 强杀恢复；C6 七条接缝 + `#[gpui_kit::test]`。
- **修复**：`use gpui_kit::*` 把 gpui 的 `test` 宏带进子模块导致 `#[test]` 自递归（P-010）；`nsh_poc.rs` 667 行超 500，拆 `mod.rs` 409 + `page.rs` 268。
- **测试**：默认 workspace 228 tests；`--features nsh-poc` 23 tests（含未签名拒绝）；fmt / clippy / check-size 通过。

## 下一步（按优先级）

1. Phase 3.5 P0 收口：桌面命令面板接新清单（批 5）→ D1/D2/D4/D7 真机验收 → `report` 草稿；之后 P1 详设（compaction/缓存/无头）。
2. 桌面壳视觉精修（与 3.5 并行，零冲突）：research/07 → 设计稿 → 批次实现。
3. `target/` 已按新版 §7 清到 14GiB；后续每阶段收尾跑 `scripts/clean-build`（40.6 → 14.0，释放 26.6GiB）。sccache 到位后再把硬限收紧到 15GiB。docs 总数 65 超软限 60，Phase 3.5 开工前做整合评审。

> 施工顺序唯一来源：当前阶段设计文档；设计错误先改文档再写码。

## 未决问题

- NSH 渲染/热更仍为人工验收（设计 03 §8 允许），CI 只跑“未签名拒绝”。
- 生产签名密钥分发策略未定，留 Phase 4 ADR。
- `target/` 已清到 14GiB（软 15 / 硬 25）；无 sccache，未做全清；分类读数与清扫统一用 `scripts/target-size` / `clean-build`。
- 桌面壳遗留：模型热切换、设置·扩展页、会话搜索、超大 diff 外部编辑器、trace 全文入口、审阅开关不重放（roadmap phase-3 §6）。

## 关键提醒

- **内核零 GUI**：`crates/` 不得出现 gpui-kit/NSH；桌面只 import `timo-kernel`。
- **审阅前无副作用**：拒绝必须不写文件、不 spawn；等待中取消/关窗 → 拒绝。
- **CLI 等价**：`tool_review = None` 时 Phase 1/2 测试一字不改全绿。
- **密钥只写钥匙串**：`models.json` 只存引用；不打印、不入库；临时测试密钥文件用完即删。
- **版本只认 registry 源码签名**；0.8.1/0.1.12 下 NSH 报 E0716，锁 0.8.0 家族 + 0.1.10；升级前重跑编译探针。
- **NSH 只在 `nsh-poc` feature**：默认构建与 `cargo test --workspace` 不依赖 NSH。
- **信任流程**：先 `verify_package`、健康后再 `accept_package`；不要用 `load_package`（会提前登记构建，失败无法回退）。
- **覆盖层快捷键**：Sheet/Dialog 的焦点陷阱会让 App 根级 action 失效；焦点与 action 必须在覆盖层子树内（P-009）。
- **UI 测试禁用 `use gpui_kit::*`**（P-007）；子模块里的普通 `#[test]` 也不要让 glob 带进 `test` 宏（P-010）。
- **真机冒烟配置**：临时目录放 `desktop.json` + `models.json`（顶层 `{"providers": {...}}`，`base_url` 下划线命名），`api_key` 用 `$DEEPSEEK_API_KEY`，启动时 `TIMO_AGENT_DIR` 指过去；不写个人路径入库。
- **门禁四连**：fmt / clippy / `CARGO_INCREMENTAL=0 cargo test --workspace` / `check-size`；NSH 改动另跑 `--features nsh-poc` 的 clippy 与测试；全量测试慢，用 `CARGO_BUILD_JOBS=1`。
- **构建缓存按容量治理**：`target/` 15GiB 软 / 25GiB 硬；incremental >4GiB 先删；阶段收尾跑 `scripts/clean-build`，`check-size` 超硬限直接 FAIL；无 sccache 不全清。
- **写中文文件后扫隐形字符**：`grep -nP '[\x{3040}-\x{30ff}\x{200b}-\x{200f}\x{feff}]'`。
- **收尾四件事**：重写本卡、归档 handoff、沉淀 pitfalls/skills、体量核对。
- 踩坑索引：P-001 流事件静默回归；P-002 编辑重复行；P-003 Windows bash 是 WSL；P-004 上游摘要当合同；P-005 模块格式与运行时不符；P-006 依赖解析成功 ≠ API 兼容；P-007 gpui-kit 测试 glob 导入；P-008 `impl IntoElement` 生命周期捕获；P-009 sheet 焦点陷阱；P-010 gpui glob 遮蔽 `#[test]`。
