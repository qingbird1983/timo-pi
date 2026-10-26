# 设计 02：timo-agent —— 循环、事件总线与取消

- 状态：定稿
- 日期：2026-10-26
- 关联：architecture/02 §2~5、03 接缝 #2/#6/#7；ADR-0009/0011；pi: packages/agent/src/agent-loop.ts、agent.ts

## 1. 定位

agent 循环是 pi 语义对齐的**核心**：消息→模型→工具调用→回填→再请求，直到无工具调用且无排队消息。循环内**零硬编码 hook/扩展分支**（接缝 #2 铁律）：一切外泄走事件总线，一切注入点走 LoopConfig。

## 2. 事件总线（architecture/02 §3 的落地）

```rust
#[derive(Clone)] pub enum KernelEvent {
    AgentStarted, AgentFinished { messages: Arc<Vec<Message>> },
    TurnStarted, TurnFinished { message: Arc<Message>, tool_results: Arc<Vec<Message>> },
    MessageAdded { message: Arc<Message> },                 // message_end 的持久化对应物
    StreamEvent { event: Arc<AssistantStreamEvent> },       // 流式粒度（Start/Delta/…）
    ToolCallRequested { call: Arc<ToolCall> },              // Phase 1 仅通告；veto 接口 Phase 2
    ToolCallStarted { tool_call_id, tool_name, args }, ToolCallFinished { tool_call_id, tool_name,
        result: Arc<ToolResultMessage>, duration_ms: Option<u64> },
    ProviderRequest { provider, model }, ProviderResponse { provider, model, usage: Usage },
    CompactionEvent { /* Phase 5+ 预留，Phase 1 不 emit */ },
    Error { message: Arc<Message>, context: String },
}
pub struct EventBus { tx: broadcast::Sender<KernelEvent> }   // capacity 1024；滞后订阅者 Lagged 不致命
impl EventBus { pub fn subscribe(&self) -> broadcast::Receiver<KernelEvent>; }
```

pi AgentEvent ↔ KernelEvent 映射：`agent_start/end→AgentStarted/Finished`；`turn_start/end→TurnStarted/Finished`；`message_start/update/end→StreamEvent`（update 携带 AssistantStreamEvent）+ `message_end→MessageAdded`；`tool_execution_*→ToolCall*`。payload 全为快照（Arc），订阅者不得改内核态。

## 3. 循环状态与配置

```rust
pub struct AgentLoop { /* 无状态：每次 run 一个 future */ }
pub struct LoopConfig {
    pub model: Model, pub api: Arc<dyn ProviderApi>, pub tools: Arc<ToolRegistry>,
    pub thinking_level: ThinkingLevel,
    pub auth: Arc<AuthResolver>,                      // 每次请求重解析（对齐 pi getApiKey）
    pub session: Option<Arc<SessionWriter>>,          // message 落盘挂点
    pub tool_execution: ToolExecutionMode,            // Parallel(默认)/Sequential
    pub retry: RetrySettings,
    // —— 注入点（Phase 1 全为 None；Phase 2 由扩展宿主填充，结构先占位 = 接缝）——
    pub transform_context: Option<ContextTransform>,  // 请求前改写上下文
    pub prepare_request: Option<PrepareRequest>,      // 每次请求前
    pub before_tool_call: Option<BeforeToolCall>,     // 可 block（veto 前身）
    pub after_tool_call: Option<AfterToolCall>,       // 字段级覆盖结果
    pub finish_turn: Option<FinishTurn>,              // end/continue 决策
    pub get_steering_messages / get_follow_up_messages: Option<MessageSource>,
}
```

## 4. 循环算法（对齐 pi runLoop，逐条）

