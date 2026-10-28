//! 路由状态落会话（设计 10 §4.2）：只增不改、Direct 不写、跨 run 可读。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};
use timo_agent::{AgentHandle, LoopConfig, LoopHooks, RouteChoice, RouteReason, RouteRequest};
use timo_ai::{
    AssistantMessage, AssistantStreamEvent, AuthResolver, Content, Context, Message, Model,
    ProviderApi, StopReason, StreamOptions,
};
use timo_config::{CompactionSettings, FixedClock, RetrySettings, ThinkingLevel};
use timo_session::{IdGen, SessionWriter, kinds, read_entries};
use timo_tools::ToolRegistry;

struct ScriptedApi {
    script: Mutex<Vec<&'static str>>,
}

#[async_trait]
impl ProviderApi for ScriptedApi {
    async fn stream(
        &self,
        model: &Model,
        _context: &Context,
        _options: StreamOptions,
    ) -> timo_ai::AssistantMessageStream {
        let text = {
            let mut script = self.script.lock().unwrap();
            if script.is_empty() {
                "done"
            } else {
                script.remove(0)
            }
        };
        let mut message =
            AssistantMessage::pending(model.api.as_str(), &model.provider, &model.id, 0);
        message.content = vec![Content::Text {
            text: text.to_string(),
        }];
        message.stop_reason = StopReason::Stop;
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        let _ = tx
            .send(AssistantStreamEvent::Done {
                reason: StopReason::Stop,
                message,
            })
            .await;
        timo_ai::AssistantMessageStream::new(rx)
    }
}

struct RouteHooks {
    pick: HashMap<&'static str, &'static str>,
    states: HashMap<&'static str, Value>,
    seen: Mutex<Vec<(RouteReason, Option<Value>)>>,
}

impl RouteHooks {
    fn new() -> Self {
        Self {
            pick: HashMap::from([("user", "small"), ("direct", "large")]),
            states: HashMap::from([
                ("user", json!({ "phase": "build" })),
                ("direct", json!({ "phase": "summary" })),
            ]),
            seen: Mutex::new(vec![]),
        }
    }

    fn seen(&self) -> Vec<(RouteReason, Option<Value>)> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait]
impl LoopHooks for RouteHooks {
    async fn route_model(&self, req: RouteRequest<'_>) -> Result<Option<RouteChoice>, String> {
        self.seen
            .lock()
            .unwrap()
            .push((req.reason, req.state.cloned()));
        let Some(id) = self.pick.get(req.reason.as_str()).copied() else {
            return Ok(None);
        };
        Ok(Some(RouteChoice {
            model: model(id),
            thinking: ThinkingLevel::Medium,
            state: self.states.get(req.reason.as_str()).cloned(),
        }))
    }
}

fn model(id: &str) -> Model {
    serde_json::from_value(json!({
        "id": id, "name": id, "provider": "mock", "api": "anthropic-messages",
        "base_url": "http://mock", "reasoning": false, "input": ["text"], "cost": {},
        "context_window": 12, "max_tokens": 100
    }))
    .unwrap()
}

fn registry() -> Arc<ToolRegistry> {
    Arc::new(ToolRegistry::new())
}

fn session_writer(dir: &std::path::Path) -> Arc<tokio::sync::Mutex<SessionWriter>> {
    let writer = SessionWriter::create(
        dir,
        "test",
        Some("router/auto"),
        Arc::new(FixedClock::new(1_000)),
        Arc::new(IdGen::new()),
    )
    .unwrap();
    Arc::new(tokio::sync::Mutex::new(writer))
}

fn handle(
    api: Arc<dyn ProviderApi>,
    hooks: Arc<dyn LoopHooks>,
    route_state: Arc<Mutex<Option<Value>>>,
    compaction: Option<CompactionSettings>,
) -> AgentHandle {
    AgentHandle::new(LoopConfig {
        model: Model::virtual_entry("router", "auto", "Auto"),
        api,
        tools: registry(),
        thinking_level: ThinkingLevel::Off,
        auth: Arc::new(AuthResolver {
            explicit: Some("k".into()),
            provider_ref: None,
            env_names: vec![],
        }),
        retry: RetrySettings::default(),
        cwd: Arc::from(std::path::Path::new(".")),
        system_prompt: "base".into(),
        clock: Arc::new(FixedClock::new(1_000)),
        max_turns: None,
        compaction,
        hooks: Some(hooks),
        provider_hooks: None,
        review: None,
        dispatch: None,
        route_state,
    })
}

