//! LoopHooks 集成测试（设计 04 §2.1/§3）：系统提示、上下文、路由与工具裁决。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};
use timo_agent::{
    AgentHandle, BeforeAgentStart, BeforeToolCall, LoopConfig, LoopHooks, RouteChoice,
    RouteRequest, ToolOutcome, ToolOutputPatch,
};
use timo_ai::{
    AssistantMessage, AssistantStreamEvent, Content, Context, Message, ProviderApi, StopReason,
    StreamOptions,
};
use timo_config::{FixedClock, RetrySettings, ThinkingLevel};
use timo_tools::ToolRegistry;

struct RecordingApi {
    script: Mutex<Vec<Vec<AssistantStreamEvent>>>,
    seen: Mutex<Vec<(String, Context)>>,
}

#[async_trait]
impl ProviderApi for RecordingApi {
    async fn stream(
        &self,
        model: &timo_ai::Model,
        context: &Context,
        options: StreamOptions,
    ) -> timo_ai::AssistantMessageStream {
        self.seen
            .lock()
            .unwrap()
            .push((model.id.clone(), context.clone()));
        let events = {
            let mut q = self.script.lock().unwrap();
            if q.is_empty() { vec![] } else { q.remove(0) }
        };
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        tokio::spawn(async move {
            for ev in events {
                if options.cancel.is_cancelled() {
                    break;
                }
                if tx.send(ev).await.is_err() {
                    return;
                }
            }
        });
        timo_ai::AssistantMessageStream::new(rx)
    }
}

impl RecordingApi {
    fn new(responses: Vec<Vec<AssistantStreamEvent>>) -> Self {
        Self {
            script: Mutex::new(responses),
            seen: Mutex::new(vec![]),
        }
    }

    fn contexts(&self) -> Vec<(String, Context)> {
        self.seen.lock().unwrap().clone()
    }
}

#[derive(Default)]
struct TestHooks {
    system_prompt: Option<&'static str>,
    block: Option<&'static str>,
    args: Option<Value>,
    patch_text: Option<&'static str>,
    empty_context: bool,
    route_to: Option<&'static str>,
    calls: Mutex<Vec<&'static str>>,
    prompt_seen: Mutex<Option<String>>,
}

impl TestHooks {
    fn seen(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl LoopHooks for TestHooks {
    async fn before_agent_start(&self, req: BeforeAgentStart<'_>) -> Option<String> {
        self.calls.lock().unwrap().push("before_agent_start");
        *self.prompt_seen.lock().unwrap() = Some(req.system_prompt.to_string());
        self.system_prompt.map(str::to_string)
    }

    async fn transform_context(&self, messages: Vec<Message>) -> Option<Vec<Message>> {
        self.calls.lock().unwrap().push("transform_context");
        if self.empty_context {
            return Some(vec![]);
        }
        let _ = messages;
        None
    }

    async fn route_model(&self, _req: RouteRequest<'_>) -> Result<Option<RouteChoice>, String> {
        self.calls.lock().unwrap().push("route_model");
        Ok(self.route_to.map(|id| {
            let mut model = base_model();
            model.id = id.to_string();
            RouteChoice {
                model,
                thinking: ThinkingLevel::Medium,
                state: None,
            }
        }))
    }

    async fn before_tool_call(&self, _call: &Content) -> Option<BeforeToolCall> {
        self.calls.lock().unwrap().push("before_tool_call");
        if let Some(reason) = self.block {
            return Some(BeforeToolCall {
                block: Some(reason.to_string()),
                ..BeforeToolCall::default()
            });
        }
        self.args.clone().map(|args| BeforeToolCall {
            args: Some(args),
            ..BeforeToolCall::default()
        })
    }

    async fn after_tool_call(&self, _outcome: &ToolOutcome<'_>) -> Option<ToolOutputPatch> {
        self.calls.lock().unwrap().push("after_tool_call");
        self.patch_text.map(|text| ToolOutputPatch {
            content: Some(vec![Content::Text {
                text: text.to_string(),
            }]),
            ..ToolOutputPatch::default()
        })
    }
}

fn base_model() -> timo_ai::Model {
    serde_json::from_value(json!({
        "id": "m", "name": "m", "provider": "mock", "api": "anthropic-messages",
        "base_url": "http://mock", "reasoning": false, "input": ["text"], "cost": {},
        "context_window": 1000, "max_tokens": 100
    }))
    .unwrap()
}

fn registry(executions: Arc<AtomicUsize>) -> Arc<ToolRegistry> {
    let mut reg = ToolRegistry::new();
    reg.register(timo_tools::ToolDefinition {
        name: "echo".into(),
        label: "Echo".into(),
        description: "echo back".into(),
        prompt_snippet: None,
        prompt_guidelines: vec![],
        parameters: json!({
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"]
        }),
        output_schema: None,
        exposure: timo_tools::Exposure::Direct,
        namespace: None,
        annotations: timo_tools::Annotations::default(),
        execution_mode: timo_tools::ExecutionMode::Parallel,
        execute: Arc::new(move |ctx| {
            executions.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(timo_tools::ToolOutput::text(format!(
                    "echo: {}",
                    ctx.params["text"].as_str().unwrap_or_default()
                )))
            })
        }),
    });
    Arc::new(reg)
}

