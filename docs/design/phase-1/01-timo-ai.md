# 设计 01：timo-ai —— 协议层与模型目录

- 状态：定稿
- 日期：2026-10-26
- 关联：architecture/02 §1~2、03 接缝 #3/#6；ADR-0003/0005；pi: packages/ai/src

## 1. 模块布局

```
crates/timo-ai/src/
  types.rs          Message/Content/Usage/StopReason/ThinkingLevel/Context
  stream.rs         AssistantStreamEvent + AssistantMessageStream + StreamOptions
  api/mod.rs        ProviderApi trait + api 工厂（按 Model::api 分发）
  api/sse.rs        SSE 解码器（自研，见 00 §5 D2）
  api/anthropic/    mod.rs（编排）request.rs（构造请求）convert.rs（消息↔协议）stream.rs（SSE→事件）
  api/openai/       同上四件（openai-completions 语义）
  catalog.rs        模型目录装载（assets/models/*.json）
  auth.rs           AuthMethod 枚举 + 解析（api_key/$ENV/!cmd/env 变量表）
  retry.rs          两层重试（传输层 + assistant 轮次层）
  errors.rs         AiError
  clock.rs          Clock trait（可注入）
assets/models/      anthropic.json、openai.json（pi 目录子集，见 §6）
```

## 2. 核心类型（types.rs）

```rust
pub enum Message {
    System(SystemMessage), User(UserMessage),
    Assistant(AssistantMessage), ToolResult(ToolResultMessage),
}   // serde tag = "role"；时间戳 i64（unix ms）
pub struct SystemMessage { pub content: String, pub timestamp: i64 }
// pi 的 sections/toolsAdded/toolsRemoved 是扩展期机制，Phase 1 不用；结构 Phase 2 增补（只加不改）
pub struct UserMessage { pub content: Vec<Content>, pub timestamp: i64 }
pub struct AssistantMessage {
    pub content: Vec<Content>,           // Text | Thinking | ToolCall
    pub api: ApiKind, pub provider: String, pub model: String,
    pub response_model: Option<String>, pub response_id: Option<String>,
    pub usage: Usage,                    // 必填，零值初始化
    pub stop_reason: StopReason, pub raw_stop_reason: Option<String>,
    pub error_message: Option<String>,
    pub thinking_level: Option<ThinkingLevel>,
    pub duration_ms: Option<u64>,
    pub timestamp: i64,
}
pub struct ToolResultMessage {
    pub tool_call_id: String, pub tool_name: String,
    pub content: Vec<Content>, pub is_error: bool,
    pub usage: Option<Usage>, pub duration_ms: Option<u64>, pub timestamp: i64,
}
pub enum Content {
    Text { text: String },
    Thinking { thinking: String, signature: Option<String>, redacted: bool },
    Image { data: String /*base64*/, mime_type: String },
    ToolCall { id: String, name: String, arguments: serde_json::Value,
               arguments_error: Option<String> },   // 解析失败: arguments={}, error 记原因
}
pub enum StopReason { Pending, Stop, Length, ToolUse, Error, Aborted, Deferred }
pub enum ThinkingLevel { Off, Minimal, Low, Medium, High, Xhigh, Max }
pub struct Usage { input, output, cache_read, cache_write, reasoning: Option<u64>,
                   cache_write_1h: Option<u64>, total_tokens: u64, cost: Cost }
pub struct Context { pub system_prompt: Option<String>, pub messages: Vec<Message>,
                     pub tools: Vec<Tool> }   // 发送前折叠为 TranscriptContext（仅 messages）
pub struct Tool { pub name: String, pub description: String, pub parameters: Value }  // JSON Schema
```

不变量（对齐 pi，验收断言）：`usage.input` 不含 cache_read/cache_write；`total_tokens` = 四项之和；`reasoning` ⊆ `output`。

## 3. 协议 trait 与流事件（stream.rs / api/mod.rs）

