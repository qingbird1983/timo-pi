# 工作上下文交接卡

- 更新：2026-10-26（会话主题：**Phase 1 真实验收**——DeepSeek 跑通 A1~A4，修复两个真实缺陷）

## 当前阶段与状态

- **Phase 1 验收完成**（2026-10-26）：A1~A4 真实通过，A6 通过；仅剩 A5 远端 CI 确认与 tag `phase-1.end`。
- 实现提交：`4889ee6`..`2f725e3`（tools 清理 → mcp 失败状态 → agent 事件/中止/轮数 → kernel 门面 → CLI → ai 传输注入+回放 → session 夹具 → mcp echo → 文档 → 真机缺陷修复）。
- 看板：`docs/roadmap/00-roadmap.md` §1；报告：`docs/design/phase-1/report.md`。

## 上一轮完成了什么

- **真实验收（DeepSeek `deepseek-flash`）**：A1 改文件并验证、A2 会话列表与恢复续聊、A3 运行中 `taskkill` 强杀后文件完好、A4 模型调用 MCP `mcp__echo__echo`——全部通过。
- **修复 `edit` 行重复**（P-002）：精确替换把整文件行 splice 回原数组，导致后续行重复+空行；改为字节区间替换并加精确内容回归。
- **修复 Windows bash 落 WSL**（P-003）：仅认原生 MSYS/Cygwin bash，否则回退 PowerShell。
- 门禁：122 测试 + fmt/clippy/check-size 全绿。
- 文档：设计 03 回填、报告更新、CHANGELOG 0.1.11、handoff H-002。

## 下一步（按优先级）

1. **A5**：推送后确认 GitHub Actions（windows-latest，A4 依赖 Python）。
2. **阶段收尾**：报告转正（去掉"CI 待确认"）→ 看板状态 → tag `phase-1.end`。
3. Phase 2 扩展宿主设计（内核接口已稳定，可并行）。
4. 可选人工复核：Ctrl+C 两段语义（控制台信号未自动化）、真实 HTTP MCP server。

## 未决问题

- Ctrl+C 第一次 abort/第二次退出仅代码级验证；kill -9 路径已实际验证。
- MCP 工具"发现失败"不自动重试发现（调用层会重连）——Phase 2 补。
- `LoopConfig` 七个扩展钩子结构占位未落地（D10；backlog）。
- 真实第三方 HTTP MCP server 未验证。

## 关键提醒

- **方向纪律（ADR-0011）**：D1~D10 是全部已授权偏离；新增偏离走 ADR/backlog。
- **真实接口配置**：OpenAI 兼容 `https://api.deepseek.com`，模型 `deepseek-flash` / `deepseek-v4-pro`，凭证 `$DEEPSEEK_API_KEY`（ValueRef 用 `$NAME`，不是 `${NAME}`）。临时测试目录 `.llm-test/` 已清理、不入库。
- **两个真机坑**：`edit` 写回必须按字节区间（P-002）；Windows bash 必须验证原生 MSYS（P-003）。
- **门禁四连**：`cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && bash scripts/check-size`。
- **收尾四件事**：重写本卡、归档 handoff、沉淀 pitfalls/skills、体量核对。
