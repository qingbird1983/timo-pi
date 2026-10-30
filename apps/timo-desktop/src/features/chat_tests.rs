#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::{ChatState, RowKind, TraceStatus};
use std::sync::Arc;
use timo_kernel::{
    AssistantMessage, AssistantStreamEvent, Content, KernelEvent, Message, StopReason,
    ToolResultMessage, Usage, UserMessage,
};

fn assistant(content: Vec<Content>) -> Message {
    let mut message = AssistantMessage::pending("openai", "deepseek", "deepseek-chat", 0);
    message.content = content;
    message.stop_reason = StopReason::Stop;
    Message::Assistant(message)
}

fn tool_call(id: &str, name: &str, args: serde_json::Value) -> Content {
    Content::ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: args,
        arguments_error: None,
    }
}

fn tool_result(id: &str, text: &str, is_error: bool) -> Message {
    Message::ToolResult(ToolResultMessage {
        tool_call_id: id.into(),
        tool_name: "read".into(),
        content: vec![Content::Text { text: text.into() }],
        is_error,
        usage: None,
        duration_ms: Some(12),
        timestamp: 0,
    })
}

#[test]
fn stream_updates_assistant_text() {
    let mut chat = ChatState::default();
    chat.apply(&KernelEvent::AgentStarted);
    assert!(chat.running);

    let partial = AssistantMessage::pending("openai", "deepseek", "m", 0);
    chat.apply(&KernelEvent::TurnStarted);
    chat.apply(&KernelEvent::StreamEvent {
        event: Arc::new(AssistantStreamEvent::Start {
            partial: partial.clone(),
        }),
    });
    let mut partial = partial;
    partial.content = vec![Content::Text {
        text: "hello".into(),
    }];
    chat.apply(&KernelEvent::StreamEvent {
        event: Arc::new(AssistantStreamEvent::Delta {
            index: 0,
            delta: "hello".into(),
            partial,
        }),
    });

    assert_eq!(chat.rows.len(), 1);
    match &chat.rows[0].kind {
        RowKind::Assistant(row) => {
            assert_eq!(row.text, "hello");
            assert!(row.streaming);
        }
        other => panic!("unexpected row: {other:?}"),
    }
}

#[test]
fn codemode_progress_updates_accumulate_in_the_trace() {
    let mut chat = ChatState::default();
    chat.apply(&KernelEvent::AgentStarted);
    chat.apply(&KernelEvent::TurnStarted);
    chat.apply(&KernelEvent::ToolCallStarted {
        tool_call_id: "c1".into(),
        tool_name: "codemode".into(),
        args: Arc::new(serde_json::json!({"code": "await tools.bash({});"})),
    });
    for line in ["→ tools.bash", "← bash ok"] {
        chat.apply(&KernelEvent::ToolCallUpdate {
            tool_call_id: "c1".into(),
            tool_name: "codemode".into(),
            partial: Arc::new(timo_kernel::ToolOutput::text(line)),
        });
    }
    let RowKind::Assistant(row) = &chat.rows[0].kind else {
        panic!("expected assistant row");
    };
    assert_eq!(row.trace.len(), 1);
    assert_eq!(row.trace[0].status, TraceStatus::Running);
    assert_eq!(
        row.trace[0].output.as_deref(),
        Some("→ tools.bash\n← bash ok")
    );
}

#[test]
fn tool_lifecycle_merges_into_one_trace_entry() {
    let mut chat = ChatState::default();
    chat.apply(&KernelEvent::AgentStarted);
    chat.apply(&KernelEvent::TurnStarted);
    chat.apply(&KernelEvent::ToolCallRequested {
        call: Arc::new(tool_call(
            "c1",
            "read",
            serde_json::json!({ "path": "a.txt" }),
        )),
    });
    chat.apply(&KernelEvent::ToolCallStarted {
        tool_call_id: "c1".into(),
        tool_name: "read".into(),
        args: Arc::new(serde_json::json!({ "path": "a.txt" })),
    });
    chat.apply(&KernelEvent::ToolCallFinished {
        tool_call_id: "c1".into(),
        tool_name: "read".into(),
        result: Arc::new(tool_result("c1", "file body", true)),
        duration_ms: Some(12),
    });

    let RowKind::Assistant(row) = &chat.rows[0].kind else {
        panic!("expected assistant row");
    };
    assert_eq!(row.trace.len(), 1);
    assert_eq!(row.trace[0].status, TraceStatus::Failed);
    assert_eq!(row.trace[0].output.as_deref(), Some("file body"));
    assert_eq!(row.trace[0].duration_ms, Some(12));
    assert!(row.trace_open);

    chat.apply(&KernelEvent::AgentFinished {
        messages: Arc::new(vec![]),
    });
    assert!(!chat.running);
    let RowKind::Assistant(row) = &chat.rows[0].kind else {
        panic!("expected assistant row");
    };
    assert!(!row.trace_open);
}