fn handle(
    api: Arc<dyn ProviderApi>,
    hooks: Arc<dyn LoopHooks>,
    runs: Arc<AtomicUsize>,
) -> AgentHandle {
    AgentHandle::new(LoopConfig {
        model: base_model(),
        api,
        tools: registry(runs),
        thinking_level: ThinkingLevel::Off,
        auth: Arc::new(timo_ai::AuthResolver {
            explicit: Some("k".into()),
            provider_ref: None,
            env_names: vec![],
        }),
        retry: RetrySettings::default(),
        cwd: Arc::from(std::path::Path::new(".")),
        system_prompt: "base".into(),
        clock: Arc::new(FixedClock::new(1_000)),
        max_turns: None,
        compaction: None,
        hooks: Some(hooks),
        provider_hooks: None,
        review: None,
        dispatch: None,
        route_state: Default::default(),
    })
}

fn text_done(text: &str) -> Vec<AssistantStreamEvent> {
    let mut m = AssistantMessage::pending("mock", "mock", "m", 0);
    m.content = vec![Content::Text { text: text.into() }];
    m.stop_reason = StopReason::Stop;
    vec![AssistantStreamEvent::Done {
        reason: StopReason::Stop,
        message: m,
    }]
}

fn tool_call_done(call_id: &str, tool: &str, args: Value) -> Vec<AssistantStreamEvent> {
    let mut m = AssistantMessage::pending("mock", "mock", "m", 0);
    m.content = vec![Content::ToolCall {
        id: call_id.into(),
        name: tool.into(),
        arguments: args,
        arguments_error: None,
    }];
    m.stop_reason = StopReason::ToolUse;
    vec![AssistantStreamEvent::Done {
        reason: StopReason::ToolUse,
        message: m,
    }]
}

#[tokio::test]
async fn before_agent_start_replaces_system_prompt() {
    let api = Arc::new(RecordingApi::new(vec![text_done("ok")]));
    let hooks = Arc::new(TestHooks {
        system_prompt: Some("replaced"),
        ..TestHooks::default()
    });
    let agent = handle(api.clone(), hooks.clone(), Arc::new(AtomicUsize::new(0)));
    agent.run("hi".into(), vec![], None).await.unwrap();
    assert_eq!(hooks.prompt_seen.lock().unwrap().as_deref(), Some("base"));
    let contexts = api.contexts();
    assert_eq!(contexts[0].1.system_prompt.as_deref(), Some("replaced"));
}

