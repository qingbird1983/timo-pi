# 设计 05：pi 事件矩阵（逐项分类与发射点）

- 状态：定稿
- 日期：2026-10-26
- 关联：architecture/02 §3、04 §1；ADR-0003/0011；本目录 01/04
- 蓝本：pi 1.4.0 `types.ts` 的 `ExtensionEvent` 联合（共 41 种事件类型）

## 1. 分类口径

| 标记 | 含义 |
|---|---|
| **支持·通知** | 事件真实发射；handler 返回值被忽略（除非表中另注） |
| **支持·请求响应** | 事件真实发射；返回值按表中规则改内核态（经 04 的 `LoopHooks`/`ProviderHooks`） |
| **缓做** | 成员存在、可订阅；本版**不发射**，登记时返回"未实现"诊断（矩阵 §1.2 口径） |
| **不支持** | 成员存在、可订阅；本版**不发射**，登记时返回"本版不支持"诊断（no-op，永不 throw） |

- 每扩展每事件只记一条登记诊断，不按触发次数刷屏。
- 所有 handler 顺序、隔离、超时、abort 语义见 04；本表只定义"何时发、载荷是什么、返回什么"。

## 2. 事件总表

| # | 事件 | 状态 | 发射点 / 触发 | 返回值与差异 |
|---|---|---|---|---|
| 1 | `project_trust` | 不支持 | 无项目信任机制 | — |
| 2 | `resources_discover` | 不支持 | 无 skills/prompts/themes 资源发现 | — |
| 3 | `mcp_servers_change` | 不支持 | 扩展注册 MCP 不广播；用 `getMcpServers()` 查询 | — |
| 4 | `session_start` | 支持·通知 | 首次 `run` 建会话（reason=`"new"`）或 `resume`（reason=`"resume"`） | 无返回值 |
| 5 | `session_info_changed` | 不支持 | 无会话命名 | — |
| 6 | `session_before_switch` | 缓做 | 无会话切换 | — |
| 7 | `session_before_fork` | 缓做 | 无分叉 | — |
| 8 | `session_before_compact` | 缓做 | 无 compaction（D10） | — |
| 9 | `session_compact` | 缓做 | 同上 | — |
| 10 | `session_compact_failed` | 缓做 | 同上 | — |
| 11 | `session_shutdown` | 支持·通知 | CLI 退出前（reason=`"quit"`）；宿主不阻塞等待 | 无返回值 |
| 12 | `session_before_tree` | 缓做 | 无会话树导航 | — |
| 13 | `session_tree` | 缓做 | 同上 | — |
| 14 | `context` | 支持·请求响应 | 每次模型请求前；载荷**不含** system 消息 | 可返回 `{messages}` 或原地改 `event.messages`；system 消息由内核在结果上恢复（pi 同构） |
| 15 | `context_with_system` | 支持·请求响应 | 每个 `context` 处理完、system 恢复后 | 返回 `{messages}` 即最终 transcript（含 system）；不支持则保持不变 |
| 16 | `cache_warming_decision` | 不支持 | 无缓存预热 | — |
| 17 | `before_provider_request` | 支持·请求响应 | 请求体 JSON 构建完成、发送前 | 返回任意值即替换请求体；`undefined` 保持原样 |
| 18 | `before_provider_headers` | 支持·请求响应 | 请求头组装完成、HTTP 调用前 | 原地改 `event.headers`；值设为 `null` 删除该头；返回新的头对象亦可 |
| 19 | `after_provider_response` | 支持·通知 | 收到成功响应、消费流前 | `{status, headers}`；无返回值 |
| 20 | `provider_stream_event` | 不支持 | 流已在本项目归一化，无原始事件 | — |
| 21 | `before_agent_start` | 支持·请求响应（受限） | 每次 run 开始前、`agent_start` 之前 | 支持返回 `{systemPrompt}`（后者覆盖前者）；返回 `{message}` 注入自定义消息**不支持**（诊断） |
| 22 | `agent_start` | 支持·通知 | 与 `AgentStarted` 同步 | — |
| 23 | `agent_end` | 支持·通知 | 与 `AgentFinished` 同步；载荷 `messages` = 本次 run 新消息 | 无返回值 |
| 24 | `agent_before_settle` | 缓做 | 无 settle 边界模型 | — |
| 25 | `agent_settled` | 缓做 | 同上 | — |
| 26 | `ui_prompt_start` | 缓做 | 对话框生命周期事件不在本版 | — |
| 27 | `ui_prompt_end` | 缓做 | 同上 | — |
| 28 | `turn_start` | 支持·通知 | 与 `TurnStarted` 同步；`{turnIndex, timestamp}` | — |
| 29 | `turn_end` | 支持·通知 | 与 `TurnFinished` 同步；`{message, toolResults}` | 返回 `{continue}`/`{entries}` **不支持**（诊断） |
| 30 | `message_start` | 支持·通知 | assistant：`StreamEvent::Start`；user/toolResult：`MessageAdded` 前合成 | — |
| 31 | `message_update` | 支持·通知 | 每个 `AssistantStreamEvent`（Start/Delta/BlockEnd/…） | 载荷 `assistantMessageEvent` 为归一化事件 |
| 32 | `message_end` | 支持·通知 | `MessageAdded`（assistant 终态/user/toolResult） | 返回替换消息 **不支持**（诊断） |
| 33 | `tool_execution_start` | 支持·通知 | 与 `ToolCallStarted` 同步；`parentToolCallId` 恒 `undefined` | — |
| 34 | `tool_execution_update` | 支持·通知 | 与 `ToolCallUpdate` 同步 | — |
| 35 | `tool_execution_end` | 支持·通知 | 与 `ToolCallFinished` 同步；含 `durationMs` | — |
| 36 | `model_select` | 支持·通知 | `pi.setModel`/CLI 切模型；`{model, previousModel, source}` | source 本版仅 `"set"` |
| 37 | `thinking_level_select` | 支持·通知 | `pi.setThinkingLevel`/CLI 切档 | — |
| 38 | `user_bash` | 不支持 | CLI 无 `!`/`!!` 前缀 | — |
| 39 | `input` | 不支持 | 输入管线钩子不在本版矩阵 | — |
| 40 | `tool_call` | 支持·请求响应 | 工具参数校验前；**每个调用一次** | `event.input` 可原地改参（改后**不重新校验**，pi 同构）；返回 `{block, reason, terminate}`；首个 `block` 短路后续 handler |
| 41 | `tool_result` | 支持·请求响应 | 工具执行成功/失败后、结果入上下文前 | 字段级覆盖 `{content, details, structuredContent, isError, usage}`；替换 `content` 而未给 `structuredContent` 时后者被丢弃（pi 同构） |

