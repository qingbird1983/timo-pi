//! 虚拟模型路由契约（设计 10 §4.2）：reason 判定、per-request 模型、状态与失败语义。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};
use timo_agent::{
    AgentHandle, LoopConfig, LoopHooks, ModelDispatch, RouteChoice, RouteReason, RouteRequest,
    ToolExecutionMode,
};
use timo_ai::{
    ApiKind, AssistantMessage, AssistantStreamEvent, AuthResolver, Content, Context, Message,
    Model, ProviderApi, StopReason, StreamOptions,
};
use timo_config::{FixedClock, RetrySettings, ThinkingLevel};
use timo_tools::ToolRegistry;

/// 脚本一步：终态消息由适配器按本次请求模型构造。
#[derive(Clone)]
enum Step {
    Text(&'static str),
    ToolCall,
    Error(&'static str),
}

struct ScriptedApi {
    script: Mutex<Vec<Step>>,
    seen: Mutex<Vec<(String, ThinkingLevel)>>,
}

#[async_trait]
impl ProviderApi for ScriptedApi {
    async fn stream(
        &self,
        model: &Model,
        context: &Context,
        _options: StreamOptions,
    ) -> timo_ai::AssistantMessageStream {
        self.seen.lock().unwrap().push((
            model.id.clone(),
            context.thinking_level.unwrap_or(ThinkingLevel::Off),
        ));
        let step = {
            let mut script = self.script.lock().unwrap();
            if script.is_empty() {
                Step::Text("done")
            } else {
                script.remove(0)
            }
        };
        let mut message =
            AssistantMessage::pending(model.api.as_str(), &model.provider, &model.id, 0);
        let event = match step {
            Step::Text(text) => {
                message.content = vec![Content::Text {
                    text: text.to_string(),
                }];
                message.stop_reason = StopReason::Stop;
                AssistantStreamEvent::Done {
                    reason: StopReason::Stop,
                    message,
                }
            }
            Step::ToolCall => {
                message.content = vec![Content::ToolCall {
                    id: "call_1".into(),
                    name: "echo".into(),
                    arguments: json!({ "text": "hi" }),
                    arguments_error: None,
                }];
                message.stop_reason = StopReason::ToolUse;
                AssistantStreamEvent::Done {
                    reason: StopReason::ToolUse,
                    message,
                }
            }
            Step::Error(reason) => {
                message.stop_reason = StopReason::Error;
                message.error_message = Some(reason.to_string());
                AssistantStreamEvent::Error { message }
            }
        };
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let _ = tx.send(event).await;
        timo_ai::AssistantMessageStream::new(rx)
    }
}

impl ScriptedApi {
    fn new(steps: Vec<Step>) -> Self {
        Self {
            script: Mutex::new(steps),
            seen: Mutex::new(vec![]),
        }
    }

    fn requests(&self) -> Vec<(String, ThinkingLevel)> {
        self.seen.lock().unwrap().clone()
    }
}

/// 路由请求的可观察快照。
#[derive(Clone)]
struct SeenRoute {
    reason: RouteReason,
    selected: (String, String),
    thinking: ThinkingLevel,
    previous: Option<(String, String, ThinkingLevel)>,
    failed: Option<(String, ThinkingLevel)>,
    state: Option<Value>,
}

struct RouteHooks {
    /// reason 规范名 → 请求模型 id。
    pick: Mutex<std::collections::HashMap<&'static str, &'static str>>,
    state_out: Mutex<Option<Value>>,
    fail: Option<&'static str>,
    seen: Mutex<Vec<SeenRoute>>,
}

impl RouteHooks {
    fn new(pairs: &[(&'static str, &'static str)]) -> Self {
        Self {
            pick: Mutex::new(pairs.iter().copied().collect()),
            state_out: Mutex::new(None),
            fail: None,
            seen: Mutex::new(vec![]),
        }
    }

    fn failing(message: &'static str) -> Self {
        Self {
            fail: Some(message),
            ..Self::new(&[])
        }
    }

    fn seen(&self) -> Vec<SeenRoute> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait]
impl LoopHooks for RouteHooks {
    async fn route_model(&self, req: RouteRequest<'_>) -> Result<Option<RouteChoice>, String> {
        self.seen.lock().unwrap().push(SeenRoute {
            reason: req.reason,
            selected: (req.model.provider.clone(), req.model.id.clone()),
            thinking: req.thinking,
            previous: req
                .previous
                .map(|(model, level)| (model.provider.clone(), model.id.clone(), level)),
            failed: req.failed.map(|(model, level)| (model.id.clone(), level)),
            state: req.state.cloned(),
        });
        if let Some(message) = self.fail {
            return Err(message.to_string());
        }
        let id = self.pick.lock().unwrap().get(req.reason.as_str()).copied();
        Ok(id.map(|id| RouteChoice {
            model: model(id),
            thinking: ThinkingLevel::Medium,
            state: self.state_out.lock().unwrap().take(),
        }))
    }
}

fn model(id: &str) -> Model {
    serde_json::from_value(json!({
        "id": id, "name": id, "provider": "mock", "api": "anthropic-messages",
        "base_url": "http://mock", "reasoning": false, "input": ["text"], "cost": {},
        "context_window": 1000, "max_tokens": 100
    }))
    .unwrap()
}

fn virtual_model() -> Model {
    Model::virtual_entry("router", "auto", "Auto")
}

fn registry() -> Arc<ToolRegistry> {
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
        execute: Arc::new(|ctx| {
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
    model: Model,
    retry: RetrySettings,
    dispatch: Option<Arc<dyn ModelDispatch>>,
) -> AgentHandle {
    AgentHandle::new(LoopConfig {
        model,
        api,
        tools: registry(),
        thinking_level: ThinkingLevel::Low,
        auth: Arc::new(AuthResolver {
            explicit: Some("k".into()),
            provider_ref: None,
            env_names: vec![],
        }),
        tool_execution: ToolExecutionMode::Parallel,
        retry,
        cwd: Arc::from(std::path::Path::new(".")),
        system_prompt: "base".into(),
        clock: Arc::new(FixedClock::new(1_000)),
        max_turns: None,
        compaction: None,
        hooks: Some(hooks),
        provider_hooks: None,
        review: None,
        dispatch,
    })
}

fn fast_retry() -> RetrySettings {
    RetrySettings {
        enabled: true,
        max_retries: 1,
        base_delay_ms: 0,
        max_retry_delay_ms: 0,
    }
}

#[tokio::test]
async fn route_reason_switches_the_request_model_per_turn() {
    let api = Arc::new(ScriptedApi::new(vec![Step::ToolCall, Step::Text("done")]));
    let hooks = Arc::new(RouteHooks::new(&[
        ("user", "small"),
        ("continuation", "large"),
    ]));
    let agent = handle(
        api.clone(),
        hooks.clone(),
        virtual_model(),
        RetrySettings::default(),
        None,
    );
    agent.run("hi".into(), vec![], None).await.unwrap();

    let requests = api.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].0, "small");
    assert_eq!(requests[1].0, "large");

    let seen = hooks.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].reason, RouteReason::User);
    assert_eq!(seen[0].thinking, ThinkingLevel::Low, "虚拟档位原样交给路由");
    assert_eq!(seen[1].reason, RouteReason::Continuation);
    assert_eq!(
        seen[0].selected,
        ("router".to_string(), "auto".to_string()),
        "选中模型在本 run 内保持不变"
    );
    assert_eq!(seen[1].selected, ("router".to_string(), "auto".to_string()));
}

