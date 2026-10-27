//! 无头 JSON 模式（phase-3.5/06 §4）：内核事件 → JSONL，逐行输出并立即 flush。
//!
//! 只做映射与写出，不持有内核状态；未知事件自然被忽略（向前兼容）。

use std::io::Write;

use serde_json::{Value, json};
use timo_kernel::{AssistantMessage, AssistantStreamEvent, Content, KernelEvent, Message};

/// JSONL 写出器（stdout）。
pub struct JsonWriter {
    out: std::io::Stdout,
}

impl JsonWriter {
    /// 新建（无缓冲；每行 flush）。
    #[must_use]
    pub fn new() -> Self {
        Self {
            out: std::io::stdout(),
        }
    }

    /// 首行会话头（恒为第一行）。
    pub fn header(&mut self, session_id: &str, cwd: &str, model: &str) {
        self.emit(json!({
            "type": "session_header",
            "v": 1,
            "session_id": session_id,
            "cwd": cwd,
            "model": model,
        }));
    }

    /// 映射并写出一条内核事件（映射不到的类型丢弃）。
    pub fn handle(&mut self, event: &KernelEvent) {
        if let Some(value) = mapped_event(event) {
            self.emit(value);
        }
    }

    /// 错误路径兜底：保证 JSON 流以 `agent_end` 收尾（只在缺失时调用）。
    pub fn end_without_events(&mut self) {
        self.emit(json!({"type": "agent_end", "messages": []}));
    }

    fn emit(&mut self, value: Value) {
        let _ = writeln!(self.out, "{value}");
        let _ = self.out.flush();
    }
}

