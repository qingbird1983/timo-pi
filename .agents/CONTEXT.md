# 工作上下文交接卡

- 更新：2026-10-27（会话主题：**Phase 3.5 P0：提示、技能、命令与用量**）

## 当前阶段与状态

- **Phase 1 ✅**（tag `phase-1.end`）；**Phase 2 ✅**（tag `phase-2.end`）；**Phase 3 ✅**（tag `phase-3.end`，报告 `docs/design/phase-3/report.md`）。
- Phase 3 六批全部完成：最小窗口与版本锁 → K0/K1 → K2 审阅接缝 → 会话/聊天流 → diff 审阅/命令面板/设置/钥匙串 → NSH PoC + C1~C6。
- **Phase 3.5 P1 进行中（ADR-0013）**：P0 完成（D1/D2/D4/D7，报告 `docs/design/phase-3.5/report.md`）；P1 详设 04~07 定稿；P1.1~P1.4（会话分区+导出、compaction、无头模式、缓存统计与预热）已提交。下一步阶段收尾：D3/D5/D6 验收 + P1 报告 + `clean-build`。
- 看板：`docs/roadmap/00-roadmap.md` §1；阶段文档：`docs/roadmap/phase-3-desktop-shell.md`（§6 已知限制）。
- 远端：`github.com/qingbird1983/timo-pi`（私有）。

## 上一轮完成了什么

- **Phase 3.5 P1.4（缓存统计与预热）**：统计从会话 entry 现算 miss（噪声地板 1024、压缩/预热重置基线），`/session` 加 `cache:` 行；预热按模型 `cache_ttl_seconds`（Anthropic 300 秒）+ TTL 90% + $0.05 经济阈值，`max_tokens=1` 重放并写 `usage(cache_warm)`；`cache_warming: off|streaming|idle`，streaming 续一次、idle 30 分钟内重复（D50）。真机 DeepSeek `/session` 显示 `warming: inactive`（无 TTL）且无 miss 行；预热任务用假适配器验过"恰好一次 + 落 entry"。
- **Phase 3.5 P1.3（无头模式）**：`timo -p "<任务>"`（可重复、同会话）与 `--mode json`；text 只出终答，JSONL 首行会话头、首/末事件 `agent_start`/`agent_end`、`message_update` 不带 partial；退出码 0/1/2、Ctrl+C 130。内核只补 `Kernel::start_session()`（D49）。真机：json 模式走完 `bash` 工具调用得到 `TIMO_JSON_SMOKE`，两条 `-p` 保住暗号。
- **Phase 3.5 P1.2（compaction）**：每轮请求前按「窗口 − reserve」自动压缩；切点只落 user 消息；`session.compaction` entry 只增不删，恢复投影为「摘要 + 保留尾部 + 之后消息」；手动 `/compact [补充要求]` 输出前后 token 与保留条数。真机手动压缩 `1836 → 279`，重启后仍答出「青柚」；修掉 `tokens_after` 沿用压缩前 usage 的读数错误；`/session` 改为按会话文件全部 message entry 累计，压缩后历史用量不再缩水。
- **Phase 3.5 P1.1（会话分区+导出）**：新会话落 `sessions/--<encoded-cwd>--/`，旧平铺只读兼容；resume/delete/list 跨分区；`export_session` 原子导出 leaf 链；CLI `/export` 可用。真机冒烟：分区目录与导出文件逐字核对。
- **Phase 3.5 P1 详设**：04 compaction（阈值/切点/摘要/entry/投影，D39~D40）、05 缓存统计与预热（D41~D44）、06 无头 `-p`/`--mode json`、07 会话按 cwd 分区与导出（D45~D47）；00 总览补偏离表。
- **Phase 3.5 P0 批 5（桌面接线）**：桥接启动后重载资源并下发命令清单与用量；面板「技能」组只收 `Ready` 命令，点选插入 `/skill:<name> ` 并聚焦；状态栏显示 tokens/费用；新建/打开/删除/每轮结束后刷新用量。
- **P0 验收（真实 DeepSeek，临时数据目录）**：D1 项目 `AGENTS.md` 的暗号规则在 CLI 与桌面都生效；D2 `/skill:hello` 两边触发并遵循技能内容；D4 `cache_read=1536`、累计 token/费用与分桶可查；D7 workspace 263 tests 全绿、既有测试零改动。
- **验收中修复**：桌面把 `/skill:` 原文发给模型 → 展开收进 `Kernel::run`（`85527d4`）。
- **测试**：窗口级面板测试移到 `features/palette_tests.rs`，`app_tests.rs` 回到 500 行内；新增事件到壳、插入动作与调色板关闭断言。
- **门禁**：fmt / clippy / workspace tests / check-size 全绿；`clean-build` 后 `target/` 25.4 → 14.0GiB（验收重建后 15.0）。

## 下一步（按优先级）

1. Phase 3.5 收尾：D3（压缩/恢复）、D5（会话分区/导出）、D6（无头/子进程）验收；补 `docs/design/phase-3.5/report.md` 的 P1 部分；跑 `scripts/clean-build` 后复跑四门禁。之后进入 P2（templates/provider-composer/codemode/trust）。
2. 桌面壳视觉精修（与 3.5 并行，零冲突）：research/07 → 设计稿 → 批次实现。
3. `target/` 当前 20.5GiB（软 15 / 硬 25）；后续每阶段收尾跑 `scripts/clean-build`。sccache 到位后再收紧硬限。docs 总数 69 超软限 60，整合评审待办（删除面需所有者确认）。

> 施工顺序唯一来源：当前阶段设计文档；设计错误先改文档再写码。

## 未决问题

- NSH 渲染/热更仍为人工验收（设计 03 §8 允许），CI 只跑“未签名拒绝”。
- 生产签名密钥分发策略未定，留 Phase 4 ADR。
- `target/` 当前 20.5GiB（软 15 / 硬 25），阶段收尾再清；无 sccache，未做全清；分类读数与清扫统一用 `scripts/target-size` / `clean-build`。
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
