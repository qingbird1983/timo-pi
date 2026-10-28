//! 嵌套接缝测试夹具（`nested.rs` / `nested_limits.rs` 共用）。
#![allow(dead_code)] // dead-code: 共用测试夹具，两个测试目标各取所需

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};
use timo_agent::{
    AgentHandle, BeforeToolCall, LoopConfig, LoopHooks, ToolOutcome, ToolOutputPatch,
};
use timo_ai::{
    AssistantMessage, AssistantStreamEvent, Content, ProviderApi, StopReason, StreamOptions,
};
use timo_config::{FixedClock, RetrySettings, ThinkingLevel};
use timo_tools::{
    ExecutionMode, Exposure, NestedCall, ToolCallCtx, ToolDefinition, ToolOutput, ToolRegistry,
};

pub(crate) struct ScriptedApi {
    script: Mutex<Vec<Vec<AssistantStreamEvent>>>,
}

#[async_trait]
impl ProviderApi for ScriptedApi {
    async fn stream(
        &self,
        _model: &timo_ai::Model,
        _context: &timo_ai::Context,
        options: StreamOptions,
    ) -> timo_ai::AssistantMessageStream {
        let events = {
            let mut queue = self.script.lock().unwrap();
            if queue.is_empty() {
                vec![]
            } else {
                queue.remove(0)
            }
        };
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        tokio::spawn(async move {
            for event in events {
                if options.cancel.is_cancelled() {
                    break;
                }
                if tx.send(event).await.is_err() {
                    return;
                }
            }
        });
        timo_ai::AssistantMessageStream::new(rx)
    }
}

pub(crate) fn scripted(events: Vec<Vec<AssistantStreamEvent>>) -> Arc<dyn ProviderApi> {
    Arc::new(ScriptedApi {
        script: Mutex::new(events),
    })
}

pub(crate) fn base_model() -> timo_ai::Model {
    serde_json::from_value(json!({
        "id": "m", "name": "m", "provider": "mock", "api": "anthropic-messages",
        "base_url": "http://mock", "reasoning": false, "input": ["text"], "cost": {},
        "context_window": 1000, "max_tokens": 100
    }))
    .unwrap()
}

pub(crate) fn tool_call_done(id: &str, tool: &str) -> Vec<AssistantStreamEvent> {
    let mut message = AssistantMessage::pending("mock", "mock", "m", 0);
    message.content = vec![Content::ToolCall {
        id: id.into(),
        name: tool.into(),
        arguments: json!({}),
        arguments_error: None,
    }];
    message.stop_reason = StopReason::ToolUse;
    vec![AssistantStreamEvent::Done {
        reason: StopReason::ToolUse,
        message,
    }]
}

pub(crate) fn text_done(text: &str) -> Vec<AssistantStreamEvent> {
    let mut message = AssistantMessage::pending("mock", "mock", "m", 0);
    message.content = vec![Content::Text { text: text.into() }];
    message.stop_reason = StopReason::Stop;
    vec![AssistantStreamEvent::Done {
        reason: StopReason::Stop,
        message,
    }]
}

pub(crate) type ExecFuture = Pin<Box<dyn Future<Output = timo_tools::Result<ToolOutput>> + Send>>;
pub(crate) type ProbeFuture = Pin<Box<dyn Future<Output = ToolOutput> + Send>>;

pub(crate) fn definition(
    name: &str,
    exposure: Exposure,
    execution_mode: ExecutionMode,
    execute: impl Fn(ToolCallCtx) -> ExecFuture + Send + Sync + 'static,
) -> ToolDefinition {
    ToolDefinition {
        name: name.into(),
        label: name.into(),
        description: format!("{name} tool"),
        prompt_snippet: None,
        prompt_guidelines: vec![],
        parameters: json!({"type": "object"}),
        output_schema: None,
        exposure,
        namespace: None,
        annotations: timo_tools::Annotations::default(),
        execution_mode,
        execute: Arc::new(execute),
    }
}

pub(crate) fn call(ctx: &ToolCallCtx, name: &str, arguments: Value) -> NestedCall {
    NestedCall {
        parent_id: ctx.tool_call_id.clone(),
        name: name.into(),
        arguments,
        cancel: ctx.cancel.clone(),
        updates: ctx.updates.clone(),
    }
}

