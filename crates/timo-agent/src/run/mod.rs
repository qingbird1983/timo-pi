//! agent 循环（设计 02 §4~§5；对齐 pi agent-loop.ts runLoop）。
//!
//! 本文件不含任何扩展分支（接缝 #2）：工具经统一注册表，事件经总线，
//! steering/follow-up 经队列注入。并行工具批用 join_all（IO 并发），
//! 避免跨任务借用循环状态。

use std::sync::Arc;

use timo_ai::{AssistantMessage, AssistantStreamEvent, Content, Message, StopReason};
use timo_config::Clock;
use timo_session::SessionWriter;
use tokio_util::sync::CancellationToken;

use crate::events::{EventBus, KernelEvent};
use crate::hooks::{BeforeAgentStart, RouteRequest};
use crate::queue::PendingMessageQueue;

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
    /// 循环钩子（扩展宿主注入；None = 与 Phase 1 逐字节等价）。
    pub hooks: Option<std::sync::Arc<dyn crate::hooks::LoopHooks>>,
    /// 供应商钩子（透传到每次请求；None = 无钩子）。
    pub provider_hooks: Option<std::sync::Arc<dyn timo_ai::ProviderHooks>>,
    /// 工具执行前审阅器（None = 自动执行，Phase 1/2 行为）。
    pub review: Option<std::sync::Arc<dyn timo_tools::ToolReview>>,
}

/// 会话写入口（MessageAdded 落盘挂点；None = 无会话）。
pub type SessionSink = Option<Arc<tokio::sync::Mutex<SessionWriter>>>;

/// run 的通道组（事件 + 注入队列）。
pub struct RunChannels<'a> {
    /// 事件总线。
    pub bus: &'a EventBus,
    /// steering 队列。
    pub steering: &'a PendingMessageQueue,
    /// follow-up 队列。
    pub follow_up: &'a PendingMessageQueue,
}

/// 一次 run 的产出：本次新增消息与 run 结束时的完整上下文。
///
/// `new_messages` 与 [`crate::AgentHandle::run`] 的返回值一致；`context`
/// 是含 run 前历史的完整投影，宿主可原样带回下一次 run（Kernel 的 history）。
#[derive(Debug, Clone)]
pub struct RunOutcome {
    /// 本次 run 新增的消息（用户 prompt、assistant、工具结果、注入消息）。
    pub new_messages: Vec<Message>,
    /// run 结束时的完整上下文（含 run 前的历史）。
    pub context: Vec<Message>,
}

/// 运行一轮完整 agent run；返回新增消息与完整上下文。
///
/// 契约：`AgentStarted` 恒为首事件、`AgentFinished` 恒为末事件（含错误/取消路径）。
pub async fn run_agent_loop(
    config: &LoopConfig,
    channels: &RunChannels<'_>,
    history: Vec<Message>,
    prompt: Vec<Message>,
    session: SessionSink,
    cancel: CancellationToken,
) -> RunOutcome {
    let mut system_prompt = config.system_prompt.clone();
    if let Some(hooks) = &config.hooks
        && let Some(replaced) = hooks
            .before_agent_start(BeforeAgentStart {
                prompt: &prompt,
                system_prompt: &system_prompt,
            })
            .await
    {
        system_prompt = replaced;
    }
    channels.bus.emit(KernelEvent::AgentStarted);
    let mut ctx = Ctx {
        config,
        bus: channels.bus,
        cancel,
        session,
        messages: history,
        new_messages: Vec::new(),
        system_prompt: Some(system_prompt),
        model: config.model.clone(),
        thinking_level: config.thinking_level,
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
    RunOutcome {
        new_messages,
        context: std::mem::take(&mut ctx.messages),
    }
}

/// 循环内部状态。
struct Ctx<'a> {
    config: &'a LoopConfig,
    bus: &'a EventBus,
    cancel: CancellationToken,
    session: SessionSink,
    messages: Vec<Message>,
    new_messages: Vec<Message>,
    system_prompt: Option<String>,
    model: timo_ai::Model,
    thinking_level: timo_config::ThinkingLevel,
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
    async fn request(&mut self) -> AssistantMessage {
        let mut attempt: u32 = 0;
        loop {
            self.apply_route().await;
            self.bus.emit(KernelEvent::ProviderRequest {
                provider: self.model.provider.clone(),
                model: self.model.id.clone(),
            });
            let message = self.request_once().await;
            self.bus.emit(KernelEvent::ProviderResponse {
                provider: self.model.provider.clone(),
                model: self.model.id.clone(),
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

    /// 虚拟模型路由（每次请求前；D23 仅同 provider）。
    async fn apply_route(&mut self) {
        let Some(hooks) = self.config.hooks.clone() else {
            return;
        };
        let choice = hooks
            .route_model(RouteRequest {
                model: &self.model,
                messages: &self.messages,
            })
            .await;
        if let Some(choice) = choice {
            self.model = choice.model;
            if let Some(level) = choice.thinking_level {
                self.thinking_level = level;
            }
        }
    }

    async fn request_once(&mut self) -> AssistantMessage {
        let mut messages = self.messages.clone();
        if let Some(hooks) = self.config.hooks.clone() {
            if let Some(next) = hooks.transform_context(messages.clone()).await {
                messages = next;
            }
            let backup = messages.clone();
            let transformed = hooks
                .transform_full_context(self.system_prompt.clone(), messages)
                .await;
            messages = match transformed {
                Some((system, next)) => {
                    self.system_prompt = system;
                    next
                }
                None => backup,
            };
        }
        let api_key = match self.config.auth.resolve().await {
            Ok(k) => k,
            Err(e) => return self.error_terminal(&e.to_string(), false),
        };
        let ctx = timo_ai::Context {
            system_prompt: self.system_prompt.clone(),
            messages,
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
            thinking_level: Some(self.thinking_level),
        };
        let options = timo_ai::StreamOptions {
            api_key: Some(api_key),
            cancel: self.cancel.clone(),
            max_retries: 0,
            provider_hooks: self.config.provider_hooks.clone(),
            ..timo_ai::StreamOptions::default()
        };
        let mut stream = self.config.api.stream(&self.model, &ctx, options).await;
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

    fn error_terminal(&self, reason: &str, cancelled: bool) -> AssistantMessage {
        let mut m = AssistantMessage::pending(
            self.model.api.as_str(),
            &self.model.provider,
            &self.model.id,
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
}

mod tools;