```rust
#[async_trait::async_trait]
pub trait ProviderApi: Send + Sync {
    async fn stream(&self, model: &Model, context: TranscriptContext,
                    options: StreamOptions) -> AssistantMessageStream;
}
pub struct AssistantMessageStream { /* mpsc(256) 接收端 */ }
impl AssistantMessageStream {
    pub async fn next(&mut self) -> Option<AssistantStreamEvent>;
    pub async fn result(self) -> AssistantMessage;   // 取终态消息（done/error 携带）
}
pub enum AssistantStreamEvent {          // 接缝 #6 富枚举
    Start { partial: AssistantMessage },
    BlockStart { index: usize, partial: AssistantMessage },       // text/thinking/toolcall 共用
    Delta { index: usize, delta: String, partial: AssistantMessage },
    BlockEnd { index: usize, partial: AssistantMessage },          // toolcall 含完整 ToolCall
    Done { reason: StopReason, message: AssistantMessage },
    Error { message: AssistantMessage },   // stop_reason ∈ {Error, Aborted} + error_message
}
pub struct StreamOptions { api_key: Option<String>, cancel: CancellationToken,
    max_tokens: Option<u64>, temperature: Option<f64>, max_retries: u32 /*默认0*/,
    max_retry_delay_ms: u64 /*默认60_000*/, base_url: Option<String>, headers: Vec<(String,String)> }
```

- 契约（对齐 pi）：`Start` 恒为首事件；终事件恰一个（Done/Error）；**适配器不得 panic/返回 Err**——一切失败编码为 Error 事件（异常仅在缺 api-key 时同步抛出 `AiError::NoApiKey`）。
- 块类型由 `partial.content[index]` 判别（Text→文本增量 / Thinking→思考增量 / ToolCall→参数片段）；toolcall 的 live 参数解析记 backlog（D10，Phase 1 终态解析）。

## 4. anthropic-messages（pi: api/anthropic-messages.ts）

- 端点：`{base_url}/v1/messages`，`stream: true`；鉴权 `x-api-key`（OAuth Bearer 占位）。
- 请求体：`model / messages / max_tokens(默认 model.max_tokens) / system(文本块数组，末块 cache_control=ephemeral) / tools[{name, description, input_schema}]（末工具 cache_control）/ tool_choice(默认省略) / thinking（level≠Off 时 {type:"enabled", budget_tokens:1024·档位系数}）/ temperature（仅 thinking 关闭时发送）`。
- 消息转换：system→`system`；user→text/image 块（纯文本折叠为字符串）；assistant→text / thinking(带 signature) / redacted_thinking / tool_use；**连续 tool_result 合并为一条 user 消息的 tool_result 块**（is_error 映射）。
- SSE 映射：`message_start`→播种 usage/response_id；`content_block_start/delta/stop`→BlockStart/Delta/BlockEnd（`input_json_delta` 原文直透 Delta，`content_block_stop` 时整体 JSON 解析，失败→arguments_error）；`message_delta`→stop_reason 映射（`end_turn→Stop, max_tokens→Length, tool_use→ToolUse, refusal/sensitive→Error, pause_turn/stop_sequence→Stop`，未知→Error）+ usage 增量（非空才覆盖）；`message_stop`→收尾校验（未收到→Error "stream ended before message_stop"）。
- 工具 id 规范化：`[^A-Za-z0-9_-]→_`，截 64。

## 5. openai-completions（pi: api/openai-completions.ts）

- 端点：`{base_url}/chat/completions`，`stream_options:{include_usage:true}`；鉴权 `Authorization: Bearer`。
- 请求体：`model / messages / tools[{type:"function", function:{name, description, parameters(原样透传)}}] / tool_choice / temperature / max_tokens / reasoning_effort（thinking 模型，Off→省略）`；`stream:true`。
- 消息转换：system→system；user→content parts（text/image_url data URI）；assistant→content 字符串 + `tool_calls[{id,type:"function",function:{name,arguments:JSON.stringify}}]`；tool_result→`role:"tool"` + `tool_call_id`。
- 流映射：`delta.content`→文本 Delta；`delta.reasoning_content|reasoning|reasoning_text`（首个非空）→思考 Delta（signature 记字段名）；`delta.tool_calls[{index,id,function:{name,arguments}}]`→按 index 聚合 ToolCall（新 index→BlockStart，片段→Delta，流末统一 BlockEnd）；`usage`：`prompt_tokens_details.cached_tokens→cache_read`，`input = max(0, prompt - cached - cache_write)`，`completion_tokens_details.reasoning_tokens→reasoning`。
- `finish_reason` 映射：`stop/end→Stop, length→Length, tool_calls/function_call→ToolUse, 其余（含 content_filter/network_error）→Error("Provider finish_reason: X")`；未收到 finish_reason→Error（除非 compat 关闭校验→有工具调用推 ToolUse 否则 Stop）。

