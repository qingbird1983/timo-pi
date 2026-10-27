#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use timo_ai::{AssistantMessage, AssistantMessageStream, AssistantStreamEvent};

/// 不发起真实请求的协议适配器。
struct FakeApi {
    reply: AssistantMessage,
}

#[async_trait::async_trait]
impl ProviderApi for FakeApi {
    async fn stream(
        &self,
        _model: &Model,
        _context: &Context,
        _options: StreamOptions,
    ) -> AssistantMessageStream {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let reply = self.reply.clone();
        tokio::spawn(async move {
            let _ = tx
                .send(AssistantStreamEvent::Done {
                    reason: reply.stop_reason,
                    message: reply,
                })
                .await;
        });
        AssistantMessageStream::new(rx)
    }
}

fn model() -> Model {
    serde_json::from_value(serde_json::json!({
        "id": "test", "name": "Test", "provider": "fake", "api": "openai-completions",
        "base_url": "http://localhost", "reasoning": false, "input": ["text"],
        "cost": {}, "context_window": 100000, "max_tokens": 1000
    }))
    .unwrap()
}

fn reply(text: &str) -> AssistantMessage {
    let mut m = AssistantMessage::pending("openai-completions", "fake", "test", 1);
    m.stop_reason = StopReason::Stop;
    m.content = vec![Content::Text {
        text: text.to_string(),
    }];
    m.usage.total_tokens = 12;
    m
}

fn auth() -> Arc<timo_ai::AuthResolver> {
    Arc::new(timo_ai::AuthResolver {
        explicit: Some("k".into()),
        provider_ref: None,
        env_names: vec![],
    })
}

fn settings(keep: u64) -> CompactionSettings {
    CompactionSettings {
        keep_recent_tokens: keep,
        ..Default::default()
    }
}

fn conversation() -> Vec<Message> {
    vec![
        user_text("a".repeat(400).as_str(), 1),
        Message::Assistant(reply("first answer")),
        user_text("b".repeat(400).as_str(), 2),
        Message::Assistant(reply("second answer")),
        user_text("latest", 3),
    ]
}

#[test]
fn estimate_and_threshold() {
    assert_eq!(estimate_tokens(&user_text("12345678", 0)), 2);
    assert!(should_compact(90_000, 100_000, &settings(20_000)));
    assert!(!should_compact(80_000, 100_000, &settings(20_000)));
    assert!(!should_compact(90_000, 0, &settings(20_000)));
}

#[test]
fn cut_walks_back_to_a_user_message() {
    let messages = conversation();
    let cut = cut_index(&messages, 20).unwrap();
    assert_eq!(cut, 2);
    assert!(matches!(messages[cut], Message::User(_)));
    assert!(cut_index(&messages[..1], 1).is_none());
}

#[test]
fn serialization_skips_thinking_and_truncates_tool_results() {
    let messages = vec![
        user_text("hi", 1),
        Message::Assistant(reply("ok")),
        Message::ToolResult(timo_ai::ToolResultMessage {
            tool_call_id: "c".into(),
            tool_name: "read".into(),
            content: vec![Content::Text {
                text: "x".repeat(2500),
            }],
            is_error: false,
            usage: None,
            duration_ms: None,
            timestamp: 2,
        }),
    ];
    let text = serialize_conversation(&messages);
    assert!(text.contains("[User]: hi"));
    assert!(text.contains("[Assistant]: ok"));
    assert!(text.contains("more characters truncated"));
}

#[test]
fn file_operations_are_deduplicated_and_sorted() {
    let messages = vec![Message::Assistant(AssistantMessage {
        content: vec![
            Content::ToolCall {
                id: "1".into(),
                name: "read".into(),
                arguments: serde_json::json!({"path": "b.txt"}),
                arguments_error: None,
            },
            Content::ToolCall {
                id: "2".into(),
                name: "read".into(),
                arguments: serde_json::json!({"path": "a.txt"}),
                arguments_error: None,
            },
            Content::ToolCall {
                id: "3".into(),
                name: "edit".into(),
                arguments: serde_json::json!({"path": "b.txt"}),
                arguments_error: None,
            },
        ],
        ..AssistantMessage::pending("openai-completions", "fake", "test", 0)
    })];
    let (read, modified) = file_operations(&messages);
    assert_eq!(read, vec!["a.txt"]);
    assert_eq!(modified, vec!["b.txt"]);
}

#[tokio::test]
async fn compact_returns_summary_plus_kept_tail() {
    let mut messages = conversation();
    // 末尾 assistant 带压缩前的 usage 读数：tokens_before 用它当基线，
    // tokens_after 必须重新估算，不能沿用这颗过期数字。
    let mut measured = reply("third answer");
    measured.usage.total_tokens = 5000;
    messages.push(Message::Assistant(measured));
    let api: Arc<dyn ProviderApi> = Arc::new(FakeApi {
        reply: reply("## Goal\nship it"),
    });
    let clock: Arc<dyn Clock> = Arc::new(timo_config::FixedClock::new(9));
    let outcome = compact(CompactInput {
        messages: &messages,
        model: &model(),
        api: &api,
        auth: &auth(),
        retry: &RetrySettings::default(),
        thinking: ThinkingLevel::Off,
        settings: &settings(20),
        cancel: &CancellationToken::new(),
        clock: &clock,
        instructions: Some("focus on tests"),
    })
    .await
    .unwrap();
    assert!(outcome.summary.contains("## Goal"));
    assert_eq!(outcome.keep_recent_messages, 4);
    assert_eq!(outcome.messages.len(), 5);
    assert_eq!(
        timo_session::extract_summary(&outcome.messages[0]),
        Some(outcome.summary.as_str())
    );
    assert_eq!(outcome.tokens_before, 5000);
    let estimated: u64 = outcome.messages.iter().map(estimate_tokens).sum();
    assert_eq!(outcome.tokens_after, estimated);
    assert!(
        outcome.tokens_after < 1000,
        "after = {}",
        outcome.tokens_after
    );
}

#[tokio::test]
async fn length_stop_is_a_failure() {
    let messages = conversation();
    let mut stop = reply("partial");
    stop.stop_reason = StopReason::Length;
    let api: Arc<dyn ProviderApi> = Arc::new(FakeApi { reply: stop });
    let clock: Arc<dyn Clock> = Arc::new(timo_config::FixedClock::new(9));
    let err = compact(CompactInput {
        messages: &messages,
        model: &model(),
        api: &api,
        auth: &auth(),
        retry: &RetrySettings::default(),
        thinking: ThinkingLevel::Off,
        settings: &settings(20),
        cancel: &CancellationToken::new(),
        clock: &clock,
        instructions: None,
    })
    .await
    .unwrap_err();
    assert!(matches!(err, CompactionError::Failed(_)));
}
