# 工作上下文交接卡

- 更新：2026-10-26（会话主题：**Phase 1 收尾**——内核门面与 CLI 落地、夹具回放、进入人工验收）

## 当前阶段与状态

- **Phase 1 实现完成、人工验收中**（2026-10-26）。Phase 0 已完成（tag `phase-0.end`）。
- 实现提交序列：`4889ee6` tools 清理 → `ea22972` mcp 失败状态 → `7cc66be` agent 事件/中止/轮数 → `cce7d22` kernel 门面 → `d4bfe6e` CLI → `02769dd` 依赖清理 → `2f8570d` ai 传输注入+回放 → `014bb67` session 夹具 → `5c74381` mcp echo 集成测试。
- 看板唯一位置：`docs/roadmap/00-roadmap.md` §1；当前进度报告：`docs/design/phase-1/report.md`（进行中）。

## 上一轮完成了什么

- **timo-kernel 收尾**：MCP 首次 `run`/`tools` 才连接（失败只发 Error 事件不阻断）；补 `tools()`、`wait_idle()`、`abort_handle()`、无凭证可用的 `sessions_dir`/`list_sessions_at`；门面重导出事件类型（薄壳只依赖本 crate）。
- **timo-cli 落地**：`run` / `resume <id|last>` / `sessions`；流式正文、`[tool]` 起止行、usage 摘要、错误到 stderr；stdin 逐行与 `/abort`；Ctrl+C 第一次中止、第二次退出。
- **timo-ai 可测化**：`SseTransport` 注入 + `api_with_transport`；`fixtures/protocol/` 8 个回放用例；**修复 OpenAI 适配器从不产出流式事件**的缺陷（其 on_sse 以前恒返回 None）。
- **夹具补齐**：`fixtures/session/`（线性/工具/未知型/半行）、`fixtures/mcp/echo_server.py` + 集成测试（A4 自动化）。
- **门禁**：fmt / clippy `-D warnings` / 全量 117 测试 / check-size 全绿；体量回预算（run.rs 拆分、死代码清零）。
- **文档**：设计 02/06/07 加"实现回填"；进度报告、CHANGELOG 0.1.10、docs/README 登记。

## 下一步（按优先级）

1. **A1~A3 人工验收**：需要真实模型凭证与所有者授权（付费调用）。A1 改文件任务 → A2 会话列表/恢复续聊 → A3 中途 Ctrl+C 与 kill -9 后文件完好。
2. **A4 上半**：人工配置一个真实第三方 stdio MCP server；HTTP 传输尚未人工验证。
3. **A5**：推送后确认 GitHub Actions（windows-latest；A4 测试依赖 Python）。
4. **阶段收尾**：报告转正 → roadmap 看板状态 → tag `phase-1.end`。
5. Phase 2 扩展宿主设计可与上述并行（内核接口已稳定）。

## 未决问题

- A1~A3 阻塞阶段收尾（需凭证/授权）；不阻塞 Phase 2 设计。
- MCP 工具"发现失败"不会自动重试发现（已发现工具的调用失败会在调用层重连）——Phase 2 惰性管理补。
- `LoopConfig` 七个扩展钩子结构占位未落地（D10 不实现执行；占位随 Phase 2 设计，见设计 02 回填与 roadmap backlog）。
- 取消回放测试依赖"先收到 Delta 再 cancel"；若该路径回归为无事件，测试会挂起而非报红（见下）。

## 关键提醒

- **方向纪律（ADR-0011）**：行为语义以 pi 为唯一蓝本；偏离白名单只有 D1~D10，新增须 ADR/backlog。
- **事件契约**：OpenAI/Anthropic 都必须产出 `BlockStart → Delta → BlockEnd`；新增协议适配器照抄两适配器的事件时序（回放测试会验证）。
- **跨 crate 夹具路径**：测试用 `env!("CARGO_MANIFEST_DIR")/../../fixtures/...`；夹具文件头必须写来源声明。
- **门禁四连**：`cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && bash scripts/check-size`；全量测试若 LNK1102 用 `CARGO_BUILD_JOBS=1`。
- **收尾四件事**：重写本卡、归档 handoff、沉淀 pitfalls/skills、体量核对（standards 08；报告模板见 standards 03 §5）。
