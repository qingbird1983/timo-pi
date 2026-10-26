# H-001 Phase 1 实现收尾（内核 + CLI + 夹具）

- 日期：2026-10-26
- 主题：接续 Phase 1 未完成实现；本会话完成 kernel/CLI 并补齐验收所需夹具。

## 本轮做了什么

1. timo-kernel：MCP 惰性连接（首次 run/tools）、`tools()`/`wait_idle()`/`abort_handle()`、静态 sessions 入口；门面重导出。
2. timo-cli：run/resume/sessions、事件渲染、Ctrl+C 两段语义、退出码。
3. timo-agent：补 `ToolCallRequested` 与 `ToolCallStarted.args`、`AbortHandle`、`max_turns`；拆 `tool_calls.rs`。
4. timo-mcp：失败状态记录；`fixtures/mcp/echo_server.py` + A4 集成测试。
5. timo-ai：`SseTransport` 注入 + 8 个协议回放夹具；修复 OpenAI 无流式事件缺陷。
6. timo-session：4 个 v1 格式夹具 + 投影/截断测试。
7. 门禁：fmt/clippy/117 测试/check-size 全绿；设计 02/06/07 实现回填；进度报告与 CHANGELOG 0.1.10。

## 遗留与下一步

- A1~A3 人工验收（真实凭证 + 授权）；A4 真实第三方 server；A5 推送后 CI；阶段收尾 tag。
- MCP 发现失败不自动重试发现（调用层重连）；扩展钩子占位延后 Phase 2。
- 详见 `.agents/CONTEXT.md`。

## 已提取的沉淀

- P-001：适配器流式事件静默缺失（测试会挂起而非报错）。
- S-001：定位挂起的 cargo test（先单测、再串行、日志落仓内）。
