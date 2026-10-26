//! 会话格式兼容 fixtures（设计 07 §3）：v1 线性 / 工具 / 未知型 / 半行截断。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use timo_ai::{Content, Message};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/session")
        .join(name)
}

fn text_of(message: &Message) -> String {
    let content = match message {
        Message::System(m) => return m.content.clone(),
        Message::User(m) => &m.content,
        Message::Assistant(m) => &m.content,
        Message::ToolResult(m) => &m.content,
    };
    content
        .iter()
        .filter_map(|c| match c {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn linear_projection_is_stable() {
    let entries = timo_session::read_entries(&fixture("v1-linear.jsonl")).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].kind, timo_session::kinds::HEADER);
    let messages = timo_session::project_context(&entries);
    assert_eq!(messages.len(), 2);
    assert_eq!(text_of(&messages[0]), "hello");
    assert_eq!(text_of(&messages[1]), "hi there");
    assert!(matches!(messages[1], Message::Assistant(_)));
}

#[test]
fn tools_projection_keeps_call_and_result() {
    let entries = timo_session::read_entries(&fixture("v1-tools.jsonl")).unwrap();
    assert_eq!(entries.len(), 5);
    let messages = timo_session::project_context(&entries);
    assert_eq!(messages.len(), 4);
    match &messages[1] {
        Message::Assistant(a) => match &a.content[0] {
            Content::ToolCall {
                id,
                name,
                arguments,
                ..
            } => {
                assert_eq!(id, "toolu_1");
                assert_eq!(name, "read");
                assert_eq!(arguments["path"], "a.txt");
            }
            other => panic!("tool call expected: {other:?}"),
        },
        other => panic!("assistant expected: {other:?}"),
    }
    match &messages[2] {
        Message::ToolResult(r) => {
            assert_eq!(r.tool_call_id, "toolu_1");
            assert_eq!(text_of(&messages[2]), "content of a");
            assert!(!r.is_error);
        }
        other => panic!("tool result expected: {other:?}"),
    }
    assert_eq!(text_of(&messages[3]), "done");
}

#[test]
fn unknown_kind_is_preserved_but_not_projected() {
    let entries = timo_session::read_entries(&fixture("v1-unknown-type.jsonl")).unwrap();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[1].kind, "custom.demo");
    assert_eq!(entries[1].payload["n"], 1);
    let messages = timo_session::project_context(&entries);
    assert_eq!(messages.len(), 2);
    assert_eq!(text_of(&messages[0]), "still works");
    assert_eq!(text_of(&messages[1]), "yes");
}

#[test]
fn truncated_last_line_is_dropped() {
    let entries = timo_session::read_entries(&fixture("v1-truncated-last.jsonl")).unwrap();
    assert_eq!(entries.len(), 2, "half-written last entry is discarded");
    let messages = timo_session::project_context(&entries);
    assert_eq!(messages.len(), 1);
    assert_eq!(text_of(&messages[0]), "before the crash");
}
