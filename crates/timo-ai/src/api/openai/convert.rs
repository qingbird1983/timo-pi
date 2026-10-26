//! openai-completions 消息转换（设计 01 §5）。

use serde_json::{Value, json};

use crate::types::{Content, Message};

/// 统一消息数组 → chat.completions 消息数组。
#[must_use]
pub fn messages_to_api(messages: &[Message]) -> Vec<Value> {
    let mut out: Vec<Value> = vec![];
    for msg in messages {
        match msg {
            Message::System(s) => out.push(json!({"role": "system", "content": s.content})),
            Message::User(u) => out.push(user_to_api(u.content.iter())),
            Message::Assistant(a) => out.push(assistant_to_api(a.content.iter())),
            Message::ToolResult(t) => {
                let text: String = t
                    .content
                    .iter()
                    .map(|c| match c {
                        Content::Text { text } => text.clone(),
                        Content::Image { mime_type, .. } => format!("[image: {mime_type}]"),
                        _ => String::new(),
                    })
                    .collect::<Vec<_>>()
                    .join("");
                out.push(json!({
                    "role": "tool",
                    "tool_call_id": t.tool_call_id,
                    "content": text,
                }));
            }
        }
    }
    out
}

fn user_to_api<'a>(content: impl Iterator<Item = &'a Content>) -> Value {
    let parts: Vec<Value> = content
        .map(|c| match c {
            Content::Text { text } => json!({"type": "text", "text": text}),
            Content::Image { data, mime_type } => {
                json!({"type": "image_url",
                       "image_url": {"url": format!("data:{mime_type};base64,{data}")}})
            }
            Content::Thinking { thinking, .. } => json!({"type": "text", "text": thinking}),
            Content::ToolCall { .. } => json!({"type": "text", "text": "[tool call]"}),
        })
        .collect();
    if parts.len() == 1 && parts[0].get("type") == Some(&json!("text")) {
        return json!({"role": "user", "content": parts[0]["text"]});
    }
    json!({"role": "user", "content": parts})
}

fn assistant_to_api<'a>(content: impl Iterator<Item = &'a Content>) -> Value {
    let mut text = String::new();
    let mut tool_calls: Vec<Value> = vec![];
    for c in content {
        match c {
            Content::Text { text: t } => text.push_str(t),
            // 思考内容不回传（无有效签名载体；pi 对无 signature 同样丢弃）
            Content::Thinking { .. } => {}
            Content::ToolCall {
                id,
                name,
                arguments,
                ..
            } => tool_calls.push(json!({
                "id": id, "type": "function", "function": {
                    "name": name, "arguments": serde_json::to_string(arguments)
                        .unwrap_or_else(|_| "{}".into()),
                },
            })),
            Content::Image { .. } => {}
        }
    }
    let mut m = json!({"role": "assistant"});
    m["content"] = if text.is_empty() && !tool_calls.is_empty() {
        Value::Null
    } else {
        json!(text)
    };
    if !tool_calls.is_empty() {
        m["tool_calls"] = Value::Array(tool_calls);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AssistantMessage, ToolResultMessage, UserMessage};

    #[test]
    fn tool_result_maps_to_tool_role() {
        let msgs = vec![Message::ToolResult(ToolResultMessage {
            tool_call_id: "call_1".into(),
            tool_name: "read".into(),
            content: vec![Content::Text {
                text: "data".into(),
            }],
            is_error: false,
            usage: None,
            duration_ms: None,
            timestamp: 0,
        })];
        let arr = messages_to_api(&msgs);
        assert_eq!(arr[0]["role"], "tool");
        assert_eq!(arr[0]["tool_call_id"], "call_1");
        assert_eq!(arr[0]["content"], "data");
    }

    #[test]
    fn assistant_with_tool_calls() {
        let msgs = vec![Message::Assistant(crate::types::AssistantMessage {
            content: vec![
                Content::Text {
                    text: "let me".into(),
                },
                Content::ToolCall {
                    id: "call_1".into(),
                    name: "read".into(),
                    arguments: json!({"path": "a.txt"}),
                    arguments_error: None,
                },
            ],
            ..AssistantMessage::pending("openai-completions", "openai", "gpt-x", 0)
        })];
        let arr = messages_to_api(&msgs);
        assert_eq!(arr[0]["content"], json!("let me"));
        assert_eq!(arr[0]["tool_calls"][0]["function"]["name"], "read");
        assert_eq!(
            arr[0]["tool_calls"][0]["function"]["arguments"],
            json!(r#"{"path":"a.txt"}"#)
        );
    }

    #[test]
    fn user_text_single_part_collapses() {
        let msgs = vec![Message::User(UserMessage {
            content: vec![Content::Text { text: "hi".into() }],
            timestamp: 0,
        })];
        assert_eq!(messages_to_api(&msgs)[0]["content"], json!("hi"));
    }
}
