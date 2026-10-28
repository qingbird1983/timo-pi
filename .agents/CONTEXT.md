# 工作上下文交接卡

- 更新：2026-10-28（会话主题：**Phase 3.5 收尾完成（clean-build + tag `phase-3.5.end`）**；同日此前为项目信任门控撤回与 P2 第三题落地）。

## 当前阶段与状态

- **Phase 1 ✅**（tag `phase-1.end`）；**Phase 2 ✅**（tag `phase-2.end`）；**Phase 3 ✅**（tag `phase-3.end`，报告 `docs/design/phase-3/report.md`）。
- **Phase 3.5 ✅**（tag `phase-3.5.end`，2026-10-28）：P0（D1/D2/D4/D7）、P1（P1.1~P1.4，D3/D5/D4 缓存半场）、D6（subagent 四批）与 P2 三题（09 模板、10 provider composer/虚拟模型、11 codemode 批 A~C）全部落地并真机验收；P2 第四题（项目信任门控）撤回（[ADR-0015](../docs/adr/ADR-0015-no-project-trust.md)）。报告 `docs/design/phase-3.5/report.md`。
- 下一关：**审查收债**（前置已满足）→ **Phase 4（GUI 插件系统）**；桌面壳视觉精修按 roadmap 约定在 3.5 与 4 均收口后启动。
- 看板：`docs/roadmap/00-roadmap.md` §1；阶段文档：`docs/roadmap/phase-3-desktop-shell.md`（§6 已知限制）。
- 远端：`github.com/qingbird1983/timo-pi`（私有）。

## 上一轮完成了什么

- **Phase 3.5 收尾（2026-10-28）**：门禁四连全绿（fmt / clippy `-D warnings` / workspace **413 tests** / check-size）；`clean-build` 从 24.3GiB 清到 **14.0GiB**（incremental 0.0 / deps 12.8），释放 10.3GiB；报告补阶段收尾，看板与 docs 索引改为完成态；打 tag `phase-3.5.end` 并确认远端 CI 为绿。
- **项目信任门控整体撤回**：按所有者决定，产品不做 VS Code 式工作区信任。删除内核 `trust` 模块与决策链、CLI `/trust` 与 `--approve/--no-approve`、扩展 `project_trust` 事件与 `ctx.isProjectTrusted()` 真值（恢复恒 `true`）、子代理透传、桌面五项弹框与 `trust.json`；项目设置/MCP/扩展/技能/模板/SYSTEM/APPEND 恢复无条件加载，启动不再询问。删除详设 12、D80~D87、P-014；新增 [ADR-0015](../docs/adr/ADR-0015-no-project-trust.md)。
- **codemode 慢 runner 竞态修复**：CI 两次分别命中两个超时/取消用例。根因：① SHIM 安装期撞上 deadline，中断错误被当成沙箱失败；② 主循环先按 deadline 收尾并取消子调用，脚本线程随后报的 `cancelled` 覆盖了 timeout 原因。修法：安装期失败按 deadline/取消重述，主循环主动中断时强制用中断原因覆盖收尾错误；本机同组用例重复 20 次全绿，详见 [P-015](pitfalls/P-015-interrupt-reason-must-survive-cleanup.md)。

## 下一步（按优先级）

1. **审查收债**：按 [docs/roadmap/audit-debt.md](../docs/roadmap/audit-debt.md) 清偿 2026-10-27 三路审查发现——**A1 会话半行拼接缺陷最先**（[P-012](pitfalls/P-012-session-half-line-append.md)，唯一会丢数据），B1 桌面壳穿透门面次之，其余排序见台账 §4。
2. **Phase 4（GUI 插件系统）**：按 [docs/roadmap/phase-4-gui-plugins.md](../docs/roadmap/phase-4-gui-plugins.md) 启动；B 轨 slot 扩展。
3. **桌面壳视觉精修**（3.5 与 4 均收口后）：research/07 → 设计稿 → 批次实现。
4. 体量读数只在「门禁」行登记；docs 实测 82 篇，整合评审待办（删除面需所有者确认）。

> 施工顺序唯一来源：当前阶段设计文档；设计错误先改文档再写码。

## 未决问题

