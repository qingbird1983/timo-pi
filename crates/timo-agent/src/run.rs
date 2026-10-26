//! agent 循环（设计 02 §4~§5；对齐 pi agent-loop.ts runLoop）。
//!
//! 本文件不含任何扩展分支（接缝 #2）：工具经统一注册表，事件经总线，
//! steering/follow-up 经队列注入。并行工具批用 join_all（IO 并发），
//! 避免跨任务借用循环状态。

use std::sync::Arc;

use timo_ai::{
    AssistantMessage, AssistantStreamEvent, Content, Message, StopReason, ToolResultMessage,
};
use timo_config::Clock;
use timo_session::SessionWriter;
use tokio_util::sync::CancellationToken;

use crate::events::{EventBus, KernelEvent};
use crate::queue::PendingMessageQueue;
use crate::tool_calls::{error_result_message, tool_args, tool_id, tool_name};

/// 工具批执行模式（pi toolExecution）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolExecutionMode {
    /// 并发执行（IO 并发），结果按原始顺序回填（默认）。
    #[default]
    Parallel,
    /// 逐个执行、可被取消打断。
    Sequential,
}

/// 一次 run 的循环配置（装配期定死；Phase 2 钩子参数在此扩充）。
pub struct LoopConfig {
    /// 本 run 使用的模型。
    pub model: timo_ai::Model,
    /// 协议适配器。
    pub api: std::sync::Arc<dyn timo_ai::ProviderApi>,
    /// 工具注册表（内置 + MCP 同表）。
    pub tools: std::sync::Arc<timo_tools::ToolRegistry>,
    /// 思考档位。
    pub thinking_level: timo_config::ThinkingLevel,
    /// 认证解析器（每次请求重解析，pi getApiKey 同构）。
    pub auth: std::sync::Arc<timo_ai::AuthResolver>,
    /// 工具批模式。
    pub tool_execution: ToolExecutionMode,
    /// assistant 轮次重试设置（timo-config settings.retry）。
    pub retry: timo_config::RetrySettings,
    /// 工作目录（工具 cwd）。
    pub cwd: std::sync::Arc<std::path::Path>,
    /// 系统提示（循环组装）。
    pub system_prompt: String,
    /// 时钟（standards/05 §3：时间注入）。
    pub clock: std::sync::Arc<dyn Clock>,
    /// 单次 run 的模型请求轮数上限（None = 不限）。
    pub max_turns: Option<usize>,
}

/// 会话写入口（MessageAdded 落盘挂点；None = 无会话）。
pub type SessionSink = Option<Arc<tokio::sync::Mutex<SessionWriter>>>;

/// 运行一轮完整 agent run；返回本次 run 产生的全部新消息。
///
/// 契约：`AgentStarted` 恒为首事件、`AgentFinished` 恒为末事件（含错误/取消路径）。
/// run 的通道组（事件 + 注入队列）。
pub struct RunChannels<'a> {
    /// 事件总线。
    pub bus: &'a EventBus,
    /// steering 队列。
    pub steering: &'a PendingMessageQueue,
    /// follow-up 队列。
    pub follow_up: &'a PendingMessageQueue,
}

