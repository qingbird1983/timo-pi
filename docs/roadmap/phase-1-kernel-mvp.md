# Phase 1：内核 MVP

- 状态：未开始（开工前置条件见 §6）
- 参考估时：6~10 周（单人全职）
- 关联：ADR-0001/0005/0007/0009；architecture/02、03、05

## 1. 目标（一句话）

产出一个**无界面但完整可用的 agent 内核**：给它一个任务，它能读文件、改文件、运行命令、汇报结果，并把全过程存进可恢复的会话。

## 2. 范围

### 做
- workspace 与 crates 骨架（architecture/01 命名）
- timo-ai：anthropic-messages + openai-completions 两种协议的**流式**适配；协议 trait；模型目录数据（同步 pi 目录子集）；认证枚举（api-key/env；OAuth 仅占位）
- timo-agent：agent 循环；事件总线（architecture/02 §3 事件集全集 emit）；CancellationToken 贯穿
- timo-tools：ToolDefinition 元模型（exposure/命名空间/annotations 齐全）+ 内置工具 read/write/edit/bash(Windows 走 PowerShell 支持)/glob/grep
- timo-session：会话格式定稿并实现（append-only、树形结构、未知类型跳过、崩溃安全）
- timo-mcp：rmcp 封装（stdio + HTTP；MCP 工具并入统一管线）
- timo-config：设置文件、供应商/模型解析、凭证引用
- timo-cli：非交互冒烟壳（仅验收载体，不打磨交互）
- **构建卫生（standards/08 §7）**：`.cargo/config.toml` 瘦身 profile（line-tables-only 等）；`scripts/clean-stale` 按龄清扫脚本；`check-size` 盘点 target/ 对照 15G 软上限
- CI：fmt + clippy + test + 会话格式兼容测试

### 不做（显式排除）
OAuth、其余三种协议、compaction、会话树的用户功能面、扩展宿主（Phase 2）、GUI、日志美化、性能优化

## 3. 文档交付物（实现前定稿）

- `docs/design/phase-1/`：各 crate 详细设计（数据类型定稿、协议 trait 签名、错误类型表、配置文件格式）
- architecture/05 会话格式：草稿 → **定稿**（含 §6 待决问题决议）
- standards/04 附录：依赖准入表初版（锁版本）

## 4. 验收场景（逐条执行，结果进进度报告）

| # | 场景 | 通过标准 |
|---|---|---|
| A1 | 在任一示例项目目录下达"把文件 X 中的 A 改成 B 并验证" | agent 自主完成 读→编辑→运行验证→汇报；全过程可见流式输出 |
| A2 | 打开 A1 生成的会话文件 | JSONL 可读、entry 树完整；重列会话列表可见该会话并可恢复继续对话 |
| A3 | 会话中途 Ctrl+C / kill -9 | 进程干净退出/被杀；会话文件可读，未写完的半条 entry 被丢弃无损坏 |
| A4 | 配置一个 stdio MCP server | 其工具出现在工具列表、可被模型调用、走统一管线（命名空间 `mcp_*`） |
| A5 | CI 全绿 | fmt/clippy/test 通过；协议 fixture 回放测试覆盖两协议的流式/工具调用/错误路径 |
| A6 | 接缝核对 | architecture/03 七条接缝逐条达标并写入进度报告 |

## 5. 风险与对策

- 协议细节偏差 → fixture 以真实抓包为基础，pi 协议层行为作对照（research/01 §5 入口表）。
- 会话格式考虑不周 → 定稿评审时按 architecture/05 §6 清单逐项决议，宁缓不烂。

## 6. 开工前置条件

- [ ] 本文档状态转"定稿"（当前已定稿）
- [ ] `docs/design/phase-1/` 详设文档定稿（**未完成，开工前必做**）
- [ ] architecture/05 定稿
- [ ] 所有者对"Phase 1 开工"知情（看板/CHANGELOG 可见）
