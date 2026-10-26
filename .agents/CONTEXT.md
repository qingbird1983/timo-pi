# 工作上下文交接卡

- 更新：2026-10-27（会话主题：**Phase 3 实现第 5 批：审阅 sheet、命令面板、设置与钥匙串**）

## 当前阶段与状态

- **Phase 1 ✅**（tag `phase-1.end`）；**Phase 2 ✅**（tag `phase-2.end`）。
- **Phase 3 进行中**：设计定稿（tag `phase-3.start`）；实现第 1~5 批完成——最小窗口、K0/K1、K2 审阅接缝、内核桥接 + 会话/聊天流、diff 审阅 + 命令面板 + 设置/钥匙串。
- 设计：`docs/design/phase-3/00-overview.md` ~ `04-tool-review.md` + `prototype.html`；决策 ADR-0012。
- **Phase 3.5 已立项（草案，ADR-0013）**：内核深化——补齐 pi 灵魂件（AGENTS.md 上下文/提示词工程/skills/compaction/缓存/无头模式），排期在 Phase 3 整体完工后。
- 看板：`docs/roadmap/00-roadmap.md` §1；阶段文档：`docs/roadmap/phase-3-desktop-shell.md`（§6 已登记第 5 批遗留限制）。
- 远端：`github.com/qingbird1983/timo-pi`（私有）。

## 上一轮完成了什么

- **审阅链**：`similar` → `DiffFile::parse` → `DiffState` → `Diff`；2 MiB / 5,000 行上限；串行队列 + “本会话允许”缓存；接受/拒绝回传 `ReviewDecision`，拒绝原因显示在 trace（Blocked）；取消 run 时 pending 审阅按拒绝处理。
- **注入**：`ReviewBroker` 实现 `ToolReview`，按 `review.files/commands` 放行或发 `BridgeEvent::Review`；`KernelOptions.tool_review` 注入；CLI 保持 `None`。
- **命令面板**：Ctrl+K；四组命令；过滤/Enter/Esc；打开时清空查询并聚焦；快捷键提示。
- **设置页**：常规（语言/主题/字号）、模型（只读）、凭证（状态 + 设置/移除确认）、工具（审阅开关 + 清空允许）、关于；`desktop.json` 原子持久化，启动恢复、退出保存窗口尺寸。
- **凭证**：`ValueRef::Keychain`、`keychain:<service>/<account>`；service 固定 `timo-pi`、account=provider id；`set_provider_api_key` 原子改写 `models.json`；Debug 脱敏；真实钥匙串往返测试。
- **快捷键**：Ctrl+N / Ctrl+K / Ctrl+, / Ctrl+. / Ctrl+Enter（接受审阅）/ Ctrl+Backspace（拒绝审阅）/ Esc（关面板、设置；审阅由 sheet 处理）。
- **修复**：sheet 焦点陷阱导致 App 根级快捷键失效（P-009）、面板查询残留、关闭覆盖层后焦点丢失、设置页垂直居中、钥匙串移除后状态不刷新、设置页内成功反馈不可见。
- **测试**：桌面 22 条（含审阅接受/拒绝/快捷键、面板开关与清空、设置快捷键）；workspace 228 tests 全绿。

## 真机验收（DeepSeek，临时 `TIMO_AGENT_DIR`）

- 接受写 `notes.txt` → 文件落盘、内容 `timo review ok`；拒绝写 `notes2.txt` → 文件不存在；拒绝 `echo > cmd-out.txt` → 未执行、文件不存在。
- 钥匙串保存 → `models.json` 仅 `keychain:timo-pi/deepseek`、数据目录无 `sk-` 明文、凭据管理器出现 `deepseek.timo-pi`；移除 → 条目消失、状态“未配置”、页内显示成功反馈。
- 命令面板：过滤“设置”→ 执行；重开查询为空；Ctrl+N 新建；Ctrl+, 设置页顶部对齐；主题写入并在重启后恢复。
- 侧栏次要信息：相对时间（刚刚/N 分钟前/N 小时前/N 天前）。

## 下一步（按优先级）

1. 第 6 批：NSH PoC（`nsh-poc` feature 门控）+ C1~C6 验收 → `report.md` → 看板/CHANGELOG → tag `phase-3.end`。
2. 遗留（本阶段不阻塞验收）：模型热切换、设置·扩展页、会话搜索框、超大 diff 外部编辑器、trace 全文入口（见 phase doc §6）。

> 施工顺序唯一来源：`docs/design/phase-3/00-overview.md` §8。设计错误先改文档再写码。

## 未决问题

- NSH 0.3.0 + gpui-fast 0.1.10 的**运行期**行为（渲染/热更）尚未验证，只有编译探针。
- C4 官方 CLI 生成 v1/v2 包的具体命令参数（`nsh keygen/certify/publish/verify`，实现期验证）。
- C3 的“日志无明文”目前只扫描了数据目录；应用日志输出尚未落盘成文件，C1~C6 时补一次运行期扫描。
- `target/` 约 33GB（软上限 15GB），`cargo-sweep` 未安装；下次空闲按龄清扫。

## 关键提醒

- **内核零 GUI**：`crates/` 不得出现 gpui-kit/NSH；桌面只 import `timo-kernel`。
- **审阅前无副作用**：拒绝必须不写文件、不 spawn；等待中取消/关窗 → 拒绝。
- **CLI 等价**：`tool_review = None` 时 Phase 1/2 测试一字不改全绿。
- **密钥只写钥匙串**：`models.json` 只存引用；不打印、不入库；临时测试密钥文件用完即删。
- **版本只认 registry 源码签名**；0.8.1/0.1.12 下 NSH 报 E0716，锁定 0.8.0 家族 + 0.1.10。
- **覆盖层快捷键**：Sheet/Dialog 的焦点陷阱会让 App 根级 action 失效；焦点与 action 必须在覆盖层子树内（P-009）。
- **关闭覆盖层还焦点**：面板/设置关闭后 `focus_composer`，否则全局快捷键失灵。
- **UI 测试禁用 `use gpui_kit::*`**（P-007）；循环里构造行的方法返回 `AnyElement`（P-008）。
- **真机冒烟配置**：临时目录放 `settings.json` + `models.json`（顶层 `{"providers": {...}}`），`api_key` 用 `$DEEPSEEK_API_KEY`，启动时 `TIMO_AGENT_DIR` 指过去；不写个人路径入库。
- **门禁四连**：fmt / clippy / `cargo test --workspace` / `check-size`；全量测试慢，用 `CARGO_BUILD_JOBS=1`。
- **写中文文件后扫隐形字符**：`grep -nP '[\x{3040}-\x{30ff}\x{200b}-\x{200f}\x{feff}]'`。
- **收尾四件事**：重写本卡、归档 handoff、沉淀 pitfalls/skills、体量核对。
- 踩坑索引：P-001 流事件静默回归；P-002 编辑重复行；P-003 Windows bash 是 WSL；P-004 上游摘要当合同；P-005 模块格式与运行时不符；P-006 依赖解析成功 ≠ API 兼容；P-007 gpui-kit 测试 glob 导入；P-008 `impl IntoElement` 生命周期捕获；P-009 sheet 焦点陷阱阻断 App 快捷键。