/// 运行一轮完整 agent run；返回本次 run 产生的全部新消息。
///
/// 契约：`AgentStarted` 恒为首事件、`AgentFinished` 恒为末事件（含错误/取消路径）。
pub async fn run_agent_loop(
    config: &LoopConfig,
    channels: &RunChannels<'_>,
    history: Vec<Message>,
    prompt: Vec<Message>,
    session: SessionSink,
    cancel: CancellationToken,
) -> Vec<Message> {
    channels.bus.emit(KernelEvent::AgentStarted);
    let mut ctx = Ctx {
        config,
        bus: channels.bus,
        cancel,
        session,
        messages: history,
        new_messages: Vec::new(),
    };
    for m in prompt {
        ctx.push_message(m).await;
    }
    let mut pending = channels.steering.drain();
    let mut turns: usize = 0;
    'outer: loop {
        // 内层：轮次循环
        loop {
            if ctx.cancel.is_cancelled() && pending.is_empty() {
                break;
            }
            if let Some(max) = config.max_turns
                && turns >= max
            {
                break 'outer;
            }
            turns += 1;
            ctx.bus.emit(KernelEvent::TurnStarted);
            for m in std::mem::take(&mut pending) {
                ctx.push_message(m).await;
            }
            let message = ctx.request().await;
            // assistant 终态消息入上下文（对齐 pi message_end 语义）
            ctx.push_message(Message::Assistant(message.clone())).await;
            if matches!(message.stop_reason, StopReason::Error | StopReason::Aborted) {
                ctx.bus.emit(KernelEvent::TurnFinished {
                    message: Arc::new(Message::Assistant(message.clone())),
                    tool_results: Arc::new(vec![]),
                });
                break 'outer; // 硬退出（pi 同构）
            }
            let calls: Vec<Content> = message
                .content
                .iter()
                .filter(|c| matches!(c, Content::ToolCall { .. }))
                .cloned()
                .collect();
            let mut results: Vec<Message> = vec![];
            let mut terminated_all = true;
            if !calls.is_empty() {
                if message.stop_reason == StopReason::Length {
                    // 截断消息中的工具调用一律不执行（pi failToolCallsFromTruncatedMessage）
                    for call in &calls {
                        results.push(ctx.fail_tool_call(call, "…output token limit…").await);
                    }
                    terminated_all = false;
                } else {
                    let (rs, all_term) = ctx.execute_tools(&calls).await;
                    results = rs;
                    terminated_all = all_term;
                }
                for r in results.clone() {
                    ctx.push_message(r).await;
                }
            }
            let has_more = !results.is_empty() && !terminated_all;
            ctx.bus.emit(KernelEvent::TurnFinished {
                message: Arc::new(Message::Assistant(message)),
                tool_results: Arc::new(results),
            });
            // pi 条件：hasMoreToolCalls || pending 非空 → 继续（下一轮请求）
            pending = channels.steering.drain();
            if !has_more && pending.is_empty() {
                break;
            }
        }
        if ctx.cancel.is_cancelled() {
            break;
        }
        let follow = channels.follow_up.drain();
        if follow.is_empty() {
            break;
        }
        pending = follow;
    }
    let new_messages = std::mem::take(&mut ctx.new_messages);
    ctx.bus.emit(KernelEvent::AgentFinished {
        messages: Arc::new(new_messages.clone()),
    });
    new_messages
}

/// 循环内部状态。
struct Ctx<'a> {
    config: &'a LoopConfig,
    bus: &'a EventBus,
    cancel: CancellationToken,
    session: SessionSink,
    messages: Vec<Message>,
    new_messages: Vec<Message>,
}

impl<'a> Ctx<'a> {
    /// 追加消息：入上下文 + `MessageAdded` + 会话落盘。
    async fn push_message(&mut self, message: Message) {
        self.write_session(&message).await;
        self.bus.emit(KernelEvent::MessageAdded {
            message: Arc::new(message.clone()),
        });
        self.messages.push(message.clone());
        self.new_messages.push(message);
    }

    async fn write_session(&self, message: &Message) {
        if let Some(sink) = &self.session
            && let Ok(payload) = serde_json::to_value(message)
        {
            let mut w = sink.lock().await;
            let _ = w.append_raw(timo_session::kinds::MESSAGE, payload);
        }
    }

    /// 一次模型请求（含 assistant 轮次重试；设计 02 §4.c/d）。
    async fn request(&self) -> AssistantMessage {
        let mut attempt: u32 = 0;
        loop {
            self.bus.emit(KernelEvent::ProviderRequest {
                provider: self.config.model.provider.clone(),
                model: self.config.model.id.clone(),
            });
            let message = self.request_once().await;
            self.bus.emit(KernelEvent::ProviderResponse {
                provider: self.config.model.provider.clone(),
                model: self.config.model.id.clone(),
                usage: message.usage.clone(),
            });
            let retryable = message.stop_reason == StopReason::Error
                && self.config.retry.enabled
                && attempt < self.config.retry.max_retries
                && message
                    .error_message
                    .as_deref()
                    .map(timo_ai::retry::assistant_error_retryable)
                    .unwrap_or(false);
            if !retryable {
                return message;
            }
            attempt += 1;
            let delay = timo_ai::retry::assistant_backoff(
                attempt,
                self.config.retry.base_delay_ms,
                self.config.retry.max_retry_delay_ms,
            );
            tokio::select! {
                _ = self.cancel.cancelled() => return message,
                _ = tokio::time::sleep(delay) => {}
            }
        }
    }

