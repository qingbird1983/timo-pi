# 工作上下文交接卡

- 更新：2026-10-28（会话主题：**Phase 4 设计**——市场 + 已安装管理形态落盘，市场卡片定为错落式瀑布流）。

## 当前阶段与状态

- **Phase 1 ✅**（tag `phase-1.end`）；**Phase 2 ✅**（tag `phase-2.end`）；**Phase 3 ✅**（tag `phase-3.end`，报告 `docs/design/phase-3/report.md`）。
- **Phase 3.5 ✅**（tag `phase-3.5.end`，2026-10-28）：报告 `docs/design/phase-3.5/report.md`。
- **审查收债 ✅**（2026-10-28）：A1 数据缺陷、B1~B9 代码缺口、C1~C10 文档回填、D1~D9 复杂度清理全部完成；台账 `docs/roadmap/audit-debt.md`。
- 下一关：**Phase 4（GUI 插件系统）**——设计 00~08 已于 2026-10-28 定稿，**P0 实现中**：第一批（schema 2 清单 + 内核统一插件清单）已完成，下一步市场索引与桌面两页。
- 三个设计默认值：市场索引地址默认留空（首次配置，不伪造官方地址）；评论只外链；实验性组 v1 为空（只登记真实功能）。
- 看板：`docs/roadmap/00-roadmap.md` §1；桌面壳已知限制见 `docs/roadmap/phase-3-desktop-shell.md` §6。
- 远端：`github.com/qingbird1983/timo-pi`（私有）。

## 上一轮完成了什么

- **审查收债全清**：A1 续写前截断会话半行；B1 桌面壳改走门面（`timo_kernel::config`/`data_dir`）；B4 取消贯穿三层（MCP 等待、扩展工具 per-call 标志、Unix 进程组）；B5 钩子错误隔离（单扩展失败记 E302 后继续）；B6/B7/B8 未接线面补齐（嵌套进度、删 `session_dir`、进程级 `IdGen`）；B9/B2/B3 语义按实现定稿进架构文档。
- **复杂度清理**：删除未接线执行模式/队列模式、零调用 API、未消费配置等约 330 行与 1 个无关依赖；`timo sessions` 时间格式化复用 `time`（下沉 `timo-session::format_utc_minutes`），`fs_scope` 通配换 `globset`（先证 `**` 吞 0 段/`*`/`?` 等价，`[]`/`{}` 转义为字面量防授权面扩大）。
- **验证**：workspace **419 tests** 全绿；fmt/clippy `-D warnings`/check-size 通过；`clean-build` 从 22.1GiB 清到 14.0GiB（释放 8.1GiB）。行为测试做了变异验证：B5 隔离、`globset` 两层语义、A1 修复均确认「恢复前红/恢复后绿」；内核取消分支的破坏性变异被工具分类器拦截，改由宿主层变异 + 内核测试双覆盖。
- 每批独立提交：A1、B1、D3~D7、D1/D2+B6/B7/B8、D8/D9+C10、B2~B5+C9（最新提交见 git log）。
- **Phase 4 设计已定稿**：统一管理器、manifest/签名包、slot/动作、宿主 API v1、运行时隔离、devkit、自证插件、插件市场；市场卡片为**错落式瀑布流**（08 §3.1，按可用宽度 3/2/1、最多三列）。
- **市场形态与分发边界**见 [ADR-0016](../docs/adr/ADR-0016-plugin-marketplace.md)：静态索引只提供元数据，安装仍以签名包为准；屏蔽 ≠ 卸载。

## 下一步（按优先级）

1. **Phase 4 设计评审**：看 `docs/design/phase-4/00-overview.md`、`08-marketplace.md` 与 `prototype.html`（市场瀑布流 + 已安装分组）；三个默认值待确认（市场索引 URL、评论只外链、实验性插件按自家 backlog），定稿后写码。
2. **桌面壳视觉精修**（3.5 与 4 均收口后）：research/07 → 设计稿 → 批次实现。
3. **推送与远端核验**：本地收债提交尚未 push（远端停在 `8bd0a18` / tag `phase-3.5.end`），推送后必须核远端 CI 为绿。
4. docs 实测篇数与整合评审待办（删除面需所有者确认）。

> 施工顺序唯一来源：当前阶段设计文档；设计错误先改文档再写码。

## 未决问题

