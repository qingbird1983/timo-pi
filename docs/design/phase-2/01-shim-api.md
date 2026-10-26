# 设计 01：pi 兼容 shim —— 模块、注册面、动作面与类型映射

- 状态：定稿
- 日期：2026-10-26
- 关联：architecture/04 §1；ADR-0003/0006/0011；roadmap/phase-2；本目录 00/04/05
- 蓝本：pi 1.4.0 `packages/coding-agent/src/core/extensions/types.ts`、`packages/ai/src/types.ts`（只读对照）

## 1. 虚拟模块与导入映射

安装期打包把下列 specifier 标记为 external，运行期由 QuickJS 的 loader 解析到内置 shim 源码（`include_str!`）。

| specifier | 提供内容 | 备注 |
|---|---|---|
| `@earendil-works/pi-coding-agent` | `defineTool`、`VERSION`、`isToolCallEventType`、`isBashToolResult` 等类型守卫、`CustomEditor`（惰性类） | 扩展最常导入的入口 |
| `@earendil-works/pi-ai` | `Type`（转出 typebox shim） | hello 样本从该路径取 `Type` |
| `@earendil-works/pi-agent-core` | 空命名空间 + 惰性 Proxy | 绝大多数导入是类型（编译期擦除） |
| `@earendil-works/pi-tui` | 惰性 Proxy（任意成员可导入、调用即 no-op + 诊断） | 对应矩阵 §1.3 |
| `typebox` | `Type` 构建器子集（§1.1） | 参数 schema 通货 |
| `@mariozechner/pi-coding-agent` 等旧名 | 与对应 `@earendil-works/*` **同一模块实例** | pi 的旧包名兼容习惯 |

- 类型-only 导入在 oxc 转换后消失，不需要运行时导出。
- 未知 specifier：打包期视为 npm 依赖解析；解析失败 → E103，运行期不兜底。
- `node:*` 与 Node 内建名（`fs`、`path`、`child_process`、`net`、`http`、`os`、`crypto`、`worker_threads`）一律打包期拒绝 → E104。

### 1.1 TypeBox 子集

只实现扩展定义 schema 所需的最小面；输出必须是**纯 JSON**（TypeBox 1.3 的 schema 本就是普通对象）。

| 成员 | 输出 |
|---|---|
| `Type.Object(props, opts?)` | `{type:"object", properties, required:[非 Optional 的键], ...opts}` |
| `Type.String/Number/Integer/Boolean/Null/Any/Unknown()` | `{type:...}` + opts 合并 |
| `Type.Array(items, opts?)` | `{type:"array", items, ...opts}` |
| `Type.Optional(s)` | 标记内部 `__optional`，由 `Object` 收入 `required` 判定并剥离标记 |
| `Type.Union([...])` | `{anyOf:[...]}`（全字面量时退化为 `enum`） |
| `Type.Literal(v)` | `{const:v}` |
| `Type.Record(k, v)` | `{type:"object", additionalProperties:v, propertyNames:k}` |

- 不支持 `Type.Ref`/`$ref` 解析、递归 schema、自定义 format 校验；出现即按普通 JSON 处理（校验失败 → 工具调用错误结果 E304）。
- 顶层必须是 `type:"object"`；否则注册被拒并记诊断（与内核工具契约一致）。

## 2. 身份与命名

- 扩展 id：manifest 的 `id`，缺省取安装目录名或文件 stem；规范化 `[a-z0-9][a-z0-9_-]{0,31}`。
- 冲突：同一 id 已存在时项目级覆盖用户级；同层冲突加 `-2/-3` 后缀并记 E204。
- **工具/命令名强制前缀** `plugin_<id>_<原名>`（architecture/04 §3）。模型看到、钩子收到、`getAllTools` 返回的都是前缀名（D13）。
- 扩展 flag 以 `<id>.<name>` 寻址；CLI 传 `--ext-flag <id>.<name>=<value>`，`pi.getFlag("name")` 在扩展内按裸名查询。

## 3. 注册面（`pi.*`）

状态口径：**支持**=完整语义；**受限**=可用但有下表差异；**缓做**=成员存在、返回"未实现"诊断；**no-op**=永不 throw、每扩展每成员一条诊断。