    async fn request_once(&self) -> AssistantMessage {
        let api_key = match self.config.auth.resolve().await {
            Ok(k) => k,
            Err(e) => return self.error_terminal(&e.to_string(), false),
        };
        let ctx = timo_ai::Context {
            system_prompt: Some(self.system_prompt_value()),
            messages: self.messages.clone(),
            tools: self
                .config
                .tools
                .declared()
                .into_iter()
                .map(|d| timo_ai::Tool {
                    name: d.name.clone(),
                    description: d.description.clone(),
                    parameters: d.parameters.clone(),
                })
                .collect(),
            thinking_level: Some(self.config.thinking_level),
        };
        let options = timo_ai::StreamOptions {
            api_key: Some(api_key),
            cancel: self.cancel.clone(),
            max_retries: 0,
            ..timo_ai::StreamOptions::default()
        };
        let mut stream = self
            .config
            .api
            .stream(&self.config.model, &ctx, options)
            .await;
        let mut terminal: Option<AssistantMessage> = None;
        while let Some(ev) = stream.next_event().await {
            match &ev {
                AssistantStreamEvent::Done { message, .. }
                | AssistantStreamEvent::Error { message } => terminal = Some(message.clone()),
                _ => {}
            }
            self.bus.emit(KernelEvent::StreamEvent {
                event: Arc::new(ev),
            });
        }
        terminal.unwrap_or_else(|| {
            self.error_terminal(
                "stream ended without terminal event",
                self.cancel.is_cancelled(),
            )
        })
    }

    fn system_prompt_value(&self) -> String {
        self.config.system_prompt.clone()
    }

    fn error_terminal(&self, reason: &str, cancelled: bool) -> AssistantMessage {
        let mut m = AssistantMessage::pending(
            self.config.model.api.as_str(),
            &self.config.model.provider,
            &self.config.model.id,
            self.now_ms(),
        );
        m.stop_reason = if cancelled {
            StopReason::Aborted
        } else {
            StopReason::Error
        };
        m.error_message = Some(reason.to_string());
        m
    }

    fn now_ms(&self) -> i64 {
        self.config.clock.now_ms()
    }

    /// 工具批执行（设计 02 §5）；返回（结果消息, 是否全部 terminate）。
    async fn execute_tools(&self, calls: &[Content]) -> (Vec<Message>, bool) {
        match self.config.tool_execution {
            ToolExecutionMode::Sequential => self.execute_tools_sequential(calls).await,
            ToolExecutionMode::Parallel => self.execute_tools_parallel(calls).await,
        }
    }

    async fn execute_tools_sequential(&self, calls: &[Content]) -> (Vec<Message>, bool) {
        let mut out = vec![];
        let mut all_term = true;
        for call in calls {
            if self.cancel.is_cancelled() {
                out.push(self.fail_tool_call(call, "Operation aborted").await);
                all_term = false;
                continue;
            }
            let (m, term) = self.run_one(call).await;
            all_term &= term;
            out.push(m);
        }
        let non_empty = !out.is_empty();
        (out, all_term && non_empty)
    }

    async fn execute_tools_parallel(&self, calls: &[Content]) -> (Vec<Message>, bool) {
        // 并发执行（join_all 轮询并发，IO 语义）；结果按原始顺序回填（pi 同构）
        let futures = calls.iter().map(|call| async {
            if self.cancel.is_cancelled() {
                return (self.fail_tool_call(call, "Operation aborted").await, false);
            }
            self.run_one(call).await
        });
        let results = futures::future::join_all(futures).await;
        let all_term = results.iter().all(|(_, t)| *t);
        let non_empty = !results.is_empty();
        (
            results.into_iter().map(|(m, _)| m).collect(),
            all_term && non_empty,
        )
    }