- NSH 渲染/热更仍为人工验收（设计 03 §8 允许），CI 只跑“未签名拒绝”。
- 生产签名密钥分发策略未定，留 Phase 4 ADR。
- `target/` 无 sccache 时不做全清；分类读数与清扫统一用 `scripts/target-size` / `clean-build`（当前读数见「门禁」行）。
- **无头 stderr 噪音**（D6 批② 发现，已登记 report 已知问题）：`-p` 一次运行会打 8 条 `.agents/skills/` 的 frontmatter 告警；处置方向留待资源治理批次定。
- 桌面壳遗留：模型热切换、设置·扩展页、会话搜索、超大 diff 外部编辑器、trace 全文入口、审阅开关不重放（roadmap phase-3 §6）。
- Unix 子进程组清理已实现但无 Windows CI 覆盖，随平台打包实测。

## 关键提醒

- **内核零 GUI**：`crates/` 不得出现 gpui-kit/NSH；桌面只 import `timo-kernel`。
- **现阶段 UI 是粗轮廓**（所有者 2026-10-28 定调）：只核对功能入口与按钮位置；配色、间距、图标后期精修，不因视觉细节反复返工。
- **市场卡片错落式瀑布流**：卡片高度随内容，列数按卡片区可用宽度 3/2/1（≥660 / 420~659 / <420），**最多三列**，窗口缩放即时重排；不用等高网格；已安装页保持分组列表，两种排版不混用。
- **审阅前无副作用**：拒绝必须不写文件、不 spawn；等待中取消/关窗 → 拒绝。
- **CLI 等价**：`tool_review = None` 时 Phase 1/2 测试一字不改全绿。
- **取消三层**：MCP 用 `select!` 等 `ctx.cancel`；扩展工具把 per-call `AtomicBool` 交给 QuickJS 中断处理器（宿主无预算，取消是唯一收敛手段）；Unix 子进程 `process_group(0)` + `kill -KILL -- -pid`。
- **钩子失败不再整体报错**：`dispatch_all` 记录诊断后继续；只有 payload 编码失败才返回 Err。
- **密钥只写钥匙串**：`models.json` 只存引用；不打印、不入库；临时测试密钥文件用完即删。
- **版本只认 registry 源码签名**；0.8.1/0.1.12 下 NSH 报 E0716，锁 0.8.0 家族 + 0.1.10；升级前重跑编译探针。
- **NSH 只在 `nsh-poc` feature**：默认构建与 `cargo test --workspace` 不依赖 NSH。
- **NSH 信任流程**：先 `verify_package`、健康后再 `accept_package`；不要用 `load_package`。
- **两种信任别混**：扩展能力授权与 NSH 包信任互相独立；产品不做 VS Code 式项目/工作区信任（ADR-0015）。
- **覆盖层快捷键**：Sheet/Dialog 的焦点陷阱会让 App 根级 action 失效（P-009）。
- **UI 测试禁用 `use gpui_kit::*`**（P-007）；子模块普通 `#[test]` 不要被 glob 带进 `test` 宏（P-010）。
- **真机冒烟配置**：临时目录放 `desktop.json` + `models.json`（顶层 `{"providers": {...}}`，`base_url` 下划线命名），`api_key` 用 `$DEEPSEEK_API_KEY`，启动时 `TIMO_AGENT_DIR` 指过去；不写个人路径入库。
- **门禁四连**：fmt / clippy / `CARGO_INCREMENTAL=0 cargo test --workspace` / `check-size`；NSH 改动另跑 `--features nsh-poc`；全量测试慢，用 `CARGO_BUILD_JOBS=1`。
- **门禁**：workspace **419 tests** 全绿；`clean-build` 后 14.0GiB（incremental 1.1 / deps 11.7）。
- **构建缓存按容量治理**：`target/` 15GiB 软 / 25GiB 硬；incremental >4GiB 先删；阶段收尾跑 `scripts/clean-build`；无 sccache 不全清。
- **写中文文件后扫隐形字符**：`grep -nP '[\x{3040}-\x{30ff}\x{200b}-\x{200f}\x{feff}]'`。
- **收尾四件事**：重写本卡、归档 handoff、沉淀 pitfalls/skills、体量核对。
- 踩坑索引：P-001 流事件静默回归；P-002 编辑重复行；P-003 Windows bash 是 WSL；P-004 上游摘要当合同；P-005 模块格式与运行时不符；P-006 依赖解析成功 ≠ API 兼容；P-007 gpui-kit 测试 glob 导入；P-008 `impl IntoElement` 生命周期捕获；P-009 sheet 焦点陷阱；P-010 gpui glob 遮蔽 `#[test]`；P-011 kill 后孙进程握着管道；P-012 会话半行拼接；P-013 Git Bash 改写 `/命令` 参数；P-015 主动中断的原因必须覆盖收尾错误；P-016 错误隔离会改变派发返回值。
