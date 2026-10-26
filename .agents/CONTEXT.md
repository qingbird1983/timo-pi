# 工作上下文交接卡

- 更新：2026-10-26（会话主题：**Phase 1 收尾完成**；随后修正目录笔误 `.agent/` → `.agents/`）

## 当前阶段与状态

- **Phase 1 ✅ 完成**（2026-10-26，tag `phase-1.end`）：A1~A6 全部通过，CI run 40308144023 全绿。
- 远端：`github.com/qingbird1983/timo-pi`（私有，SSH）。
- 看板：`docs/roadmap/00-roadmap.md` §1；报告：`docs/design/phase-1/report.md`（定稿）。
- **Phase 2（扩展宿主）未开始**，等待所有者开工通知。

## 上一轮完成了什么

- **真实验收（DeepSeek `deepseek-flash`）**：A1 改文件、A2 恢复续聊、A3 强杀恢复、A4 模型调用 MCP，全部通过。
- **修复 `edit` 行重复**（P-002）与 **Windows bash 落 WSL**（P-003）。
- **推送与 CI**：建私有仓库并推送；首轮 CI 失败定位为 shell 测试辅助函数绕过产品探测（GitHub runner 的 bash 是空 WSL），修复后 CI 全绿。
- **收尾**：报告转定稿、看板更新、CHANGELOG 0.1.12、tag `phase-1.end`。
- **治理修正**：`.agent/` 目录笔误并入 `.agents/`（git mv 保留历史），活跃引用全部更新，CHANGELOG 0.1.13 登记；历史流水不改写。

## 下一步（按优先级）

1. 所有者下达 Phase 2 开工通知；先读 `docs/roadmap/phase-2-extension-host.md` 与相关设计。
2. Phase 2 前可选：真实 HTTP MCP server 人工验证；Ctrl+C 两段语义信号级复核。
3. Phase 2 设计时补：MCP 发现失败自动重试、扩展钩子结构占位。

## 未决问题

- MCP 工具"发现失败"不自动重试发现（调用层会重连）。
- `LoopConfig` 七个扩展钩子结构占位未落地（D10；Phase 2 设计）。
- 真实第三方 HTTP MCP server 未验证。
- Ctrl+C 两段语义仅代码级验证（kill -9 已实测）。

## 关键提醒

- **方向纪律（ADR-0011）**：D1~D10 是全部已授权偏离；创新走 backlog。
- **真机坑**：`edit` 写回必须按字节区间（P-002）；Windows bash 必须验证原生 MSYS（P-003）；适配器必须产出 BlockStart/Delta/BlockEnd（P-001）。
- **DeepSeek 配置**：OpenAI 兼容 `https://api.deepseek.com`，模型 `deepseek-flash`/`deepseek-v4-pro`，凭证 `$DEEPSEEK_API_KEY`（ValueRef 用 `$NAME`）。
- **门禁四连**：`cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && bash scripts/check-size`；CI 在 GitHub Actions，windows-latest。
- **收尾四件事**：重写本卡、归档 handoff、沉淀 pitfalls/skills、体量核对。
