//! 工具调用辅助：名字/参数/结果消息（run 循环与工具执行共用）。

use std::sync::Arc;

use timo_ai::{Content, Message, ToolResultMessage};

/// 工具名（防御式：非 ToolCall 为空串）。
pub(super) fn tool_name(call: &Content) -> String {
    match call {
        Content::ToolCall { name, .. } => name.clone(),
        _ => String::new(),
    }
}

/// 调用 id（防御式：非 ToolCall 为空串）。
pub(super) fn tool_id(call: &Content) -> String {
    match call {
        Content::ToolCall { id, .. } => id.clone(),
        _ => String::new(),
    }
}

/// 调用参数快照（防御式：非 ToolCall 为 Null）。
pub(super) fn tool_args(call: &Content) -> Arc<serde_json::Value> {
    match call {
        Content::ToolCall { arguments, .. } => Arc::new(arguments.clone()),
        _ => Arc::new(serde_json::Value::Null),
    }
}

/// 失败结果消息（is_error，模型可读）。
pub(super) fn error_result_message(
    id: &str,
    name: &str,
    reason: &str,
    duration_ms: Option<u64>,
    ts: i64,
) -> Message {
    Message::ToolResult(ToolResultMessage {
        tool_call_id: id.to_string(),
        tool_name: name.to_string(),
        content: vec![Content::Text {
            text: reason.to_string(),
        }],
        is_error: true,
        usage: None,
        duration_ms,
        timestamp: ts,
    })
}