#[tokio::test]
async fn transform_context_reaches_provider() {
    let api = Arc::new(RecordingApi::new(vec![text_done("ok")]));
    let hooks = Arc::new(TestHooks {
        empty_context: true,
        ..TestHooks::default()
    });
    let agent = handle(api.clone(), hooks.clone(), Arc::new(AtomicUsize::new(0)));
    agent.run("hi".into(), vec![], None).await.unwrap();
    let contexts = api.contexts();
    assert!(contexts[0].1.messages.is_empty());
    assert!(hooks.seen().contains(&"transform_context"));
}

#[tokio::test]
async fn route_model_switches_model_id() {
    let api = Arc::new(RecordingApi::new(vec![text_done("ok")]));
    let hooks = Arc::new(TestHooks {
        route_to: Some("virtual"),
        ..TestHooks::default()
    });
    let agent = handle(api.clone(), hooks.clone(), Arc::new(AtomicUsize::new(0)));
    let mut rx = agent.bus().subscribe();
    agent.run("hi".into(), vec![], None).await.unwrap();
    assert_eq!(api.contexts()[0].0, "virtual");
    let mut seen_provider = false;
    while let Ok(event) = rx.try_recv() {
        if let timo_agent::KernelEvent::ProviderRequest { model, .. } = event {
            assert_eq!(model, "virtual");
            seen_provider = true;
        }
    }
    assert!(seen_provider);
    assert!(hooks.seen().contains(&"route_model"));
}

#[tokio::test]
async fn before_tool_call_blocks_without_execution() {
    let api = Arc::new(RecordingApi::new(vec![
        tool_call_done("call_1", "echo", json!({"text": "hi"})),
        text_done("done"),
    ]));
    let hooks = Arc::new(TestHooks {
        block: Some("nope"),
        ..TestHooks::default()
    });
    let runs = Arc::new(AtomicUsize::new(0));
    let agent = handle(api, hooks.clone(), Arc::clone(&runs));
    let out = agent.run("go".into(), vec![], None).await.unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    match &out[2] {
        Message::ToolResult(t) => {
            assert!(t.is_error);
            match &t.content[0] {
                Content::Text { text } => {
                    assert_eq!(text, "blocked by extension: nope");
                }
                other => panic!("text expected: {other:?}"),
            }
        }
        other => panic!("tool result expected: {other:?}"),
    }
    assert!(hooks.seen().contains(&"before_tool_call"));
}

#[tokio::test]
async fn before_tool_call_rewrites_arguments() {
    let api = Arc::new(RecordingApi::new(vec![
        tool_call_done("call_1", "echo", json!({"text": "original"})),
        text_done("done"),
    ]));
    let hooks = Arc::new(TestHooks {
        args: Some(json!({"text": "patched"})),
        ..TestHooks::default()
    });
    let agent = handle(api, hooks, Arc::new(AtomicUsize::new(0)));
    let out = agent.run("go".into(), vec![], None).await.unwrap();
    match &out[2] {
        Message::ToolResult(t) => match &t.content[0] {
            Content::Text { text } => assert_eq!(text, "echo: patched"),
            other => panic!("text expected: {other:?}"),
        },
        other => panic!("tool result expected: {other:?}"),
    }
}

#[tokio::test]
async fn after_tool_call_patches_content() {
    let api = Arc::new(RecordingApi::new(vec![
        tool_call_done("call_1", "echo", json!({"text": "hi"})),
        text_done("done"),
    ]));
    let hooks = Arc::new(TestHooks {
        patch_text: Some("patched result"),
        ..TestHooks::default()
    });
    let agent = handle(api, hooks.clone(), Arc::new(AtomicUsize::new(0)));
    let out = agent.run("go".into(), vec![], None).await.unwrap();
    match &out[2] {
        Message::ToolResult(t) => match &t.content[0] {
            Content::Text { text } => assert_eq!(text, "patched result"),
            other => panic!("text expected: {other:?}"),
        },
        other => panic!("tool result expected: {other:?}"),
    }
    assert!(hooks.seen().contains(&"after_tool_call"));
}