| pi 成员 | 状态 | 本实现语义 / 差异 |
|---|---|---|
| `registerTool(def)` | 支持 | 字段逐项见 §3.1；注册即入统一注册表 |
| `registerCommand(name, opts)` | 支持 | 名称加前缀；`handler(args, ctx)` 在 CLI 斜杠分发时调用；`getArgumentCompletions` 缓做 |
| `registerFlag(name, opts)` | 受限 | 值来自 `--ext-flag id.name=...`；缺省值照 `default`；D19 |
| `getFlag(name)` | 支持 | 返回 boolean/string/undefined |
| `registerShortcut` | 缓做 | 无快捷键面；订阅即诊断 |
| `registerMessageRenderer` | no-op | TUI 渲染面 |
| `registerEntryRenderer` | no-op | 同上 |
| `registerMarkdownTransformer` | no-op | 同上 |
| `registerToolRenderer` | no-op | 同上 |
| `registerProvider/unregisterProvider` | 受限 | 字段见 §3.2；加载期排队、装配前合并；运行期调用立即生效 |
| `registerMcpServer(name, cfg)` | 支持 | 配置子集见 §3.3；`session_start` 时连接；与 `mcp.json` 同名时后者优先 |
| `unregisterMcpServer(name)` | 支持 | 关闭连接并从注册表移除 |
| `getMcpServers()` | 支持 | 返回扩展注册的 server 列表（不含 `mcp.json` 的） |
| `registerVirtualModel/unregisterVirtualModel` | 受限 | 仅同 provider 内路由；`route(request, ctx)` 每次请求同步/异步返回 `{model, thinkingLevel?}`；跨 provider → 诊断并回退会话模型（D23） |
| `events` | 支持 | 进程内 pub/sub；扩展销毁自动退订；不跨进程、不持久化 |

### 3.1 `ToolDefinition` 字段

| 字段 | 状态 | 说明 |
|---|---|---|
| `name` / `label` / `description` | 支持 | `name` 加前缀；`label` 仅供诊断/未来 GUI |
| `promptSnippet` / `promptGuidelines` | 支持 | 进入系统提示（工具声明前入表的前提） |
| `parameters` | 支持 | TypeBox → JSON Schema；顶层非 object 拒绝 |
| `outputSchema` | 支持 | 存 `ToolDefinition.output_schema`；不做 `structuredContent` 二次校验 |
| `exposure` | 支持 | `direct/model-only/codemode/deferred/hidden` 一一映射；Phase 2 的 codemode/deferred 仍不声明给模型（同 Phase 1 D9） |
| `namespace` | 支持 | 合并 `name/description` 元数据（aliases/instructions 忽略 + 诊断） |
| `annotations` | 支持 | 四类 hint 透传 |
| `defaultActive` | 支持 | false → 注册为 Hidden，直至 `setActiveTools` 激活 |
| `prepareArguments` | 支持 | schema 校验前调用；返回值参与校验 |
| `prepareLoadout` | no-op | tool-search 面；订阅即诊断 |
| `reachesTools` | no-op | codemode 面 |
| `constrainedSampling` | 忽略 | 诊断一次 |
| `renderShell`/`renderCall`/`renderResult` | no-op | TUI 渲染面 |
| `executionMode` | 支持 | `sequential/parallel`；任一 sequential 使整批串行（内核既有语义） |
| `execute` | 支持 | 见 §6 |

### 3.2 `registerProvider` 字段

| 字段 | 状态 | 说明 |
|---|---|---|
| `baseUrl` | 支持 | 覆盖/新增模型端点 |
| `apiKey` | 支持 | 字面量、`$ENV`/`${ENV}` 引用（不落盘明文）；`!command` 暂不支持（诊断） |
| `api` | 支持 | 仅 `anthropic-messages` / `openai-completions` |
| `models` | 支持 | `id/name/reasoning/input/cost/contextWindow/maxTokens/headers/compat` 子集；未知字段忽略 + 诊断 |
| `headers` / `authHeader` | 支持 | 请求头注入 |
| `streamSimple` | 不支持 | 自定义流实现属 backlog；诊断并拒绝该 provider 的模型注册 |
| `oauth` / `images` / `classifiers` / `refreshModels` | 不支持 | 逐项诊断 |
| `unregisterProvider` | 支持 | 移除扩展 provider，内置模型恢复 |

### 3.3 `registerMcpServer` 配置子集

`type`（stdio/http，缺省按字段推断）、`command/args/env`（stdio）、`url/headers`（http）。其余字段（OAuth、超时覆盖等）忽略 + 诊断。注册的 server 与 `mcp.json` 走同一 `McpRuntime`。

## 4. 动作面