fn history() -> Vec<Message> {
    vec![
        timo_ai::user_text(&"a".repeat(400), 1),
        Message::Assistant({
            let mut message = AssistantMessage::pending("anthropic-messages", "mock", "small", 2);
            message.content = vec![Content::Text {
                text: "older answer".into(),
            }];
            message.stop_reason = StopReason::Stop;
            message
        }),
    ]
}

#[tokio::test]
async fn state_is_written_once_and_survives_across_runs() {
    let dir = tempfile::tempdir().unwrap();
    let sink = session_writer(dir.path());
    let slot: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
    let hooks = Arc::new(RouteHooks::new());
    let api = Arc::new(ScriptedApi {
        script: Mutex::new(vec!["summary", "done"]),
    });
    let agent = handle(api, hooks.clone(), Arc::clone(&slot), None);

    let out = agent
        .run_with_context("hi".into(), history(), Some(Arc::clone(&sink)))
        .await
        .unwrap();
    assert_eq!(
        slot.lock().unwrap().clone(),
        Some(json!({ "phase": "build" }))
    );
    let entries = read_entries(sink.lock().await.path()).unwrap();
    let states: Vec<&Value> = entries
        .iter()
        .filter(|entry| entry.kind == kinds::VIRTUAL_MODEL_STATE)
        .map(|entry| &entry.payload)
        .collect();
    assert_eq!(states.len(), 1, "{states:?}");
    assert_eq!(states[0]["provider"], "router");
    assert_eq!(states[0]["model_id"], "auto");
    assert_eq!(states[0]["state"], json!({ "phase": "build" }));

    // 第二次 run 从共享槽读到状态；同一状态不重复写 entry。
    let _ = agent
        .run_with_context("again".into(), out.context, Some(Arc::clone(&sink)))
        .await
        .unwrap();
    let seen = hooks.seen();
    let second_user = seen
        .iter()
        .rev()
        .find(|(reason, _)| *reason == RouteReason::User)
        .expect("user route");
    assert_eq!(
        second_user.1,
        Some(json!({ "phase": "build" })),
        "跨 run 状态必须可读"
    );
    let entries = read_entries(sink.lock().await.path()).unwrap();
    let states = entries
        .iter()
        .filter(|entry| entry.kind == kinds::VIRTUAL_MODEL_STATE)
        .count();
    assert_eq!(states, 1, "未变化的状态不得重复落盘");
}

#[tokio::test]
async fn direct_routes_never_write_state() {
    let dir = tempfile::tempdir().unwrap();
    let sink = session_writer(dir.path());
    let slot: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
    let hooks = Arc::new(RouteHooks::new());
    let api = Arc::new(ScriptedApi {
        script: Mutex::new(vec!["summary", "done"]),
    });
    // 窗口 12、保留 1 token：历史 400 字触发阈值压缩 → 摘要请求走 Direct。
    let compaction = CompactionSettings {
        enabled: true,
        reserve_tokens: 0,
        keep_recent_tokens: 1,
    };
    let agent = handle(api, hooks.clone(), slot, Some(compaction));
    agent
        .run_with_context("hi".into(), history(), Some(Arc::clone(&sink)))
        .await
        .unwrap();

    assert!(
        hooks
            .seen()
            .iter()
            .any(|(reason, _)| *reason == RouteReason::Direct),
        "压缩摘要必须经过 Direct 路由：{:?}",
        hooks.seen()
    );
    let entries = read_entries(sink.lock().await.path()).unwrap();
    let states: Vec<&Value> = entries
        .iter()
        .filter(|entry| entry.kind == kinds::VIRTUAL_MODEL_STATE)
        .map(|entry| &entry.payload)
        .collect();
    assert_eq!(states.len(), 1, "Direct 状态不得落盘：{states:?}");
    assert_eq!(states[0]["state"], json!({ "phase": "build" }));
}