#[test]
fn turn_finished_sets_final_text_and_usage() {
    let mut chat = ChatState::default();
    chat.apply(&KernelEvent::AgentStarted);
    chat.apply(&KernelEvent::TurnStarted);
    chat.apply(&KernelEvent::TurnFinished {
        message: Arc::new(assistant(vec![Content::Text {
            text: "final".into(),
        }])),
        tool_results: Arc::new(vec![]),
    });
    chat.apply(&KernelEvent::ProviderResponse {
        provider: "deepseek".into(),
        model: "deepseek-chat".into(),
        usage: Usage {
            input: 10,
            output: 5,
            total_tokens: 15,
            ..Usage::default()
        },
    });
    chat.apply(&KernelEvent::Error {
        context: "boom".into(),
    });

    let RowKind::Assistant(row) = &chat.rows[0].kind else {
        panic!("expected assistant row");
    };
    assert_eq!(row.text, "final");
    assert!(!row.streaming);
    assert_eq!(chat.usage.as_ref().map(|u| u.total_tokens), Some(15));
    assert_eq!(chat.last_error.as_deref(), Some("boom"));
}

#[test]
fn history_pairs_tool_results_and_marks_orphans() {
    let mut chat = ChatState::default();
    chat.load_history(&[
        Message::User(UserMessage {
            content: vec![Content::Text { text: "hi".into() }],
            timestamp: 0,
        }),
        assistant(vec![
            Content::Text {
                text: "checking".into(),
            },
            tool_call("c1", "read", serde_json::json!({ "path": "a" })),
            tool_call("c2", "read", serde_json::json!({ "path": "b" })),
        ]),
        tool_result("c1", "ok", false),
    ]);

    assert_eq!(chat.rows.len(), 2);
    let RowKind::Assistant(row) = &chat.rows[1].kind else {
        panic!("expected assistant row");
    };
    assert_eq!(row.text, "checking");
    assert_eq!(row.trace.len(), 2);
    assert_eq!(row.trace[0].status, TraceStatus::Done);
    assert_eq!(row.trace[1].status, TraceStatus::Interrupted);
}

#[test]
fn reset_clears_rows_and_state() {
    let mut chat = ChatState::default();
    chat.push_user("hi");
    chat.running = true;
    chat.last_error = Some("x".into());
    chat.reset();
    assert!(chat.rows.is_empty());
    assert!(!chat.running);
    assert!(chat.last_error.is_none());
}

#[test]
fn latest_tool_calls_keeps_the_most_recent_per_tool() {
    let mut chat = ChatState::default();
    chat.apply(&KernelEvent::AgentStarted);
    chat.apply(&KernelEvent::TurnStarted);
    chat.apply(&KernelEvent::ToolCallRequested {
        call: Arc::new(tool_call("c1", "read", serde_json::json!({ "path": "a" }))),
    });
    chat.apply(&KernelEvent::ToolCallFinished {
        tool_call_id: "c1".into(),
        tool_name: "read".into(),
        result: Arc::new(tool_result("c1", "first", false)),
        duration_ms: Some(3),
    });
    chat.apply(&KernelEvent::ToolCallRequested {
        call: Arc::new(tool_call(
            "c2",
            "grep",
            serde_json::json!({ "pattern": "x" }),
        )),
    });
    chat.apply(&KernelEvent::ToolCallRequested {
        call: Arc::new(tool_call("c3", "read", serde_json::json!({ "path": "b" }))),
    });

    let calls = chat.latest_tool_calls();
    assert_eq!(calls.len(), 2);
    let read = calls.iter().find(|call| call.name == "read").unwrap();
    assert_eq!(read.status, "queued");
    assert!(read.input.contains("\"path\":\"b\""), "{}", read.input);
    let grep = calls.iter().find(|call| call.name == "grep").unwrap();
    assert_eq!(grep.status, "queued");
    assert!(grep.output.is_empty());
}
