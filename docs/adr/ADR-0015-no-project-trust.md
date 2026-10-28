# ADR-0015：不做项目信任门控

- 状态：定稿
- 日期：2026-10-28
- 关联：ADR-0008（范围排除）、ADR-0011（内核期对齐 pi）、ADR-0012（工具审阅接缝）；phase-3.5 详设 12（已删除）

## 背景

pi 1.4.0 有项目信任机制：首次进入工作目录时询问是否信任，据此决定项目级资源（设置、MCP、扩展、技能、模板、系统提示）是否加载。本仓曾照此补齐内核决策链、扩展 `project_trust` 事件、CLI `--approve`/`--no-approve` 与 `/trust`、桌面五项选择弹框及 `trust.json` 存储（详设 12，D80~D87），批 A/B/C 已落地并通过测试。

真机启动桌面端后暴露产品定位问题：该交互与 VS Code 的 "Do you trust the authors of the files in this folder?" 同型，而本产品不是 VS Code 式工作区编辑器——用户预期是打开项目即用，不应在启动路径上多一道信任询问。

## 决策

1. **整体移除项目信任门控**：项目级资源无条件加载；启动不询问，不写 `trust.json`。
2. **撤回面**：内核 `trust` 模块与决策链；CLI `--approve`/`--no-approve` 与 `/trust`；扩展 `project_trust` 事件与 `ctx.isProjectTrusted()` 的真实取值（恢复恒 `true`，保留 API 形状以免扩展生态断裂）；子代理透传；桌面信任事件与弹框；详设 12 与 D80~D87。
3. **保留的安全垫**：工具执行前的审阅接缝（ADR-0012）、扩展能力授权（`permissions.json`）、NSH 包签名链、OS 级隔离。删除的只是"项目资源是否自动注入"的交互门控，不是安全边界。
4. **不再以 pi 的项目信任为蓝本**：ADR-0011 的"忠实复刻"不覆盖该机制，本 ADR 作为明确排除。

## 后果

- 正面：启动路径无额外询问，符合"打开即用"的产品预期；删除约千行实现与测试，维护面变小。
- 代价：项目目录中的 `mcp.json`、扩展、技能、模板、`SYSTEM.md` 等会被自动加载；恶意仓库可借项目资源影响模型上下文。缓解依赖审阅接缝与用户对项目的知情选择（打开项目即代表接受）。
- 迁移：曾写入的 `trust.json` 不再读取；文件留在用户数据目录不影响行为。
- 保留：原先为信任功能拆出的文件（`kernel/ready.rs`、`prompt/warnings.rs`、`shim/runtime_dispatch.js`、`app_events.rs`、`main_tests.rs`）按体量规范继续保留，不回并。
- 扩展 API：`ctx.isProjectTrusted()` 恒为 `true`，既有扩展代码无需改动。
