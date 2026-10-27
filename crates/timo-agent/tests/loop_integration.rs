//! 循环集成测试：mock 协议适配器驱动完整 run（standards/05 §1 集成层；
//! 接缝 #2/#6/#7 断言——设计 07 §4）。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use timo_agent::{AgentHandle, KernelEvent, LoopConfig, ToolExecutionMode};
use timo_ai::{
    AssistantMessage, AssistantStreamEvent, Content, Message, ProviderApi, StopReason,
    StreamOptions, Tool, api_for,
};
use timo_config::{FixedClock, RetrySettings, ThinkingLevel};
use timo_tools::ToolRegistry;
use tokio_util::sync::CancellationToken;

/// 脚本化适配器：按脚本依次吐流（脚本耗尽后重复最后一段）。
struct ScriptedApi {
    script: std::sync::Mutex<Vec<Vec<AssistantStreamEvent>>>,
}

impl ScriptedApi {
    fn new(responses: Vec<Vec<AssistantStreamEvent>>) -> Self {
        Self {
            script: std::sync::Mutex::new(responses),
        }
    }
}

#[async_trait::async_trait]
impl ProviderApi for ScriptedApi {
    async fn stream(
        &self,
        _model: &timo_ai::Model,
        _context: &timo_ai::Context,
        options: StreamOptions,
    ) -> timo_ai::AssistantMessageStream {
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

fn base_model() -> timo_ai::Model {
    serde_json::from_value(serde_json::json!({
        "id": "m", "name": "m", "provider": "mock", "api": "anthropic-messages",
        "base_url": "http://mock", "reasoning": false, "input": ["text"], "cost": {},
        "context_window": 1000, "max_tokens": 100
    }))
    .unwrap()
}

fn echo_tool_registry() -> Arc<ToolRegistry> {
    let mut reg = ToolRegistry::new();
    reg.register(timo_tools::ToolDefinition {
        name: "echo".into(),
        label: "Echo".into(),
        description: "echo back".into(),
        prompt_snippet: None,
        prompt_guidelines: vec![],
        parameters: serde_json::json!({
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
    reg.register(timo_tools::ToolDefinition {
        name: "review_probe".into(),
        label: "Review probe".into(),
        description: "reports whether the tool context carries a reviewer".into(),
        prompt_snippet: None,
        prompt_guidelines: vec![],
        parameters: serde_json::json!({"type": "object", "properties": {}}),
        output_schema: None,
        exposure: timo_tools::Exposure::Direct,
        namespace: None,
        annotations: timo_tools::Annotations::default(),
        execution_mode: timo_tools::ExecutionMode::Parallel,
        execute: Arc::new(|ctx| {
            Box::pin(async move {
                Ok(timo_tools::ToolOutput::text(format!(
                    "review={}",
                    if ctx.review.is_some() { "yes" } else { "no" }
                )))
            })
        }),
    });
    // 子代理型工具：内部另有一次模型调用，通过 ToolOutput.usage 回报用量（设计 08 §4.3）。
    reg.register(timo_tools::ToolDefinition {
        name: "scout".into(),
        label: "Scout".into(),
        description: "pretends to be a nested agent and reports its usage".into(),
        prompt_snippet: None,
        prompt_guidelines: vec![],
        parameters: serde_json::json!({"type": "object", "properties": {}}),
        output_schema: None,
        exposure: timo_tools::Exposure::Direct,
        namespace: None,
        annotations: timo_tools::Annotations::default(),
        execution_mode: timo_tools::ExecutionMode::Parallel,
        execute: Arc::new(|_| {
            Box::pin(async move {
                Ok(timo_tools::ToolOutput {
                    usage: Some(timo_ai::Usage {
                        input: 4_321,
                        output: 128,
                        total_tokens: 4_449,
                        cost: timo_ai::Cost {
                            total: 0.0107,
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ..timo_tools::ToolOutput::text("scout: 3 files")
                })
            })
        }),
    });
    Arc::new(reg)
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

fn tool_call_done(call_id: &str, tool: &str, args: serde_json::Value) -> Vec<AssistantStreamEvent> {
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

fn handle(api: Arc<dyn ProviderApi>) -> AgentHandle {
    handle_with_review(api, None)
}

fn handle_with_review(
    api: Arc<dyn ProviderApi>,
    review: Option<Arc<dyn timo_tools::ToolReview>>,
) -> AgentHandle {
    AgentHandle::new(LoopConfig {
        model: base_model(),
        api,
        tools: echo_tool_registry(),
        thinking_level: ThinkingLevel::Off,
        auth: Arc::new(timo_ai::AuthResolver {
            explicit: Some("k".into()),
            provider_ref: None,
            env_names: vec![],
        }),
        tool_execution: ToolExecutionMode::Parallel,
        retry: RetrySettings::default(),
        cwd: Arc::from(std::path::Path::new(".")),
        system_prompt: "test".into(),
        clock: Arc::new(FixedClock::new(1_000)),
        max_turns: None,
        compaction: None,
        hooks: None,
        provider_hooks: None,
        review,
        dispatch: None,
        route_state: Default::default(),
    })
}

#[tokio::test]
async fn full_loop_text_then_tool_then_stop() {
    let api = Arc::new(ScriptedApi::new(vec![
        tool_call_done("call_1", "echo", serde_json::json!({"text": "hi"})),
        text_done("all done"),
    ]));
    let agent = handle(api);
    let mut rx = agent.bus().subscribe();
    let out = agent.run("do it".into(), vec![], None).await.expect("run");
    // 事件序列：Started 首位、Finished 末位（run 契约）
    let first = rx.recv().await.unwrap();
    let mut last = first.clone();
    let mut count = 1;
    while let Ok(ev) = rx.try_recv() {
        last = ev;
        count += 1;
    }
    assert!(matches!(first, KernelEvent::AgentStarted));
    assert!(matches!(last, KernelEvent::AgentFinished { .. }));
    // 消息序列：user → assistant(toolUse) → toolResult → assistant(stop)
    assert_eq!(out.len(), 4);
    assert!(matches!(&out[1], Message::Assistant(a) if a.stop_reason == StopReason::ToolUse));
    match &out[2] {
        Message::ToolResult(t) => {
            assert!(!t.is_error);
            assert_eq!(t.tool_call_id, "call_1");
            match &t.content[0] {
                Content::Text { text } => assert_eq!(text, "echo: hi"),
                other => panic!("text expected: {other:?}"),
            }
        }
        other => panic!("tool result expected: {other:?}"),
    }
    // 接缝 #6：流事件富枚举通路存在（本轮无增量，Done 已在 StreamEvent 通路转发）
    let _ = count;
}

#[tokio::test]
async fn a_tool_can_report_usage_into_its_result_message() {
    let api = Arc::new(ScriptedApi::new(vec![
        tool_call_done("call_9", "scout", serde_json::json!({})),
        text_done("done"),
    ]));
    let agent = handle(api);
    let mut rx = agent.bus().subscribe();
    let out = agent.run("go".into(), vec![], None).await.expect("run");
    let message_usage = match &out[2] {
        Message::ToolResult(t) => t.usage.clone().expect("tool usage forwarded"),
        other => panic!("tool result expected: {other:?}"),
    };
    assert_eq!(message_usage.input, 4_321);
    assert_eq!(message_usage.total_tokens, 4_449);
    assert!(
        (message_usage.cost.total - 0.0107).abs() < 1e-12,
        "{message_usage:?}"
    );
    // 事件通路同样带出用量：UI 与 `/session` 都从消息/事件现算，不只看上下文
    let mut event_usage = None;
    while let Ok(event) = rx.try_recv() {
        if let KernelEvent::ToolCallFinished { result, .. } = event
            && let Message::ToolResult(tool_result) = result.as_ref()
        {
            event_usage = tool_result.usage.clone();
        }
    }
    assert_eq!(event_usage.expect("event carries usage").input, 4_321);
}

struct AllowingReview;

#[async_trait::async_trait]
impl timo_tools::ToolReview for AllowingReview {
    async fn review(&self, _: timo_tools::ToolReviewRequest) -> timo_tools::ReviewDecision {
        timo_tools::ReviewDecision::Allow
    }
}

#[tokio::test]
async fn loop_config_review_reaches_the_tool_context() {
    let api = Arc::new(ScriptedApi::new(vec![
        tool_call_done("call_r", "review_probe", serde_json::json!({})),
        text_done("done"),
    ]));
    let agent = handle_with_review(api, Some(Arc::new(AllowingReview)));
    let out = agent.run("x".into(), vec![], None).await.unwrap();
    match &out[2] {
        Message::ToolResult(result) => match &result.content[0] {
            Content::Text { text } => assert_eq!(text, "review=yes"),
            other => panic!("text expected: {other:?}"),
        },
        other => panic!("tool result expected: {other:?}"),
    }
}

#[tokio::test]
async fn run_with_context_carries_history_into_the_next_turn() {
    let api = Arc::new(ScriptedApi::new(vec![
        text_done("first reply"),
        text_done("second reply"),
    ]));
    let agent = handle(api);
    let first = agent
        .run_with_context("one".into(), vec![], None)
        .await
        .expect("first run");
    assert_eq!(first.new_messages.len(), 2);
    assert_eq!(first.context.len(), 2);

    let second = agent
        .run_with_context("two".into(), first.context, None)
        .await
        .expect("second run");
    // 只返回本轮新增，但上下文保留两轮（K0）
    assert_eq!(second.new_messages.len(), 2);
    assert_eq!(second.context.len(), 4);
    assert!(
        matches!(&second.context[0], Message::User(u) if matches!(&u.content[0], Content::Text { text } if text == "one"))
    );
    assert!(
        matches!(&second.context[2], Message::User(u) if matches!(&u.content[0], Content::Text { text } if text == "two"))
    );
}

#[tokio::test]
async fn error_terminal_hard_exit_no_tool_execution() {
    let mut m = AssistantMessage::pending("mock", "mock", "m", 0);
    m.stop_reason = StopReason::Error;
    m.error_message = Some("invalid schema for tools".into()); // 不可重试文案
    let api = Arc::new(ScriptedApi::new(vec![vec![AssistantStreamEvent::Error {
        message: m,
    }]]));
    let agent = handle(api);
    let out = agent.run("x".into(), vec![], None).await.unwrap();
    // user + error assistant；无工具结果
    assert_eq!(out.len(), 2);
    match &out[1] {
        Message::Assistant(a) => {
            assert_eq!(a.stop_reason, StopReason::Error);
            assert!(
                a.error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains("invalid schema")
            );
        }
        other => panic!("assistant expected: {other:?}"),
    }
}

#[tokio::test]
async fn steering_message_injected_before_next_request() {
    let api = Arc::new(ScriptedApi::new(vec![
        tool_call_done("call_1", "echo", serde_json::json!({"text": "a"})),
        text_done("done"),
    ]));
    let agent = handle(api);
    agent.steering().push(timo_ai::user_text("steer!", 42));
    let out = agent.run("start".into(), vec![], None).await.unwrap();
    // 初始 steering 轮询：steer 在首轮请求前注入（pi runLoop 启动时同构）
    // 顺序：user(prompt) → user(steer) → assistant → toolResult → assistant(stop)
    assert_eq!(out.len(), 5);
    assert!(
        matches!(&out[1], Message::User(u) if matches!(u.content[0], Content::Text { ref text } if text == "steer!"))
    );
    assert!(matches!(&out[4], Message::Assistant(a) if a.stop_reason == StopReason::Stop));
}

#[tokio::test]
async fn session_writer_receives_messages() {
    let dir = std::env::temp_dir().join(format!("timo-agent-sess-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let writer = timo_session::SessionWriter::create(
        &dir,
        "D:/x",
        None,
        Arc::new(FixedClock::new(1)),
        Arc::new(timo_session::IdGen::new()),
    )
    .unwrap();
    let sink = Arc::new(tokio::sync::Mutex::new(writer));
    let api = Arc::new(ScriptedApi::new(vec![text_done("ok")]));
    let agent = handle(api);
    let out = agent
        .run("hello".into(), vec![], Some(Arc::clone(&sink)))
        .await
        .unwrap();
    let entries =
        timo_session::read_entries(&dir.join(format!("{}.jsonl", sink.lock().await.session_id())))
            .unwrap();
    // header + user + assistant
    assert_eq!(entries.len(), 3);
    assert_eq!(out.len(), 2);
}

#[tokio::test]
async fn subscriber_compiles_without_loop_changes_seam2() {
    // 接缝 #2 检查点：外部订阅者仅依赖 KernelEvent，无需触碰循环
    let api = Arc::new(ScriptedApi::new(vec![text_done("s")]));
    let agent = handle(api);
    let mut rx = agent.bus().subscribe();
    let _ = agent.run("x".into(), vec![], None).await.unwrap();
    let mut saw_stream = false;
    while let Ok(ev) = rx.try_recv() {
        if let KernelEvent::StreamEvent { .. } = ev {
            saw_stream = true;
        }
    }
    assert!(saw_stream);
}

#[allow(dead_code)]
fn api_factory_reference() -> Arc<dyn ProviderApi> {
    // 保持 api_for 在测试构建中被引用（真实适配器经 timo-ai 测试覆盖）
    api_for(timo_ai::ApiKind::AnthropicMessages)
}

#[allow(dead_code)]
fn tool_decl_reference() -> Tool {
    Tool {
        name: "t".into(),
        description: "d".into(),
        parameters: serde_json::json!({}),
    }
}

#[allow(dead_code)]
fn cancel_reference() -> CancellationToken {
    CancellationToken::new()
}
