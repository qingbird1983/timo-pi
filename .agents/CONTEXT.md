# 工作上下文交接卡

- 更新：2026-10-27（会话主题：**Phase 3 实现第 4 批：内核桥接、会话与聊天流**）

## 当前阶段与状态

- **Phase 1 ✅**（tag `phase-1.end`）；**Phase 2 ✅**（tag `phase-2.end`）。
- **Phase 3 进行中**：设计定稿（tag `phase-3.start`）；实现第 1~4 批完成——最小窗口、K0/K1、K2 审阅接缝、内核桥接 + 会话/聊天流/工具 trace/输入取消。
- 设计：`docs/design/phase-3/00-overview.md` ~ `04-tool-review.md` + `prototype.html`；决策 ADR-0012。
- 看板：`docs/roadmap/00-roadmap.md` §1；阶段文档：`docs/roadmap/phase-3-desktop-shell.md`。
- 远端：`github.com/qingbird1983/timo-pi`（私有）。

## 上一轮完成了什么

- **桥接**：`bridge.rs` 专用线程 + current_thread 运行时独占 `Kernel`；UI 只收发 `BridgeCommand`/`BridgeEvent`。run 前 `tools()` 装配取 `AbortHandle`；run 期间只接 `Cancel`，其他命令回 `Busy`；事件订阅转发，`Lagged` 转 UI 提示。
- **会话**：侧栏列表（标题 + 消息数）、新建、切换、删除确认（AlertDialog + Danger），运行中禁切换；重启后列表与会话历史恢复。
- **聊天流**：事件归并成行；用户右气泡、assistant Markdown、思考块、错误；流式增量、usage、运行态；Enter 发送 / Shift+Enter 换行；运行中“停止”。
- **工具 trace**：调用生命周期合成一行（图标/名称/摘要/状态/时长）；输出上限 200 行或 32 KiB，超出提示已截断；`AgentFinished` 自动收起；历史回放时缺结果的调用标 `Interrupted`。
- **测试**：chat 状态机 5 条、trace 截断 2 条、假桥 UI 1 条；全套 **206 tests** 绿。
- **真机验收（DeepSeek）**：真实工具调用 + trace 折叠/展开 + usage；追问文件名验证多轮上下文；运行中停止；重启恢复会话与历史；删除确认框取消保留会话。
- **拆分**：chat 渲染移到 `chat_rows.rs`、测试移到 `chat_tests.rs`，单文件回到 ≤500 行。
- **坑**：P-008（`impl IntoElement` 捕获 `&mut Context` 生命周期、动态 ElementId、组件图标子集与缺失 trait 导入）。

## 下一步（按优先级）

1. 第 5 批：diff 审阅面板（`similar` → `DiffFile::parse`）、命令面板、设置页与系统钥匙串、把 `ToolReview` 注入 `KernelOptions`。
2. 第 6 批：NSH PoC（`nsh-poc` feature）+ C1~C6 验收 → `report.md` → tag `phase-3.end`。

> 施工顺序唯一来源：`docs/design/phase-3/00-overview.md` §8。设计错误先改文档再写码。

## 未决问题

- trace 的“查看全文”入口依赖第 5 批审阅 sheet；当前只显示截断提示，全文保存在 `TraceEntry.output`。
- trace 行暂无 `Review` 状态，随第 5 批接入。
- NSH 0.3.0 + gpui-fast 0.1.10 的**运行期**行为（渲染/热更）尚未验证，只有编译探针。
- C4 官方 CLI 生成 v1/v2 包的具体命令参数（`nsh keygen/certify/publish/verify`，实现期验证）。
- 本阶段审阅“本会话始终允许”只在内存，不跨会话持久化。
- 助手 Markdown 长代码块、会话重命名留真实使用驱动。
- `target/` 已 28GB（软上限 15GB），`cargo-sweep` 未安装；下次空闲按龄清扫。

## 关键提醒

- **内核零 GUI**：`crates/` 不得出现 gpui-kit/NSH；桌面只 import `timo-kernel`。
- **审阅前无副作用**：拒绝必须不写文件、不 spawn；等待中取消/关窗 → 拒绝。
- **CLI 等价**：`tool_review = None` 时 Phase 1/2 测试一字不改全绿。
- **版本只认 registry 源码签名**；0.8.1/0.1.12 下 NSH 报 E0716，锁定 0.8.0 家族 + 0.1.10。
- **UI 测试禁用 `use gpui_kit::*`**（P-007）；循环里构造行的方法返回 `AnyElement`（P-008）。
- **真机冒烟配置**：临时目录放 `settings.json`（`default_provider/default_model`）+ `models.json`（顶层必须是 `{"providers": {...}}`），`api_key` 用 `$DEEPSEEK_API_KEY` 引用，启动时 `TIMO_AGENT_DIR` 指过去；不写个人路径入库。
- **门禁四连**：fmt / clippy / `cargo test --workspace` / `check-size`；全量测试慢，用 `CARGO_BUILD_JOBS=1`。
- **写中文文件后扫隐形字符**：`grep -nP '[\x{3040}-\x{30ff}\x{200b}-\x{200f}\x{feff}]'`。
- **收尾四件事**：重写本卡、归档 handoff、沉淀 pitfalls/skills、体量核对。
- 踩坑索引：P-001 流事件静默回归；P-002 编辑重复行；P-003 Windows bash 是 WSL；P-004 上游摘要当合同；P-005 模块格式与运行时不符；P-006 依赖解析成功 ≠ API 兼容；P-007 gpui-kit 测试 glob 导入；P-008 `impl IntoElement` 生命周期捕获。
