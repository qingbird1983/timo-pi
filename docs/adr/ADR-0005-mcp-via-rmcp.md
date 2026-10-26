# ADR-0005：MCP 采用官方 rmcp 实现，不自研

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0001；research/01

## 背景

pi 自研了 MCP 客户端（约 3,300 行 TS，含 protocol/transports/oauth 子模块）。Rust 生态有 MCP 官方 SDK **rmcp**。

## 决策

MCP 客户端直接采用官方 `rmcp` crate，本项目只做一层薄封装（工具发现 → 并入统一工具管线、命名空间 `mcp_<server>_<tool>`、配置文件兼容 `mcp.json` 语义）。

## 理由

- 消除自研协议栈的维护与正确性成本；MCP 规范演进由官方 SDK 跟进。
- pi 的经验表明 MCP 工具与本地工具走同一管线即可天然获得钩子、权限、exposure 能力——这部分才是本项目的增值点。

## 备选（否决）

- 自研 MCP 客户端（对齐 pi 做法）：否决——重复造轮子，rmcp 已覆盖 stdio/streamable HTTP/OAuth。

## 后果

- Phase 1 即集成；OAuth（MCP 登录流）可视 rmcp 支持度延后到 Phase 5+，MVP 支持 stdio + HTTP。
- rmcp 版本锁定并登记依赖准入表（standards/04 附录）。
