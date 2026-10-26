# 研究 01：pi 内核规模与架构分析

- 状态：定稿
- 日期：2026-10-26
- 调查对象：本机克隆 `D:\pi`（GitHub: earendil-works/pi，原 badlogic/pi-mono）

## 1. pi 是什么

纯 TypeScript ESM monorepo（npm workspaces），Node ≥ 22.19，定位 "minimal, extensible agent harness"：一个可扩展的命令行 AI 编码代理。全仓 src 约 **18.2 万行**、测试约 **19.9 万行**（测试与源码约 1:1）。

## 2. 包清单与规模

| 包 | 用途 | src 行数 |
|---|---|---|
| pi-ai | 统一多供应商 LLM API（70+ provider 目录 + 协议适配层） | 28,511 |
| pi-agent-core | agent 循环、工具调用、状态管理（仅 6 个文件，很薄） | 2,541 |
| pi-mcp | 独立 MCP 客户端（**自研** JSON-RPC/MCP，不用官方 SDK） | 3,287 |
| pi-codemode | QuickJS(WASI) 沙箱，让模型写脚本调用工具 | 3,037 |
| chord | 插件宿主运行时（契约/复制状态/RPC） | 15,193 |
| pi-durable | 持久化会话/任务/文档运行时（兼容 Cloudflare Workers） | 27,700 |
| pi-env | SSH 远程执行环境 daemon（daemon 本体已是 Rust） | 2,346 |
| pi-telemetry | 遥测契约与 schema | 1,572 |
| pi-tui | 终端 UI 库（**本项目不移植**） | 19,674 |
| pi-coding-agent | CLI 主包：内置工具、会话、扩展系统、多模式 | 76,413 |
| pi-evals | 评测 harness（私有） | 1,446 |

## 3. 内核 vs TUI 边界（本项目重写范围）

- **TUI 部分** = `pi-tui` 包 + `coding-agent/src/modes/interactive/`（约 2.4 万行交互模式）→ **不移植**。
- **必需内核**（跑起一个无 TUI 的 agent 所需）≈ **8.6 万行**：pi-ai + agent-core + mcp + codemode + coding-agent 的 core/(35,590)/extensions/(7,585)/cli/(1,458)/utils/(3,823)/顶层。
- **外围运行时**（chord + durable + env + telemetry）≈ 4.7 万行 → 本项目**不纳入**（见 ADR-0008）。

## 4. 关键技术事实

- Schema 用 **TypeBox**（全仓无 zod）；TypeBox 兼容 JSON Schema。
- pi-ai 走各官方 SDK（openai、@anthropic-ai/sdk、@google/genai、aws-sdk/bedrock），协议适配层在 `src/api/`（anthropic-messages、openai-completions、openai-responses、bedrock-converse-stream、google-generative-ai 等）；**70+ provider 大部分只是模型目录数据**（生成文件 `models.generated.ts`）。
- 扩展加载用 **jiti**（进程内 TS 直载）；codemode 用 **quickjs-wasi**。
- 深度依赖 Node 内建模块（fs/path/child_process/crypto/worker_threads 等）。
- 构建：库包直接 tsc，CLI 入口 esbuild 打包单文件。
- 仓库内已有非 JS 代码两处：env 的 Rust daemon、tui 的 C/N-API 预编译模块（均与本项目范围无关）。

## 5. 关键入口文件（供未来对照行为时查阅）

| 职责 | 路径（D:\pi\packages\…） |
|---|---|
| CLI 入口 | coding-agent/src/cli.ts → main.ts |
| agent 循环 | agent/src/agent-loop.ts、agent.ts |
| 会话 | coding-agent/src/core/agent-session.ts（4,458 行）、session-manager.ts（2,018 行） |
| 工具系统 | coding-agent/src/core/tools/（read/bash/edit/write/grep/find/ls/powershell） |
| 扩展系统 | coding-agent/src/core/extensions/（types.ts 2,308 行、runner.ts、loader.ts） |
| LLM 层 | ai/src/index.ts、providers/、api/ |
| MCP | mcp/src/client.ts + protocol/ + transports/ |

## 6. 对本项目的结论

- 内核 MVP（2 协议 + 循环 + 工具 + 会话 + MCP）对应约 8.6 万行 TS 的功能子集，忠实移植估 5~7 人月；MVP 6~10 周（详见 roadmap）。
- 协议适配是最大单项；模型目录是数据问题，可同步 pi 的目录。
- MCP 自研部分可用官方 rmcp 替代（ADR-0005）。