| pi 方法 | 状态 | 语义 / 差异 |
|---|---|---|
| `sendMessage(msg, opts)` | 缓做 | 自定义消息与延迟投递属 v2；返回"未实现"诊断 |
| `sendUserMessage(content, opts)` | 支持 | 运行中按 `deliverAs` 入 steering/followUp 队列；空闲时入 followUp 队列并由门面启动下一轮 |
| `appendEntry` | 缓做 | 会话自定义 entry 属 v2 |
| `exec(cmd, args, opts)` | 支持 | 需 `exec` 能力；cwd 固定工作目录；返回 `{stdout, stderr, code}`；超时/取消中断子进程 |
| `getActiveTools()` | 支持 | 返回**声明给模型**的工具名 |
| `getAllTools()` | 支持 | `{name, description, parameters, promptGuidelines, exposure, namespace?, annotations?, sourceInfo}` |
| `setActiveTools(names)` | 支持 | 未知/hidden 名忽略；`codemode/deferred` 仍不可声明（同 Phase 1） |
| `getSettings()` | 支持 | 全局+项目合并后的 JSON 快照；凭证仍以 `$ENV` 引用形式返回，不展开 |
| `getCommands()` | 支持 | 当前会话可用斜杠命令清单 |
| `setModel(m)` | 支持 | 接受 `{provider, id}`（pi Model 对象子集）；成功返回 true；凭证缺失返回 false；写 `model_change` entry 并发射 `model_select` |
| `getThinkingLevel()` / `setThinkingLevel(level)` | 支持 | 夹取到模型能力；写 `thinking_level_change` entry 并发射事件 |
| `setSessionName/getSessionName` | no-op | 会话命名不在本版矩阵；诊断 |
| `setLabel` | no-op | 会话 entry 标签不在本版格式；诊断 |

> `sendUserMessage` 的空闲启动由 `Kernel` 负责：扩展宿主只入队并置"有待处理消息"标志；CLI 在本次 run 结束后检查并启动，避免扩展线程直接递归进入 run。

## 5. `ctx` 面

### 5.1 公共上下文

| 成员 | 状态 | 语义 / 差异 |
|---|---|---|
| `ui.notify(msg, type)` | 支持 | 写 stderr，带扩展 id 前缀 |
| `ui.select(title, options, opts)` | 支持 | TTY：逐行编号选择；非 TTY：`undefined` + 诊断 |
| `ui.confirm(title, msg, opts)` | 支持 | TTY：y/N；非 TTY：`false` + 诊断 |
| `ui.input(title, placeholder, opts)` | 支持 | TTY：读一行；非 TTY：`undefined` + 诊断 |
| `ui.*` 其余（`editor`、`setStatus`、`setWidget`、`setFooter`、`setHeader`、`setTitle`、`setWorking*`、`custom`、`pasteToEditor`、编辑器读写、autocomplete、theme、`getToolsExpanded`…） | no-op | 每扩展每成员一条诊断；`custom<T>()` 返回永不 settle 的 Promise（扩展不会误以为拿到结果） |
| `mode` | 支持 | 固定 `"print"`（本版 CLI 非 TUI） |
| `hasUI` | 支持 | `stdin`+`stdout` 均为 TTY |
| `cwd` | 支持 | 工作目录绝对路径 |
| `signal` | 支持 | AbortSignal 子集（D18） |
| `abort()` | 支持 | 等价 `Kernel::abort` |
| `isIdle()` / `hasPendingMessages()` | 支持 | 运行态查询 |
| `getSystemPrompt()` | 支持 | 当前生效系统提示（含 before_agent_start 结果） |
| `sessionManager.*` | 缓做 | 任意方法调用返回"未实现"诊断（读接口属 v2） |
| `modelRegistry.*` | no-op | 惰性 Proxy + 诊断 |
| `model` / `thinkingLevel` | 支持 | 快照对象；字段对齐 pi Model 子集 |
| `scopedModels` | 支持 | 空数组（本版无模型作用域） |
| `isProjectTrusted()` | 支持 | 恒 true（本版无项目信任机制） |
| `getContextUsage()` | 受限 | 返回 `undefined` + 一次性诊断（本版无 token 估算） |
| `compact(opts)` | 缓做 | 无 compaction；诊断 |
| `shutdown()` | no-op | 诊断；不终止宿主 |

### 5.2 `ExtensionCommandContext` 增量

