# Timo-pi

**用 Rust 重写的原生编码代理：对齐 [pi](https://github.com/earendil-works/pi) 的内核能力，用原生桌面应用取代终端界面。**

[English](README.md) · 许可证：[MIT](LICENSE)

## Timo-pi 是什么

Timo-pi 用 Rust 重写 [pi](https://github.com/earendil-works/pi) 的内核——agent 循环、大模型接入层、工具系统、会话、扩展系统——并用基于 gpui-kit + NSH 的**原生桌面应用**取代终端 UI。**Windows 优先**，macOS/Linux 保证可构建。MIT 开源。

## 特性一览

- **Rust 内核**：流式大模型接入（MVP：Anthropic Messages 与 OpenAI 兼容两类协议）、带事件总线的 agent 循环、内置工具（读/写/编辑/命令执行/搜索）、崩溃安全的追加式会话存储。
- **开箱即用的 MCP**：基于官方 `rmcp` SDK；MCP 工具与内置工具走同一条管线（钩子、权限、命名空间）。
- **双轨插件系统**
  - *A 轨（兼容）*：在内嵌 QuickJS 沙箱中运行 pi 的**编排类扩展**（自定义工具、命令、事件钩子、供应商）；TUI 专属成员优雅降级为 no-op 并逐项输出诊断。
  - *B 轨（原生）*：用 **TSX** 编写的 GUI 插件系统，能力沙箱执行、**签名热更包**分发、经 NSH 的 GPUI 后端渲染进主窗口。
- **文档先行**：设计文档、决策记录（ADR）、规范与阶段路线图先于代码落盘；每个阶段都有非技术语言描述的验收场景。

## 当前状态

未进入编码：Phase 0（仓库、规范与设计基线）已完成，实现从 Phase 1（内核 MVP）开始。

- **路线图与进度看板**：[docs/roadmap/00-roadmap.md](docs/roadmap/00-roadmap.md)
- 进展流水：[CHANGELOG.md](CHANGELOG.md)

## 给项目所有者的说明

你只需要看三处：**进度看板**（roadmap/00）、**CHANGELOG**（每次进展的流水）、每阶段的**进度报告**（`docs/design/phase-N/report.md`）。看不懂的词查 [术语表](docs/glossary.md)。

## 文档目录

| 目录 | 内容 |
|---|---|
| [`docs/research/`](docs/research/) | 调研沉淀：pi 内核与扩展系统、gpui-kit + NSH 技术栈、参考项目分析 |
| [`docs/adr/`](docs/adr/) | 决策记录（每个重大决定"为什么这么做"） |
| [`docs/architecture/`](docs/architecture/) | 系统设计：分层、关键接缝、插件系统、会话格式 |
| [`docs/roadmap/`](docs/roadmap/) | 阶段规划、范围、验收场景；`00-roadmap.md` 即进度看板 |
| [`docs/standards/`](docs/standards/) | 仓库/提交/文档/代码/测试/安全/AI 协作规范（8 篇） |
| [`docs/glossary.md`](docs/glossary.md) | 术语表（非技术读者友好） |

## 许可证

[MIT](LICENSE) © 2026 Timo-pi contributors