#[tokio::test]
async fn previous_reports_the_latest_successful_response() {
    let api = Arc::new(ScriptedApi::new(vec![Step::ToolCall, Step::Text("done")]));
    let hooks = Arc::new(RouteHooks::new(&[
        ("user", "small"),
        ("continuation", "large"),
    ]));
    let agent = handle(
        api,
        hooks.clone(),
        virtual_model(),
        RetrySettings::default(),
        None,
    );
    agent.run("hi".into(), vec![], None).await.unwrap();

    let seen = hooks.seen();
    assert_eq!(seen[0].previous, None);
    assert_eq!(
        seen[1].previous,
        Some((
            "mock".to_string(),
            "small".to_string(),
            ThinkingLevel::Medium
        ))
    );
}

#[tokio::test]
async fn retries_route_with_the_failed_model() {
    let api = Arc::new(ScriptedApi::new(vec![
        Step::Error("Rate limit exceeded (429)"),
        Step::Text("done"),
    ]));
    let hooks = Arc::new(RouteHooks::new(&[("user", "small"), ("retry", "large")]));
    let agent = handle(
        api.clone(),
        hooks.clone(),
        virtual_model(),
        fast_retry(),
        None,
    );
    agent.run("hi".into(), vec![], None).await.unwrap();

    let requests = api.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].0, "small");
    assert_eq!(requests[1].0, "large");
    let seen = hooks.seen();
    assert_eq!(seen[1].reason, RouteReason::Retry);
    assert_eq!(
        seen[1].failed,
        Some(("small".to_string(), ThinkingLevel::Medium)),
        "重试带上失败的请求模型"
    );
}

#[tokio::test]
async fn router_state_round_trips_between_requests() {
    let api = Arc::new(ScriptedApi::new(vec![Step::ToolCall, Step::Text("done")]));
    let hooks = Arc::new(RouteHooks::new(&[
        ("user", "small"),
        ("continuation", "large"),
    ]));
    *hooks.state_out.lock().unwrap() = Some(json!({ "phase": 2 }));
    let agent = handle(
        api,
        hooks.clone(),
        virtual_model(),
        RetrySettings::default(),
        None,
    );
    agent.run("hi".into(), vec![], None).await.unwrap();

    let seen = hooks.seen();
    assert_eq!(seen[0].state, None);
    assert_eq!(seen[1].state, Some(json!({ "phase": 2 })));
}