| 成员 | 状态 | 说明 |
|---|---|---|
| `waitForIdle()` | 支持 | 等待当前 run 结束 |
| `getSystemPromptOptions()` | no-op | 诊断 |
| `newSession` / `fork` / `switchSession` / `reload` | 缓做 | 诊断；`withSession` 永不调用 |
| `navigateTree` | no-op | TUI 面 |

### 5.3 `ExtensionToolContext` 增量

| 成员 | 状态 | 说明 |
|---|---|---|
| `tools` | 支持 | 空数组（嵌套调用未开放） |
| `executeTool()` | no-op | 返回 `{isError:true, content:"not supported"}` + 诊断（D24） |

## 6. 工具执行契约

JS 侧签名（与 pi 一致）：

```ts
execute(toolCallId, params, signal, onUpdate, ctx): Promise<AgentToolResult>
```

- **入参**：`params` 已经过 `prepareArguments` → JSON Schema 校验；`signal` 绑定 run 取消；`onUpdate(partial)` 映射为 `ToolCallUpdate` 事件（partial 字段同结果）。
- **返回**：`{content, details?, structuredContent?, usage?, isError?, terminate?}`。
  - `content` 元素：`{type:"text",text}` 或 `{type:"image",data,mimeType}`；其他类型 → E304。
  - `details` 缺省 `null`；`terminate` 缺省 false。
- **失败**：throw / Promise reject → 错误工具结果（`is_error=true`，文案含异常 message），并发 E303 诊断；绝不 panic、不终止会话。
- **超时**：工具执行**无 30s 预算**（与钩子不同）；只能由取消令牌中断（`signal.aborted`）。
- **并发**：同一扩展的工具调用在 JS 线程上串行进入；宿主 I/O 仍可异步交错（D11）。

## 7. JS ↔ Rust 类型映射

### 7.1 内容块（`Content`）

| Rust（会话线格式） | JS（pi 形状） |
|---|---|
| `{type:"text", text}` | `{type:"text", text}` |
| `{type:"thinking", thinking, signature?, redacted}` | `{type:"thinking", thinking, thinkingSignature?, redacted}` |
| `{type:"image", data, mime_type}` | `{type:"image", data, mimeType}` |
| `{type:"tool_call", id, name, arguments, arguments_error?}` | `{type:"toolCall", id, name, arguments, argumentsError?}` |

### 7.2 消息（`Message`）

| Rust role | JS |
|---|---|
| `user` | `{role:"user", content: string \| blocks, timestamp}`；单文本块折叠为 string |
| `assistant` | `{role:"assistant", content, api, provider, model, responseModel?, responseId?, usage, stopReason, rawStopReason?, errorMessage?, thinkingLevel?, durationMs?, timestamp}` |
| `tool_result` | `{role:"toolResult", toolCallId, toolName, content, isError, usage?, durationMs?, timestamp}` |
| `system` | `{role:"system", content: string, timestamp}`；仅 `context_with_system` 可见 |

- `usage` 字段 camelCase：`cacheRead/cacheWrite/cacheWrite1h/totalTokens`。
- `stopReason` 线值一致：`pending|stop|length|toolUse|error|aborted|deferred`。
- 工具结果的 `details` 不落会话；`tool_result` 钩子直接取执行期 `ToolOutput.details`。

## 8. 运行时基线与 `VERSION`

- `VERSION = "1.4.0"`：表示**对齐的 pi API 参考版本**，不随本项目版本变化（D15）；升级须走矩阵评审。
- 提供的全局基线（QuickJS 默认之外）：`console.*`（写 stderr）、`setTimeout/clearTimeout/setInterval/clearInterval`、`queueMicrotask`、`fetch`（受网权限）、`TextEncoder/TextDecoder`、`URL/URLSearchParams`、`atob/btoa`、`structuredClone`（JSON 语义）。
- **不提供**：`process`、`Buffer`、`require`、`node:*`、DOM。使用即运行期 ReferenceError → E202（工厂期）或 E303（工具期），并记诊断。
- 定时器由扩展线程驱动，扩展卸载/run 取消时清理；单扩展待触发定时器上限 256，超出 → E306 并丢弃。

## 9. 待决问题

- `ui.custom<T>()` 永不 settle 的 Promise 是否会令扩展永久挂起：宿主侧仍需在 30s 钩子预算内切断（04 §5），此处只保证不 throw。
- `details` 在 GUI 阶段是否需要落会话：Phase 3 设计时评估，避免现在改会话格式。
- TypeBox 的 `format`/`$ref` 扩展面是否需要：当前记录为不支持，等真实样本触发再议。
