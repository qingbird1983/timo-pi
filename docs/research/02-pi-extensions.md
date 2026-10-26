# 研究 02：pi 扩展系统分析与兼容可行性

- 状态：定稿
- 日期：2026-10-26
- 依据：`D:\pi\packages\coding-agent\src\core\extensions\`（types.ts 2,308 行 / runner.ts 1,565 行 / loader.ts 880 行）、`docs/extensions.md`、`docs/packages.md`

## 1. 机制全貌

- **形态**：`.ts` / `.js` 文件，default export 工厂 `(pi: ExtensionAPI) => void | Promise<void>`。
- **加载**：**jiti 在 pi 进程内直接 import**，无预编译、无 RPC、无沙箱。官方文档原话：扩展与 pi 同进程、同操作系统权限，可访问提示词、工具调用、文件、凭证、会话历史。
- **模块别名**：jiti 把 `@earendil-works/pi-coding-agent`、`pi-agent-core`、`pi-tui`、`pi-ai`、`typebox` 全部映射到宿主同一实例；保留 `@mariozechner/*` 旧包名别名。
- **发现路径**：项目级 `.pi/extensions/`、用户级 `~/.pi/agent/extensions/`、settings 配置、`-e` 参数、`pi install` 的 npm/git 包；目录只扫一层。
- **内置扩展**：`builtin:mcp`、`builtin:codemode`、`builtin:tool-search`、llama.cpp；前三者可被第三方同名注册整体接管（replaceable）。
- **版本承诺**：**无 semver 承诺**，靠导出类型声明 + compat 入口 + 旧包名别名做软兼容。

## 2. ExtensionAPI 能力面（兼容矩阵的事实依据）

- `on(event, handler)`：**40+ 种事件**——project_trust、session 生命周期、compact、context / context_with_system（变换整个消息列表）、cache_warming_decision、before_provider_headers / before_provider_request / after_provider_response / provider_stream_event（拦截改写 LLM 请求与响应）、before_agent_start（可整体替换系统提示词）、agent/turn/message/tool_execution 生命周期、tool_call（可改参/阻断）、tool_result（可改写）、user_bash、input。
- `registerTool(ToolDefinition)`：TypeBox 参数 schema、execute、outputSchema、**exposure**（direct|model-only|codemode|deferred|hidden）、namespace、annotations、promptSnippet/promptGuidelines（注入系统提示词）。
- `registerCommand`（slash 命令）、`registerShortcut`、`registerFlag`。
- `registerProvider`：自定义 LLM 供应商（baseUrl、apiKey 支持 `$ENV`/`!command`、OAuth、`streamSimple` 完全自定义流式实现、模型目录）。
- `registerMcpServer` / `registerVirtualModel`、`events` 总线。
- 动作：`sendMessage`、`sendUserMessage`、`appendEntry`、`exec`、工具开关、`setModel`、`getSettings` 等。
- **UI 面（本项目放弃兼容的部分）**：`ctx.ui`（select/confirm/input/editor/notify、setWidget/setFooter/setHeader/setTitle、`custom<T>()` 任意 TUI 组件、编辑器操控、autocomplete）；四类渲染注册 `registerMessageRenderer` / `registerEntryRenderer` / `registerMarkdownTransformer` / `registerToolRenderer`；工具的 `renderCall`/`renderResult` 直接返回 TUI 组件；继承 `CustomEditor` 替换主编辑器。

## 3. 与 MCP 的关系

MCP 是扩展之外的零代码接入路径（`mcp.json` 纯配置，stdio + streamable HTTP/OAuth），MCP 工具与扩展工具走**同一条管线**、同样经过 tool_call/tool_result 钩子。两者双向互通（扩展可注册 MCP server；MCP 支持本身是可被替换的内置扩展）。

## 4. 兼容可行性结论（本项目的依据）

| 扩展类型 | pi 官方示例 | 本项目结局 |
|---|---|---|
| 纯编排（无 UI、无/可打包 npm 依赖） | hello、permission-gate、protected-paths、todo、auto-commit-on-exit、prompt-customizer、dynamic-tools | **可兼容**（QuickJS + 打包 + API shim） |
| 编排 + 少量 UI 降级 | qna、jev-router、custom-provider-* | 可兼容，UI 调用降级为 no-op + 诊断 |
| UI 渲染/编辑器/TUI 组件 | modal-editor、snake、space-invaders、各 renderer | **不兼容**，能力由 GUI 插件面（TSX）替代，需重写 |
| 直接用 node:fs/child_process、重 Node 依赖 | gondolin、ssh 等 | 视打包结果，多数不可用 |

决策与实现设计见 ADR-0003、ADR-0006、`architecture/04-plugin-system.md`。