#[tokio::test]
async fn a_routing_error_ends_the_request_with_the_virtual_name() {
    let api = Arc::new(ScriptedApi::new(vec![Step::Text("unused")]));
    let hooks = Arc::new(RouteHooks::failing("router exploded"));
    let agent = handle(
        api.clone(),
        hooks,
        virtual_model(),
        RetrySettings::default(),
        None,
    );
    let out = agent.run("hi".into(), vec![], None).await.unwrap();
    assert!(api.requests().is_empty(), "路由失败不得发起请求");
    let Message::Assistant(message) = out.last().unwrap() else {
        panic!("assistant expected");
    };
    assert_eq!(message.stop_reason, StopReason::Error);
    assert_eq!(message.provider, "router");
    assert_eq!(message.model, "auto");
    assert!(
        message
            .error_message
            .as_deref()
            .is_some_and(|text| text.contains("router exploded")),
        "{:?}",
        message.error_message
    );
}

#[tokio::test]
async fn dispatch_picks_the_adapter_and_rejects_non_catalog_targets() {
    let config_api = Arc::new(ScriptedApi::new(vec![Step::Text("wrong")]));
    let dispatch_api = Arc::new(ScriptedApi::new(vec![Step::Text("done")]));
    struct FakeDispatch {
        api: Arc<ScriptedApi>,
        calls: AtomicUsize,
    }
    impl ModelDispatch for FakeDispatch {
        fn resolve(&self, provider: &str, id: &str) -> Option<Model> {
            (provider == "mock" && id == "small").then(|| model(id))
        }
        fn api(&self, _model: &Model) -> Arc<dyn ProviderApi> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.api.clone()
        }
        fn auth(&self, _model: &Model) -> Arc<AuthResolver> {
            Arc::new(AuthResolver {
                explicit: Some("k".into()),
                provider_ref: None,
                env_names: vec![],
            })
        }
    }

    // 命中目录：请求走 dispatch 的 adapter，装配期 adapter 一次都不碰。
    let dispatch = Arc::new(FakeDispatch {
        api: dispatch_api.clone(),
        calls: AtomicUsize::new(0),
    });
    let hooks = Arc::new(RouteHooks::new(&[("user", "small")]));
    let agent = handle(
        config_api.clone(),
        hooks,
        virtual_model(),
        RetrySettings::default(),
        Some(dispatch.clone()),
    );
    agent.run("hi".into(), vec![], None).await.unwrap();
    assert_eq!(dispatch.calls.load(Ordering::SeqCst), 1);
    assert_eq!(dispatch_api.requests()[0].0, "small");
    assert!(config_api.requests().is_empty());

    // 路由到目录外的模型：拒绝终态，且仍然零请求。
    let hooks = Arc::new(RouteHooks::new(&[("user", "unknown")]));
    let agent = handle(
        Arc::new(ScriptedApi::new(vec![Step::Text("unused")])),
        hooks,
        virtual_model(),
        RetrySettings::default(),
        Some(dispatch),
    );
    let out = agent.run("hi".into(), vec![], None).await.unwrap();
    let Message::Assistant(message) = out.last().unwrap() else {
        panic!("assistant expected");
    };
    assert_eq!(message.stop_reason, StopReason::Error);
    assert_eq!(message.model, "auto");
    assert!(
        message
            .error_message
            .as_deref()
            .is_some_and(|text| text.contains("not a physical model")),
        "{:?}",
        message.error_message
    );
}

#[tokio::test]
async fn an_unrouted_virtual_selection_is_rejected_without_network() {
    let agent = AgentHandle::new(LoopConfig {
        model: virtual_model(),
        api: timo_ai::api_for(ApiKind::Virtual),
        tools: registry(),
        thinking_level: ThinkingLevel::Off,
        auth: Arc::new(AuthResolver::default()),
        tool_execution: ToolExecutionMode::Parallel,
        retry: RetrySettings::default(),
        cwd: Arc::from(std::path::Path::new(".")),
        system_prompt: "base".into(),
        clock: Arc::new(FixedClock::new(1_000)),
        max_turns: None,
        compaction: None,
        hooks: None,
        provider_hooks: None,
        review: None,
        dispatch: None,
    });
    let out = agent.run("hi".into(), vec![], None).await.unwrap();
    let Message::Assistant(message) = out.last().unwrap() else {
        panic!("assistant expected");
    };
    assert_eq!(message.stop_reason, StopReason::Error);
    assert_eq!(message.model, "auto");
    assert_eq!(
        message.error_message.as_deref(),
        Some("Virtual model router/auto must be routed before streaming")
    );
}