- NSH 渲染/热更仍为人工验收（设计 03 §8 允许），CI 只跑“未签名拒绝”。
- 生产签名密钥分发策略未定，留 Phase 4 ADR。
- `target/` 无 sccache 时不做全清；分类读数与清扫统一用 `scripts/target-size` / `clean-build`（当前读数见「门禁」行）。
- **无头 stderr 噪音**（D6 批② 发现，已登记 report 已知问题）：`-p` 一次运行会打 8 条 `.agents/skills/` 的 frontmatter 告警，父代理贴 stderr 时噪音挤掉有效信息；处置方向留待资源治理批次定。
- 桌面壳遗留：模型热切换、设置·扩展页、会话搜索、超大 diff 外部编辑器、trace 全文入口、审阅开关不重放（roadmap phase-3 §6）。

## 关键提醒

- **内核零 GUI**：`crates/` 不得出现 gpui-kit/NSH；桌面只 import `timo-kernel`。
- **审阅前无副作用**：拒绝必须不写文件、不 spawn；等待中取消/关窗 → 拒绝。
- **CLI 等价**：`tool_review = None` 时 Phase 1/2 测试一字不改全绿。
- **密钥只写钥匙串**：`models.json` 只存引用；不打印、不入库；临时测试密钥文件用完即删。
- **版本只认 registry 源码签名**；0.8.1/0.1.12 下 NSH 报 E0716，锁 0.8.0 家族 + 0.1.10；升级前重跑编译探针。
- **NSH 只在 `nsh-poc` feature**：默认构建与 `cargo test --workspace` 不依赖 NSH。
- **NSH 信任流程**：先 `verify_package`、健康后再 `accept_package`；不要用 `load_package`（会提前登记构建，失败无法回退）。
- **两种信任别混**：扩展能力授权（`permissions.json`，决定扩展能做什么）与 NSH 包信任（签名链）互相独立，不可互相替代；产品不做 VS Code 式项目/工作区信任（ADR-0015）。
- **覆盖层快捷键**：Sheet/Dialog 的焦点陷阱会让 App 根级 action 失效；焦点与 action 必须在覆盖层子树内（P-009）。
- **UI 测试禁用 `use gpui_kit::*`**（P-007）；子模块里的普通 `#[test]` 也不要让 glob 带进 `test` 宏（P-010）。
- **真机冒烟配置**：临时目录放 `desktop.json` + `models.json`（顶层 `{"providers": {...}}`，`base_url` 下划线命名），`api_key` 用 `$DEEPSEEK_API_KEY`，启动时 `TIMO_AGENT_DIR` 指过去；不写个人路径入库。子进程 argv 取证用包装 exe（记录参数后再 exec 真程序，`--ext-flag bin=<wrap>`），不靠日志推断。
- **门禁四连**：fmt / clippy / `CARGO_INCREMENTAL=0 cargo test --workspace` / `check-size`；NSH 改动另跑 `--features nsh-poc` 的 clippy 与测试；全量测试慢，用 `CARGO_BUILD_JOBS=1`。
- **门禁**：workspace **413 tests** 全绿；`target/` 收尾 `clean-build` 后 14.0GiB（incremental 0.0 / deps 12.8）。累计变异 **26/26 全红**（P2 第三题；信任批的 22 条随功能撤回，移除批为纯删除、不做变异）。
- **构建缓存按容量治理**：`target/` 15GiB 软 / 25GiB 硬；incremental >4GiB 先删；阶段收尾跑 `scripts/clean-build`，`check-size` 超硬限直接 FAIL；无 sccache 不全清。
- **写中文文件后扫隐形字符**：`grep -nP '[\x{3040}-\x{30ff}\x{200b}-\x{200f}\x{feff}]'`。
- **收尾四件事**：重写本卡、归档 handoff、沉淀 pitfalls/skills、体量核对。
- 踩坑索引：P-001 流事件静默回归；P-002 编辑重复行；P-003 Windows bash 是 WSL；P-004 上游摘要当合同；P-005 模块格式与运行时不符；P-006 依赖解析成功 ≠ API 兼容；P-007 gpui-kit 测试 glob 导入；P-008 `impl IntoElement` 生命周期捕获；P-009 sheet 焦点陷阱；P-010 gpui glob 遮蔽 `#[test]`；P-011 kill 后孙进程握着管道（读取要有界）；P-012 会话半行拼接；P-013 Git Bash 改写 `/命令` 参数（验收加 `MSYS_NO_PATHCONV=1`）；P-015 主动中断的原因必须覆盖收尾错误。