/// 最终回答的正文字符串（只取 text 块，按顺序拼接）。
#[must_use]
pub fn assistant_text(message: &AssistantMessage) -> String {
    message
        .content
        .iter()
        .filter_map(|content| match content {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// 本次 run 的最后一条 assistant 消息。
#[must_use]
pub fn last_assistant(messages: &[Message]) -> Option<&AssistantMessage> {
    messages.iter().rev().find_map(|message| match message {
        Message::Assistant(assistant) => Some(assistant),
        _ => None,
    })
}

fn mapped_event(event: &KernelEvent) -> Option<Value> {
    let value = match event {
        KernelEvent::AgentStarted => json!({"type": "agent_start"}),
        KernelEvent::AgentFinished { messages } => {
            json!({"type": "agent_end", "messages": messages})
        }
        KernelEvent::TurnStarted => json!({"type": "turn_start"}),
        KernelEvent::TurnFinished {
            message,
            tool_results,
        } => json!({
            "type": "turn_end",
            "message": message,
            "tool_results": tool_results,
        }),
        KernelEvent::MessageAdded { message } => json!({"type": "message", "message": message}),
        KernelEvent::StreamEvent { event } => stream_event(event),
        KernelEvent::ToolCallRequested { call } => {
            json!({"type": "tool_call_requested", "call": call})
        }
        KernelEvent::ToolCallStarted {
            tool_call_id,
            tool_name,
            args,
        } => json!({
            "type": "tool_execution_start",
            "tool_call_id": tool_call_id,
            "tool_name": tool_name,
            "args": args,
        }),
        KernelEvent::ToolCallUpdate {
            tool_call_id,
            tool_name,
            partial,
        } => json!({
            "type": "tool_execution_update",
            "tool_call_id": tool_call_id,
            "tool_name": tool_name,
            "partial": {
                "content": partial.content,
                "details": partial.details,
                "is_error": partial.is_error,
            },
        }),
        KernelEvent::ToolCallFinished {
            tool_call_id,
            tool_name,
            result,
            duration_ms,
        } => json!({
            "type": "tool_execution_end",
            "tool_call_id": tool_call_id,
            "tool_name": tool_name,
            "result": result,
            "duration_ms": duration_ms,
        }),
        KernelEvent::ProviderRequest { provider, model } => json!({
            "type": "provider_request",
            "provider": provider,
            "model": model,
        }),
        KernelEvent::ProviderResponse {
            provider,
            model,
            usage,
        } => json!({
            "type": "provider_response",
            "provider": provider,
            "model": model,
            "usage": usage,
        }),
        KernelEvent::Error { context } => json!({"type": "error", "context": context}),
        KernelEvent::Compacted {
            reason,
            tokens_before,
            tokens_after,
        } => json!({
            "type": "compaction",
            "reason": reason.as_str(),
            "tokens_before": tokens_before,
            "tokens_after": tokens_after,
        }),
    };
    Some(value)
}

/// 流事件映射：`Start` 单独成 `message_start`，其余进 `message_update`
/// （`assistant_message_event` 不含 partial，工具块开头补 id / tool_name）。
fn stream_event(event: &AssistantStreamEvent) -> Value {
    match event {
        AssistantStreamEvent::Start { partial } => {
            json!({"type": "message_start", "message": partial})
        }
        AssistantStreamEvent::BlockStart { index, partial } => {
            let mut inner = json!({"type": "block_start", "index": index});
            if let Some(Content::ToolCall { id, name, .. }) = partial.content.get(*index) {
                inner["type"] = json!("toolcall_start");
                inner["id"] = json!(id);
                inner["tool_name"] = json!(name);
            }
            update_envelope(&partial.usage, inner)
        }
        AssistantStreamEvent::Delta {
            index,
            delta,
            partial,
        } => update_envelope(
            &partial.usage,
            json!({"type": "delta", "index": index, "delta": delta}),
        ),
        AssistantStreamEvent::BlockEnd { index, partial } => {
            update_envelope(&partial.usage, json!({"type": "block_end", "index": index}))
        }
        AssistantStreamEvent::Done { reason, message } => update_envelope(
            &message.usage,
            json!({"type": "done", "reason": reason, "message": message}),
        ),
        AssistantStreamEvent::Error { message } => {
            update_envelope(&message.usage, json!({"type": "error", "message": message}))
        }
    }
}

fn update_envelope(usage: &timo_kernel::Usage, event: Value) -> Value {
    json!({
        "type": "message_update",
        "usage": usage,
        "assistant_message_event": event,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use timo_kernel::{StopReason, Usage};

    fn assistant(content: Vec<Content>) -> AssistantMessage {
        let mut message = AssistantMessage::pending("openai-completions", "fake", "test", 7);
        message.content = content;
        message.stop_reason = StopReason::Stop;
        message
    }

    fn text(value: &str) -> Content {
        Content::Text { text: value.into() }
    }

    #[test]
    fn text_blocks_concatenate_in_order() {
        let message = assistant(vec![text("a"), text("b")]);
        assert_eq!(assistant_text(&message), "ab");
        let messages = vec![
            Message::User(timo_kernel::UserMessage {
                content: vec![text("q")],
                timestamp: 1,
            }),
            Message::Assistant(message),
        ];
        assert!(last_assistant(&messages).is_some());
    }

    #[test]
    fn start_event_is_message_start_without_partial_key() {
        let partial = assistant(vec![text("hi")]);
        let value = stream_event(&AssistantStreamEvent::Start { partial });
        assert_eq!(value["type"], "message_start");
        assert_eq!(value["message"]["content"][0]["text"], "hi");
        assert!(value.get("partial").is_none());
    }

    #[test]
    fn toolcall_block_start_carries_id_and_name() {
        let partial = assistant(vec![Content::ToolCall {
            id: "call_1".into(),
            name: "read".into(),
            arguments: json!({"path": "a.txt"}),
            arguments_error: None,
        }]);
        let value = stream_event(&AssistantStreamEvent::BlockStart { index: 0, partial });
        assert_eq!(value["type"], "message_update");
        assert_eq!(value["assistant_message_event"]["type"], "toolcall_start");
        assert_eq!(value["assistant_message_event"]["id"], "call_1");
        assert_eq!(value["assistant_message_event"]["tool_name"], "read");
        assert!(value["assistant_message_event"].get("partial").is_none());
    }

    #[test]
    fn delta_envelope_carries_usage_only() {
        let mut partial = assistant(vec![text("x")]);
        partial.usage = Usage {
            input: 3,
            output: 2,
            ..Usage::default()
        };
        let value = stream_event(&AssistantStreamEvent::Delta {
            index: 0,
            delta: "x".into(),
            partial,
        });
        assert_eq!(value["usage"]["input"], 3);
        assert_eq!(value["assistant_message_event"]["delta"], "x");
    }

    #[test]
    fn tool_events_map_to_execution_names() {
        let started = mapped_event(&KernelEvent::ToolCallStarted {
            tool_call_id: "c1".into(),
            tool_name: "read".into(),
            args: std::sync::Arc::new(json!({"path": "a.txt"})),
        })
        .unwrap();
        assert_eq!(started["type"], "tool_execution_start");
        assert_eq!(started["args"]["path"], "a.txt");
        let finished = mapped_event(&KernelEvent::ToolCallFinished {
            tool_call_id: "c1".into(),
            tool_name: "read".into(),
            result: std::sync::Arc::new(Message::ToolResult(timo_kernel::ToolResultMessage {
                tool_call_id: "c1".into(),
                tool_name: "read".into(),
                content: vec![text("ok")],
                is_error: false,
                usage: None,
                duration_ms: Some(2),
                timestamp: 9,
            })),
            duration_ms: Some(2),
        })
        .unwrap();
        assert_eq!(finished["type"], "tool_execution_end");
        assert_eq!(finished["duration_ms"], 2);
    }
}