/// 探针返回文本（不把嵌套结果的 usage 直接带出，避免与汇总重复计数）。
pub(crate) fn text_of(output: ToolOutput) -> ToolOutput {
    let text = timo_tools::output_text(&output);
    ToolOutput {
        is_error: output.is_error,
        ..ToolOutput::text(text)
    }
}

pub(crate) fn probe(
    name: &str,
    body: impl Fn(ToolCallCtx) -> ProbeFuture + Send + Sync + 'static,
) -> ToolDefinition {
    let body = Arc::new(body);
    definition(
        name,
        Exposure::Direct,
        ExecutionMode::Parallel,
        move |ctx| {
            let body = Arc::clone(&body);
            Box::pin(async move { Ok((*body)(ctx).await) })
        },
    )
}

/// 连续调用同一工具两次，返回两段文本以 `|` 连接。
pub(crate) fn probe_two(name: &'static str, tool: &'static str) -> ToolDefinition {
    probe(name, move |ctx| {
        Box::pin(async move {
            let nested = ctx.nested.clone().unwrap();
            let first = nested.execute(call(&ctx, tool, json!({}))).await.output;
            let second = nested.execute(call(&ctx, tool, json!({}))).await.output;
            ToolOutput::text(format!(
                "{}|{}",
                timo_tools::output_text(&first),
                timo_tools::output_text(&second)
            ))
        })
    })
}

pub(crate) fn handle(
    api: Arc<dyn ProviderApi>,
    tools: ToolRegistry,
    hooks: Option<Arc<dyn LoopHooks>>,
    review: Option<Arc<dyn timo_tools::ToolReview>>,
) -> AgentHandle {
    AgentHandle::new(LoopConfig {
        model: base_model(),
        api,
        tools: Arc::new(tools),
        thinking_level: ThinkingLevel::Off,
        auth: Arc::new(timo_ai::AuthResolver {
            explicit: Some("k".into()),
            provider_ref: None,
            env_names: vec![],
        }),
        retry: RetrySettings::default(),
        cwd: Arc::from(std::path::Path::new(".")),
        system_prompt: "test".into(),
        clock: Arc::new(FixedClock::new(1_000)),
        max_turns: None,
        compaction: None,
        hooks,
        provider_hooks: None,
        review,
        dispatch: None,
        route_state: Default::default(),
    })
}

#[derive(Default)]
pub(crate) struct TestHooks {
    pub(crate) seen: Mutex<Vec<String>>,
    pub(crate) block: Option<&'static str>,
    pub(crate) prefix_target: bool,
}

impl TestHooks {
    pub(crate) fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait]
impl LoopHooks for TestHooks {
    async fn before_tool_call(&self, call: &Content) -> Option<BeforeToolCall> {
        let name = match call {
            Content::ToolCall { name, .. } => name.clone(),
            _ => String::new(),
        };
        self.seen.lock().unwrap().push(name.clone());
        (self.block == Some(name.as_str())).then(|| BeforeToolCall {
            block: Some("nope".into()),
            ..BeforeToolCall::default()
        })
    }

    async fn after_tool_call(&self, outcome: &ToolOutcome<'_>) -> Option<ToolOutputPatch> {
        if !self.prefix_target || outcome.tool_name != "target" {
            return None;
        }
        let text = timo_tools::output_text(outcome.output);
        Some(ToolOutputPatch {
            content: Some(vec![Content::Text {
                text: format!("hook:{text}"),
            }]),
            ..ToolOutputPatch::default()
        })
    }
}

pub(crate) struct AllowingReview;

#[async_trait]
impl timo_tools::ToolReview for AllowingReview {
    async fn review(&self, _: timo_tools::ToolReviewRequest) -> timo_tools::ReviewDecision {
        timo_tools::ReviewDecision::Allow
    }
}

pub(crate) fn target_definition() -> ToolDefinition {
    definition("target", Exposure::Direct, ExecutionMode::Parallel, |ctx| {
        Box::pin(async move {
            Ok(ToolOutput::text(format!(
                "id={} review={} nested={} k={}",
                ctx.tool_call_id,
                if ctx.review.is_some() { "yes" } else { "no" },
                if ctx.nested.is_some() { "yes" } else { "no" },
                ctx.params.get("k").and_then(Value::as_str).unwrap_or("-"),
            )))
        })
    })
}
