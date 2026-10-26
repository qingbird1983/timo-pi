# H-007：Phase 3 第 4 批——内核桥接、会话与聊天流

- 日期：2026-10-27
- 状态：完成（四道门禁绿，真机验收通过）
- 上一批：`H-006-phase-3-window-kernel-review.md`

## 本批交付

1. **内核桥接** `src/bridge.rs`：专用线程 + current_thread tokio 运行时独占 `Kernel`；UI 只收发值类型（`BridgeCommand` / `BridgeEvent`）。run 前 `tools()` 装配 agent 取 `AbortHandle`，run 期间只接 `Cancel`，其他命令回 `Busy`；事件经 `EventBus` 订阅转发，`Lagged` 也转成 UI 提示。
2. **会话侧栏** `features/sidebar.rs`：列表（首条消息 + 消息数）、新建、切换、删除确认（AlertDialog + Danger），运行中禁用切换/新建。
3. **聊天流** `features/chat.rs` + `chat_rows.rs`：事件归并成行；用户右对齐气泡、assistant Markdown（`TextView::markdown`）、思考块、错误行；流式增量、usage、运行态。
4. **工具 trace**：`ToolCallRequested/Started/Update/Finished` 合成一行图标/名称/摘要/状态/时长；输出上限 200 行或 32 KiB，超出显示“已截断（共 N 行）”；`AgentFinished` 自动收起。
5. **输入/取消**：Enter 发送、Shift+Enter 换行；运行中按钮变“停止”，命令走 `Cancel`；状态栏与标题区同步运行态。
6. **会话恢复**：`Opened { id, history }` 用投影历史重建消息流；工具结果按 call id 配对，缺结果的标 `Interrupted`。
7. **测试**：`chat` 5 条状态机单测、trace 截断 2 条、`app` 假桥接 UI 测试 1 条；全套 206 tests 绿。

## 真机验收（DeepSeek 真实模型，临时 `TIMO_AGENT_DIR`）

- 内核就绪，状态栏显示 `deepseek/deepseek-chat`。
- 第一轮：真实工具调用 `read Cargo.toml`，trace 折叠显示“工具过程 · 1 步”，展开见状态“完成 · 0.00s”与输出；usage 1378 tokens。
- 第二轮追问“刚才读的是哪个文件”，回答 `Cargo.toml` → 多轮上下文（K0）有效。
- 运行中头部/状态栏显示“运行中”，按钮为“停止”；点击后回到“就绪”。
- 重启应用：侧栏恢复会话（标题 + 8 条消息），点击后历史完整回放。
- 删除按钮弹出确认框（标题含会话名、说明不可恢复），取消后会话保留。

## 已知限制 / 下一步

- trace 的 “查看全文” 目标（审阅 sheet）在第 5 批；当前只显示截断提示，全文仍保存在 `TraceEntry.output`。
- trace 行暂无 `Review` 状态，随第 5 批审阅接入。
- 助手 Markdown 长代码块体验、会话重命名留真实使用驱动。
- 第 5 批：diff 审阅面板（similar → `DiffFile::parse`）、命令面板、设置/钥匙串、审阅器注入。