## 3. 与 architecture/04 的矩阵校准

architecture/04 §1.1 的内联事件清单写于设计早期，与 pi 1.4.0 实际 API 有两处出入；本表为准，建议在 architecture/04 加修订注记（不改旧决策，只校准名称）：

| 原文 | 校准 |
|---|---|
| 支持 `compact` | pi 无 `compact` 事件；实际是 `session_before_compact` / `session_compact` / `session_compact_failed`，且本版无 compaction → **缓做** |
| 支持 `error` | pi 无 `error` 钩子；内核 `KernelEvent::Error` 只走诊断/CLI stderr，不向扩展伪造事件 |
| 未列 `message_*` / `tool_execution_*` | 直接由 `KernelEvent` 映射、零额外状态 → **支持·通知**（只读，不扩大可改面） |
| 未列 `input` / `user_bash` / `project_trust` 等 | 无对应内核能力 → **不支持**，登记即诊断，不静默 |
| §1.1 `registerVirtualModel` | 支持但受限：仅同 provider 路由（D23） |

> 判定标准：**有真实发射点且语义可完整表达**才标支持；"成员存在但永不触发"一律不标支持，避免扩展作者误判。

## 4. 载荷字段（与 01 §7 共用）

| 事件 | 关键字段（pi 名） |
|---|---|
| `session_start` | `reason: "new"｜"resume"`；其余 reason 本版不出现 |
| `context` | `messages`（无 system） |
| `context_with_system` | `messages`（含 system） |
| `before_provider_request` | `payload`（协议原生 JSON 对象） |
| `before_provider_headers` | `headers`（键值对象；`null` 删除） |
| `after_provider_response` | `status: number`、`headers: Record<string,string>` |
| `before_agent_start` | `systemPrompt: string`（只读快照）、`prompt`（用户原文）；`systemPromptOptions` 不支持 |
| `turn_start` / `turn_end` | `turnIndex`、`timestamp`；`message`、`toolResults` |
| `message_*` | `message`（01 §7 形状）；`message_update` 另带 `assistantMessageEvent` |
| `tool_execution_*` | `toolCallId`、`toolName`、`args`；start/end 另有 `isError`/`durationMs` |
| `model_select` | `model`、`previousModel`、`source` |
| `tool_call` | `toolCallId`、`toolName`（前缀名）、`input`（可改） |
| `tool_result` | `toolCallId`、`toolName`、`input`、`content`、`details`、`isError`、`usage?` |

## 5. 事件与 CLI/未来 GUI 的关系

- 本版 `ctx.mode="print"`：依赖 `mode === "tui"` 的扩展分支不会执行（如自定义组件）。
- 通知事件只用于展示/观测；任何需要"读回改值"的扩展必须用请求-响应事件，否则诊断提示"本版不支持"。
- GUI 阶段（Phase 3）可复用同一事件桥；届时 `mode` 与 `hasUI` 由门面注入，不改本表。

## 6. 待决问题

- `message_end` 的可改写语义在 pi 中存在；本版列通知。若后续样本（如脱敏扩展）强依赖，按"先改文档再加能力"补充。
- `agent_before_settle`/`agent_settled` 与 pi 的 settle 模型绑定；本项目循环没有该边界，暂不伪造。