1. **开工**：声明工具变更检查（Phase 1 工具集恒定→跳过）；emit `AgentStarted`+`TurnStarted`；新消息（user prompt）入上下文并 emit `MessageAdded`。
2. **外层 loop**：`has_more_tool_calls = true`；进入内层。
3. **内层 loop**（`has_more_tool_calls || pending 非空` 时继续）：
   a. 轮次开始：非首轮时先 `prepare_next_turn`（占位）；re-poll steering（此前轮空才重询）；emit `TurnStarted`。
   b. pending 消息（steering/follow_up）逐条 emit `MessageAdded` 并入上下文。
   c. `prepare_request`（占位）→ `stream_assistant_response`：折叠上下文（system 折叠 + 工具 schema 列表）→ `api.stream(...)` → 事件转发 `StreamEvent`；终态消息入上下文、emit `MessageAdded`。
   d. `stop_reason ∈ {Error, Aborted}` → 轮次重试判定（timo-ai retry 层 2；耗尽后）emit `TurnFinished{tool_results:[]}`+`AgentFinished` → **硬退出**。
   e. 提取 ToolCall 块：无 → `has_more_tool_calls=false`；有 → `stop_reason=Length` 时**全部不执行**、逐个生成错误 tool_result（"output token limit"）；否则走 §5 执行管线；结果逐条入上下文 + `MessageAdded`。
   f. emit `TurnFinished`；`finish_turn`（占位）决策 end→硬退 / continue→至多一轮纯上下文续跑。
   g. 轮询 steering → 有则回内层；否则轮询 follow_up → 有则回内层；再否则若 explicit_continuation 则再跑一轮；都无 → 退出，emit `AgentFinished{messages=本次 run 全部新消息}`。
4. **steering/followUp 队列**：`one-at-a-time` 默认（drain 取 1 条）；`all` 取全部。steering = 工具执行完后、下轮请求前注入；follow_up = 即将自然停止时注入。Agent 句柄提供 `steer()/follow_up()/clear_queues()`。

## 5. 工具执行管线（对齐 pi executeToolCalls）

```
ToolCall → 查注册表（未找到→错误 result "Tool {name} not found"）
→ before_tool_call（占位；blocked→错误 result）
→ 参数 JSON Schema 校验（jsonschema；失败→错误 result 含原因）
→ ToolCallStarted → 执行 tool.execute(id, params, cancel, on_update)
   （on_update → ToolCallFinished 前的 ToolCallUpdate 经 StreamEvent? 否——走 ToolCallUpdate 事件变体并入
     ToolCallStarted/Finished 之间以 ToolCallUpdate{partial} 形式发布）
→ 异常→错误 result（is_error=true，含文案与时长）→ after_tool_call（占位，字段级覆盖）
→ ToolCallFinished → ToolResultMessage
```

- 模式：Sequential（配置指定或任一被调工具 `execution_mode=Sequential`）逐个执行、可被取消打断；Parallel 默认——**预检串行**（逐个校验/发 Started），执行并发（tokio JoinSet），结果按 assistant 中的原始顺序回填。
- `terminate` 语义：本批全部工具结果 `terminate=true` → `has_more_tool_calls=false`（早停）。
- 取消检查点（AbortSignal 对应物）：Parallel 预检每个调用前、每个 thunk 启动前、after 钩子后；Sequential 每个调用之间；工具执行体内（token 传入 execute 与子进程/HTTP）。

## 6. 取消与运行生命周期（对齐 pi Agent.runWithLifecycle）

- `AgentHandle::prompt(text)` → 创建 run + `CancellationToken`；run 期间再调 prompt → Err（引导用 steer）；`abort()` 只触发 token，不 panic。
- run 结束：`is_streaming=false`，清 pending 工具集，`AgentFinished` 恒为 run 末事件。
- **中途失败合成消息**（对齐 pi handleRunFailure）：循环 panic 被 catch_unwind 拦截或致命错误时，合成 `stop_reason=Error/Aborted` 的 assistant 消息发 `MessageAdded`+`TurnFinished`+`AgentFinished`，保证订阅者/会话总能收到收尾（该合成消息不入上下文，仅通告——同 pi）。

## 7. 会话挂接（timo-session 唯一写点）

- `MessageAdded` 时同步写 entry（append-only）；`ToolCallStarted/Finished` 不另写 entry（pi 同构：toolResult 即 message entry）。
- `model/thinking` 变更（cli 切换）→ `model_change` / `thinking_level_change` entry（见设计 04）。
- 取消/错误不产生额外 entry（assistant 终态消息已含 stop_reason/error_message——pi 同构）。

## 8. 接缝自查（architecture/03 #2/#6/#7 落点）

- #2：本 crate 源码禁现 `if extension`/hook 名硬编码分支；新增订阅者无需改循环（编译期示例测试担保，见设计 07）。
- #6：流事件只有 `AssistantStreamEvent` 富枚举一条通路，无裸字符串流。
- #7：token 从 `prompt()` 参数一路到 `ProviderApi.stream(options.cancel)` 与 `tool.execute(..., cancel, ...)`；kill 后会话完好由 timo-session 担保（设计 04 §5）。
