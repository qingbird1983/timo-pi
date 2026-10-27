//! 协议 fixture 回放（设计 07 §2）：真实适配器 + 注入回放传输，不触网。
//!
//! 每个 `/fixtures/protocol/<api>/<case>.sse` 配 `<case>.json` 期望终态；
//! 覆盖四类路径：正常流式、工具调用、错误、取消（流中途 cancel）。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use timo_ai::{
    AiError, ApiKind, AssistantMessage, AssistantStreamEvent, ByteStream, Catalog, Content,
    Context, SseRequest, SseTransport, StopReason, StreamOptions, api_with_transport, user_text,
};
use tokio_util::sync::CancellationToken;

/// 回放传输：把夹具按行切片逐块吐出；`hold_open` 模拟"流未结束"。
struct ReplayTransport {
    chunks: Vec<Vec<u8>>,
    hold_open: bool,
}

#[async_trait]
impl SseTransport for ReplayTransport {
    async fn post_sse(&self, _request: SseRequest<'_>) -> Result<ByteStream, AiError> {
        let items: Vec<Result<Vec<u8>, String>> = self.chunks.iter().cloned().map(Ok).collect();
        if self.hold_open {
            Ok(Box::pin(futures::stream::iter(items).chain(
                futures::stream::pending::<Result<Vec<u8>, String>>(),
            )))
        } else {
            Ok(Box::pin(futures::stream::iter(items)))
        }
    }
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/protocol")
}

fn read_fixture(api: &str, case: &str, ext: &str) -> String {
    let path = fixture_dir().join(api).join(format!("{case}.{ext}"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
}

fn chunks_of(raw: &str) -> Vec<Vec<u8>> {
    raw.lines()
        .map(|line| format!("{line}\n").into_bytes())
        .collect()
}

fn model(kind: ApiKind) -> timo_ai::Model {
    let (provider, id) = match kind {
        ApiKind::AnthropicMessages => ("anthropic", "claude-sonnet-4-5"),
        ApiKind::OpenaiCompletions => ("openai", "gpt-4"),
        other => panic!("no catalog fixture for {other:?}"),
    };
    Catalog::builtin()
        .get(provider, id)
        .unwrap_or_else(|| panic!("catalog model {provider}/{id}"))
        .clone()
}

fn context() -> Context {
    Context {
        system_prompt: None,
        messages: vec![user_text("hi", 0)],
        tools: vec![],
        thinking_level: None,
    }
}

/// 回放一个夹具；`cancel_after_first_delta` 用于取消路径。
async fn replay(
    kind: ApiKind,
    case: &str,
    hold_open: bool,
    cancel_after_first_delta: bool,
) -> Vec<AssistantStreamEvent> {
    let api_name = match kind {
        ApiKind::AnthropicMessages => "anthropic",
        ApiKind::OpenaiCompletions => "openai",
        other => panic!("no fixture dir for {other:?}"),
    };
    let raw = read_fixture(api_name, case, "sse");
    let transport = Arc::new(ReplayTransport {
        chunks: chunks_of(&raw),
        hold_open,
    });
    let api = api_with_transport(kind, transport);
    let cancel = CancellationToken::new();
    let options = StreamOptions {
        api_key: Some("fixture-key".into()),
        cancel: cancel.clone(),
        ..Default::default()
    };
    let mut stream = api.stream(&model(kind), &context(), options).await;
    let mut events = vec![];
    while let Some(event) = stream.next_event().await {
        let is_delta = matches!(event, AssistantStreamEvent::Delta { .. });
        events.push(event);
        if cancel_after_first_delta && is_delta {
            cancel.cancel();
        }
    }
    events
}

fn terminal_message(events: &[AssistantStreamEvent]) -> &AssistantMessage {
    match events.last().expect("terminal event") {
        AssistantStreamEvent::Done { message, .. } | AssistantStreamEvent::Error { message } => {
            message
        }
        other => panic!("unexpected terminal: {other:?}"),
    }
}

fn message_text(message: &AssistantMessage) -> String {
    message
        .content
        .iter()
        .filter_map(|c| match c {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn assert_expected(events: &[AssistantStreamEvent], api_name: &str, case: &str) {
    let raw = read_fixture(api_name, case, "json");
    let expected: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let message = terminal_message(events);

    let want_reason: StopReason =
        serde_json::from_value(expected["stop_reason"].clone()).expect("stop_reason");
    assert_eq!(message.stop_reason, want_reason, "stop_reason");

    if let Some(text) = expected["text"].as_str() {
        assert_eq!(message_text(message), text, "text");
    }
    if let Some(needle) = expected["text_contains"].as_str() {
        assert!(
            message_text(message).contains(needle),
            "text {:?} must contain {needle:?}",
            message_text(message)
        );
    }
    if let Some(needle) = expected["error_contains"].as_str() {
        let err = message.error_message.as_deref().unwrap_or_default();
        assert!(
            err.contains(needle),
            "error {err:?} must contain {needle:?}"
        );
    }
    if let Some(usage) = expected["usage"].as_object() {
        assert_eq!(
            message.usage.input,
            usage["input"].as_u64().unwrap(),
            "usage.input"
        );
        assert_eq!(
            message.usage.output,
            usage["output"].as_u64().unwrap(),
            "usage.output"
        );
    }
    if let Some(tool) = expected["tool"].as_object() {
        let (name, arguments) = message
            .content
            .iter()
            .find_map(|c| match c {
                Content::ToolCall {
                    name, arguments, ..
                } => Some((name, arguments)),
                _ => None,
            })
            .expect("tool call in terminal message");
        assert_eq!(name, tool["name"].as_str().unwrap(), "tool name");
        assert_eq!(arguments, &tool["arguments"], "tool arguments");
    }
}

async fn run_case(kind: ApiKind, case: &str, hold_open: bool, cancel: bool) {
    let api_name = match kind {
        ApiKind::AnthropicMessages => "anthropic",
        ApiKind::OpenaiCompletions => "openai",
        other => panic!("no fixture dir for {other:?}"),
    };
    let events = replay(kind, case, hold_open, cancel).await;
    assert_expected(&events, api_name, case);
}

#[tokio::test]
async fn anthropic_text_flow() {
    run_case(ApiKind::AnthropicMessages, "text_flow", false, false).await;
}

#[tokio::test]
async fn anthropic_tool_call() {
    run_case(ApiKind::AnthropicMessages, "tool_call", false, false).await;
}

#[tokio::test]
async fn anthropic_error_event() {
    run_case(ApiKind::AnthropicMessages, "error_event", false, false).await;
}

#[tokio::test]
async fn anthropic_cancel_midstream() {
    run_case(ApiKind::AnthropicMessages, "cancel_midstream", true, true).await;
}

#[tokio::test]
async fn openai_text_flow() {
    run_case(ApiKind::OpenaiCompletions, "text_flow", false, false).await;
}

#[tokio::test]
async fn openai_tool_call() {
    run_case(ApiKind::OpenaiCompletions, "tool_call", false, false).await;
}

#[tokio::test]
async fn openai_error_stream() {
    run_case(ApiKind::OpenaiCompletions, "error_stream", false, false).await;
}

#[tokio::test]
async fn openai_cancel_midstream() {
    run_case(ApiKind::OpenaiCompletions, "cancel_midstream", true, true).await;
}