## 6. 模型目录与供应商（catalog.rs）

- `Model { id, name, api: ApiKind, provider, base_url, reasoning: bool, thinking_level_map: Option<Map>,
          input: Vec<"text"|"image">, cost: CostRates{input,output,cache_read,cache_write /*USD/Mtok*/, tiers},
          context_window: u64, max_tokens: u64, headers: Vec<(String,String)>, compat: Compat }`
- `ApiKind::{AnthropicMessages, OpenaiCompletions}`（接缝 #3：新协议 = 新实现文件 + 目录数据，不触碰循环）。
- 目录数据：`assets/models/{anthropic,openai}.json`，扁平 `model_id → Model`（数据格式即 pi Model 子集；**机制对齐**：目录即纯数据；pi 的 models.dev 生成管线接入记 backlog）。子集：anthropic（claude 现役 ≥4 型）+ openai（gpt 现役 ≥4 型），字段照抄 pi 目录值。
- 内置 provider：`anthropic`（env：`ANTHROPIC_API_KEY`）、`openai`（env：`OPENAI_API_KEY`）；用户可经 models.json 增补（见设计 05）。

## 7. 认证（auth.rs）

```rust
pub enum AuthMethod { ApiKey { value_ref: ValueRef }, EnvVar { name: String },
                      Command { cmd: String }, OAuth { /* Phase 5+，占位不实现 */ } }
pub enum ValueRef { Literal(String), Env(String /*$NAME*/), Command(String /*!cmd*/) }
```
解析优先级（对齐 pi）：调用方显式 options.api_key > models.json provider.api_key（Literal/$ENV/!cmd 三形态）> 环境变量表。失败→`AiError::NoApiKey`（同步抛，不进流）。密钥本体不落日志/事件/会话。

## 8. 重试（retry.rs，两层）

1. **传输层**（仅未开始的初始请求；流一旦开始不重试）：可重试 = `x-should-retry: true`、无状态码（网络错误）、408/409/429/5xx；延时 `retry-after-ms` → `retry-after` → `min(0.5·2^n, 8s)`·(1−rand·0.25)；服务端要求延时 > `max_retry_delay_ms` → 立即失败。
2. **assistant 轮次层**（agent 循环调用，见设计 02）：对 `stop_reason=Error` 的终态消息，错误文案匹配可重试模式（overloaded / rate limit / 429|5xx 词元 / network / timeout / "stream ended before message_stop"…）且不匹配配额类（insufficient_quota、月度限额…）时，按 `base_delay_ms·2^(attempt-1)` 上限 `max_retry_delay_ms` 重试；Aborted 永不重试。
默认值对齐 pi settings：`{enabled:true, max_retries:3, base_delay_ms:2000}`。

## 9. 错误类型表（errors.rs）

| 变体 | 触发 | 去向 |
|---|---|---|
| NoApiKey{provider} | 认证解析失败 | 同步返回调用方（cli 打印） |
| Http{status, body} | 非 2xx（重试穷尽后） | Error 事件 → 会话 assistant errorMessage |
| Sse{reason} | 流提前结束/未知事件/未知 stop_reason | 同上 |
| Json{context} | 响应体/工具参数解析失败 | 同上（工具参数→arguments_error） |
| Transport{source} | reqwest/IO 错误 | 同上（可重试判定输入） |
| Unsupported{feature} | 调用到 Phase 1 未实现面（如 OAuth） | 同步返回 |