    /// 单工具全流程：Requested → Started → 执行 → Finished；Err 转错误 result（is_error）。
    async fn run_one(&self, call: &Content) -> (Message, bool) {
        let started = std::time::Instant::now();
        let name = tool_name(call);
        let id = tool_id(call);
        self.bus.emit(KernelEvent::ToolCallRequested {
            call: Arc::new(call.clone()),
        });
        self.bus.emit(KernelEvent::ToolCallStarted {
            tool_call_id: id.clone(),
            tool_name: name.clone(),
            args: tool_args(call),
        });
        let result = self.execute_validated(call).await;
        let duration = started.elapsed().as_millis() as u64;
        match result {
            Ok(output) => {
                let message = Message::ToolResult(ToolResultMessage {
                    tool_call_id: id.clone(),
                    tool_name: name.clone(),
                    content: output.content,
                    is_error: output.is_error,
                    usage: None,
                    duration_ms: Some(duration),
                    timestamp: self.now_ms(),
                });
                self.bus.emit(KernelEvent::ToolCallFinished {
                    tool_call_id: id.clone(),
                    tool_name: name.clone(),
                    result: Arc::new(message.clone()),
                    duration_ms: Some(duration),
                });
                (message, output.terminate)
            }
            Err(e) => {
                let message =
                    error_result_message(&id, &name, &e.to_string(), Some(duration), self.now_ms());
                self.bus.emit(KernelEvent::ToolCallFinished {
                    tool_call_id: id.clone(),
                    tool_name: name.clone(),
                    result: Arc::new(message.clone()),
                    duration_ms: Some(duration),
                });
                (message, false)
            }
        }
    }

    async fn execute_validated(
        &self,
        call: &Content,
    ) -> timo_tools::Result<timo_tools::ToolOutput> {
        let Content::ToolCall {
            name,
            arguments,
            arguments_error,
            ..
        } = call
        else {
            return Err(timo_tools::ToolError::InvalidParams {
                reason: "not a tool call".into(),
            });
        };
        if let Some(err) = arguments_error {
            return Err(timo_tools::ToolError::InvalidParams {
                reason: format!("arguments are not valid JSON: {err}"),
            });
        }
        let (tx, mut rx) = tokio::sync::mpsc::channel::<timo_tools::ToolOutput>(16);
        let bus = self.bus.clone();
        let id = tool_id(call);
        let (fid, ftool) = (id.clone(), name.clone());
        let forwarder = tokio::spawn(async move {
            while let Some(partial) = rx.recv().await {
                bus.emit(KernelEvent::ToolCallUpdate {
                    tool_call_id: fid.clone(),
                    tool_name: ftool.clone(),
                    partial: Arc::new(partial),
                });
            }
        });
        let ctx = timo_tools::ToolCallCtx {
            tool_call_id: id.clone(),
            params: arguments.clone(),
            cancel: self.cancel.clone(),
            updates: tx,
            cwd: self.config.cwd.clone(),
        };
        let out = self.config.tools.execute(name, ctx).await;
        forwarder.abort();
        out
    }

    /// 截断批中失败的工具调用（pi failToolCallsFromTruncatedMessage）。
    async fn fail_tool_call(&self, call: &Content, reason: &str) -> Message {
        let name = tool_name(call);
        let id = tool_id(call);
        self.bus.emit(KernelEvent::ToolCallRequested {
            call: Arc::new(call.clone()),
        });
        self.bus.emit(KernelEvent::ToolCallStarted {
            tool_call_id: id.clone(),
            tool_name: name.clone(),
            args: tool_args(call),
        });
        let message = error_result_message(&id, &name, reason, None, self.now_ms());
        self.bus.emit(KernelEvent::ToolCallFinished {
            tool_call_id: id.clone(),
            tool_name: name.clone(),
            result: Arc::new(message.clone()),
            duration_ms: None,
        });
        message
    }
}
