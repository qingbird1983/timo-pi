# 工作上下文交接卡

- 更新：2026-10-27（会话主题：**Phase 3.5 P0：提示、技能、命令与用量**）。同日另有审查会话：代码—文档一致性审查 → 审查收债立项（见「下一步」4 与 [H-010](handoff/H-010-audit-debt.md)）。

## 当前阶段与状态

- **Phase 1 ✅**（tag `phase-1.end`）；**Phase 2 ✅**（tag `phase-2.end`）；**Phase 3 ✅**（tag `phase-3.end`，报告 `docs/design/phase-3/report.md`）。
- Phase 3 六批全部完成：最小窗口与版本锁 → K0/K1 → K2 审阅接缝 → 会话/聊天流 → diff 审阅/命令面板/设置/钥匙串 → NSH PoC + C1~C6。
- **Phase 3.5 P1 完成（ADR-0013）**：P0（D1/D2/D4/D7）与 P1.1~P1.4（D3/D5/D4 缓存半场）验收通过，报告 `docs/design/phase-3.5/report.md`；D6（subagent）详设 [design/phase-3.5/08-subagent.md](../docs/design/phase-3.5/08-subagent.md) 定稿，实现四批中 ①②③ 已落，④（扩展本体 + 真机验收）待开工；P2 待定稿。
- 看板：`docs/roadmap/00-roadmap.md` §1；阶段文档：`docs/roadmap/phase-3-desktop-shell.md`（§6 已知限制）。
- 远端：`github.com/qingbird1983/timo-pi`（私有）。

## 上一轮完成了什么

- **D6 批③（工具用量回报）**：`ToolOutput.usage: Option<Usage>` → `ToolResultMessage.usage`（`run/tools.rs`）→ `Tools/summaries` 桶（`usage.rs` 聚合既有，未改语义）；ext 侧 `ToolCallResult.usage` 原样过 JSON 边界（shim 不裁字段），内建与 MCP 保持 `None`。两条新测试（消息 + 事件双通路 / 缺省不得造），变异验证 agent 层转发承重。**实测缺口已登记**：`extensions/events.rs` 的 `usage: result.usage` 移除后 timo-kernel+timo-ext 全绿（模块私有、无单工具执行 API ⇒ 无测试入口），必须由批④ 子代理真机 `/session` 的 Tools 读数补证，否则不算完成。
- **D6 批②（内核/CLI 三参数）**：`KernelOptions` 加 `no_session`/`tool_allowlist`/`append_system_prompt`，CLI 对应 `--no-session`、`--tools a,b,*`、`--append-system-prompt <text|@path>`（可重复；`@path` 读不到 → 退出码 2；`--tools` 全空 → 退出码 2；与 `resume` 互斥）。会话闸门收成 `Kernel::ensure_session()` 一处（run 与 `start_session` 共用，不留平行分支）；工具过滤在 MCP + 扩展注册完之后统一 `registry.hide_unlisted()`，未命中项进 `resource_warnings`（新 `WarningKind::UnknownTool`）。**修掉一个真缺口**：无头 `-p` 路径原先不打印资源告警，`unknown tool` 在子代理场景静默——现 `print::run` 收尾统一出口（含失败早退路径）。真机 DeepSeek：首行 `session_id:null` 且会话目录零文件、`--tools read` 下模型答 `NO_TOOL`、追加暗号被答对；三条闸门逐个变异验红。体量闸拦下 519/601 行两文件，按既有惯例拆出 `kernel/ext_api.rs`、`printer.rs`、`sessions_cmd.rs`（未加豁免）。
- **D6 批①（宿主文件读取 + 并发执行）**：`pi.readFile`/`pi.listDir` 落地 `fs.read` 强制点——保护路径先拒（`.git`/`.ssh`/`.env`/密钥文件 `E404`）、无授权 `E401`、范围外 `E403`、超 1 MiB 标 `[truncated]`；`pi.execParallel` ≤8 任务 / 4 并发、结果按提交序返回、超时或取消 kill 子进程。踩坑与修法：kill 外壳后孙进程仍握着输出管道，`JoinHandle::join()` 白等 30 秒——改有界收尾（`mpsc` + `recv_timeout` 0.8 秒）并 reap 直接子进程，超时用例 29.4s → 1.8s。新增 9 个测试（fs_scope 5 + permissions 3 + host 1）。
- **D6 标尺详设（08-subagent）定稿**：以 pi 官方 subagent 示例为行为蓝本（只读不抄），核对出四道缺口并定补齐方案——宿主 `pi.readFile`/`pi.listDir`（落地 `fs.read` 强制点，E401/E403/E404）、CLI `--no-session`/`--tools`/`--append-system-prompt`、`ToolOutput.usage` → `ToolResultMessage.usage` 进 `Tools/summaries` 桶、宿主 `pi.execParallel`（≤8 任务、4 并发）。偏离 D51~D56 逐项登记（实时进度与 workflow 预设如实延后）。
- **P1 验收与阶段报告**：D3（自动/手动压缩 + 重启恢复）、D5（text/JSONL/退出码）、D4 缓存半场（`/session` 的 cache/warming 行）写入 `docs/design/phase-3.5/report.md` 与看板；如实登记 Ctrl+C 130 未手工触发、DeepSeek 无 TTL 时预热停用。
- **Phase 3.5 P1.4（缓存统计与预热）**：统计从会话 entry 现算 miss（噪声地板 1024、压缩/预热重置基线），`/session` 加 `cache:` 行；预热按模型 `cache_ttl_seconds`（Anthropic 300 秒）+ TTL 90% + $0.05 经济阈值，`max_tokens=1` 重放并写 `usage(cache_warm)`；`cache_warming: off|streaming|idle`，streaming 续一次、idle 30 分钟内重复（D50）。真机 DeepSeek `/session` 显示 `warming: inactive`（无 TTL）且无 miss 行；预热任务用假适配器验过"恰好一次 + 落 entry"。
- **Phase 3.5 P1.3（无头模式）**：`timo -p "<任务>"`（可重复、同会话）与 `--mode json`；text 只出终答，JSONL 首行会话头、首/末事件 `agent_start`/`agent_end`、`message_update` 不带 partial；退出码 0/1/2、Ctrl+C 130。内核只补 `Kernel::start_session()`（D49）。真机：json 模式走完 `bash` 工具调用得到 `TIMO_JSON_SMOKE`，两条 `-p` 保住暗号。
- **Phase 3.5 P1.2（compaction）**：每轮请求前按「窗口 − reserve」自动压缩；切点只落 user 消息；`session.compaction` entry 只增不删，恢复投影为「摘要 + 保留尾部 + 之后消息」；手动 `/compact [补充要求]` 输出前后 token 与保留条数。真机手动压缩 `1836 → 279`，重启后仍答出「青柚」；修掉 `tokens_after` 沿用压缩前 usage 的读数错误；`/session` 改为按会话文件全部 message entry 累计，压缩后历史用量不再缩水。
- **Phase 3.5 P1.1（会话分区+导出）**：新会话落 `sessions/--<encoded-cwd>--/`，旧平铺只读兼容；resume/delete/list 跨分区；`export_session` 原子导出 leaf 链；CLI `/export` 可用。真机冒烟：分区目录与导出文件逐字核对。
- **Phase 3.5 P0 批 5（桌面接线）**：桥接启动后重载资源并下发命令清单与用量；面板「技能」组只收 `Ready` 命令，点选插入 `/skill:<name> ` 并聚焦；状态栏显示 tokens/费用；新建/打开/删除/每轮结束后刷新用量。
- **P0 验收（真实 DeepSeek，临时数据目录）**：D1 项目 `AGENTS.md` 的暗号规则在 CLI 与桌面都生效；D2 `/skill:hello` 两边触发并遵循技能内容；D4 `cache_read=1536`、累计 token/费用与分桶可查；D7 workspace 263 tests 全绿、既有测试零改动。
- **验收中修复**：桌面把 `/skill:` 原文发给模型 → 展开收进 `Kernel::run`（`85527d4`）。
- **门禁**：fmt / clippy / workspace tests（315）/ check-size 全绿；`target/` 超软限（incremental 5.6GiB / deps 17.3GiB），阶段收尾跑 `scripts/clean-build`。

## 下一步（按优先级）

1. D6 批④（详设 08 §5/§6）：`examples/extensions/subagent`（index.ts + manifest，代理定义发现走 `pi.readFile`/`pi.listDir`，single/parallel/chain + JSONL 聚合）、示例代理 `scout`/`reviewer`、真机 DeepSeek 验收（含中止 kill 子进程用例）；验收读数必须覆盖主会话 `/session` 的 `Tools` 桶，用来补证批③ 留下的适配器缺口。跑四门禁并单独提交；之后 P2 详设 → 实现 → `clean-build` + tag。
2. 桌面壳视觉精修（与 3.5 并行，零冲突）：research/07 → 设计稿 → 批次实现。
3. `target/` 当前 23.2GiB（软 15 / 硬 25，incremental 5.2GiB）；后续每阶段收尾跑 `scripts/clean-build`。sccache 到位后再收紧硬限。docs 实测 77 篇，整合评审待办（删除面需所有者确认）。
4. **审查收债**（前置：Phase 3.5 收口）：按 [docs/roadmap/audit-debt.md](../docs/roadmap/audit-debt.md) 清偿 2026-10-27 三路审查发现——**A1 会话半行拼接缺陷最先**（[P-012](pitfalls/P-012-session-half-line-append.md)，唯一会丢数据），B1 桌面壳穿透门面次之，其余排序见台账 §4。架构文档的文档侧偏差已于本轮回填。

> 施工顺序唯一来源：当前阶段设计文档；设计错误先改文档再写码。

## 未决问题

- NSH 渲染/热更仍为人工验收（设计 03 §8 允许），CI 只跑“未签名拒绝”。
- 生产签名密钥分发策略未定，留 Phase 4 ADR。
- `target/` 当前 23.2GiB（软 15 / 硬 25，incremental 5.2GiB），阶段收尾再清；无 sccache，未做全清；分类读数与清扫统一用 `scripts/target-size` / `clean-build`。
- **无头 stderr 噪音**（D6 批② 发现，已登记 report 已知问题）：`-p` 一次运行会打 8 条 `.agents/skills/` 的 frontmatter 告警，父代理贴 stderr 时噪音挤掉有效信息；处置方向随 P2 资源治理定。
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
- 踩坑索引：P-001 流事件静默回归；P-002 编辑重复行；P-003 Windows bash 是 WSL；P-004 上游摘要当合同；P-005 模块格式与运行时不符；P-006 依赖解析成功 ≠ API 兼容；P-007 gpui-kit 测试 glob 导入；P-008 `impl IntoElement` 生命周期捕获；P-009 sheet 焦点陷阱；P-010 gpui glob 遮蔽 `#[test]`；P-011 kill 后孙进程握着管道（读取要有界）。
