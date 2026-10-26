//! anthropic 消息转换：统一 Message → 协议消息数组（设计 01 §4）。
//!
//! 连续 tool_result 合并为一条 user 消息（Anthropic 要求 tool_result 紧跟 tool_use）。

use serde_json::{Map, Value, json};

use crate::types::{Content, Context, Message};

/// 消息数组转换（mid-convo system 并入文本提示由循环保证不出现；此处防御性折叠为 user 文本）。
#[must_use]
pub fn messages_to_api(context: &Context) -> Vec<Value> {
    let mut out: Vec<Value> = vec![];
    let mut pending_results: Vec<Value> = vec![];
    for msg in &context.messages {
        match msg {
            Message::System(s) => out.push(json!({"role": "user", "content": s.content})),
            Message::User(u) => {
                flush_results(&mut out, &mut pending_results);
                out.push(user_to_api(u.content.iter()));
            }
            Message::Assistant(a) => {
                flush_results(&mut out, &mut pending_results);
                out.push(assistant_to_api(a.content.iter()));
            }
            Message::ToolResult(t) => {
                pending_results.push(json!({
                    "type": "tool_result",
                    "tool_use_id": t.tool_call_id,
                    "content": content_blocks(t.content.iter()),
                    "is_error": t.is_error,
                }));
            }
        }
    }
    flush_results(&mut out, &mut pending_results);
    out
}

/// 工具声明：`{name, description, input_schema}` + 末工具 cache_control。
#[must_use]
pub fn tools_to_api(context: &Context) -> Value {
    let mut arr: Vec<Value> = context
        .tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "input_schema": super::request::tool_schema(&t.parameters),
            })
        })
        .collect();
    if let Some(last) = arr.last_mut() {
        last["cache_control"] = json!({"type": "ephemeral"});
    }
    Value::Array(arr)
}

fn flush_results(out: &mut Vec<Value>, pending: &mut Vec<Value>) {
    if pending.is_empty() {
        return;
    }
    out.push(json!({"role": "user", "content": std::mem::take(pending)}));
}

fn user_to_api<'a>(content: impl Iterator<Item = &'a Content>) -> Value {
    let blocks = content_blocks(content);
    // 纯文本折叠为字符串（pi 同构）
    if blocks.iter().all(|b| b.get("type") == Some(&json!("text"))) {
        let joined: Vec<&str> = blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect();
        return json!({"role": "user", "content": joined.join("")});
    }
    json!({"role": "user", "content": blocks})
}

fn assistant_to_api<'a>(content: impl Iterator<Item = &'a Content>) -> Value {
    let blocks: Vec<Value> = content
        .map(|c| match c {
            Content::Text { text } => json!({"type": "text", "text": text}),
            Content::Thinking {
                thinking,
                signature,
                redacted,
            } => match (redacted, signature) {
                (true, Some(data)) => json!({"type": "redacted_thinking", "data": data}),
                (false, Some(sig)) if !sig.is_empty() => {
                    json!({"type": "thinking", "thinking": thinking, "signature": sig})
                }
                // 缺签名：降级为文本块（pi 同构），供应商无法回放
                _ => json!({"type": "text", "text": thinking}),
            },
            Content::ToolCall {
                id,
                name,
                arguments,
                ..
            } => {
                json!({"type": "tool_use", "id": id, "name": name, "input": arguments})
            }
            Content::Image { .. } => json!({"type": "text", "text": "[image in assistant]"}),
        })
        .collect();
    json!({"role": "assistant", "content": blocks})
}

fn content_blocks<'a>(content: impl Iterator<Item = &'a Content>) -> Vec<Value> {
    content
        .map(|c| match c {
            Content::Text { text } => json!({"type": "text", "text": text}),
            Content::Image { data, mime_type } => super::request::image_source(mime_type, data),
            Content::Thinking { thinking, .. } => json!({"type": "text", "text": thinking}),
            Content::ToolCall {
                arguments,
                arguments_error,
                ..
            } => {
                let mut m = Map::new();
                m.insert("type".into(), json!("text"));
                m.insert(
                    "text".into(),
                    json!(
                        arguments_error
                            .clone()
                            .unwrap_or_else(|| format!("[tool call args: {arguments}]"))
                    ),
                );
                Value::Object(m)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AssistantMessage, ToolResultMessage, UserMessage};

    #[test]
    fn consecutive_tool_results_merge_into_one_user() {
        let ctx = Context {
            system_prompt: None,
            messages: vec![
                Message::ToolResult(tr("t1", false)),
                Message::ToolResult(tr("t2", true)),
                Message::Assistant(AssistantMessage::pending("a", "p", "m", 0)),
            ],
            tools: vec![],
            thinking_level: None,
        };
        let arr = messages_to_api(&ctx);
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["role"], "user");
        assert_eq!(arr[0]["content"].as_array().expect("blocks").len(), 2);
        assert_eq!(arr[0]["content"][1]["is_error"], json!(true));
    }

    #[test]
    fn text_only_user_collapses_to_string() {
        let ctx = Context {
            system_prompt: None,
            messages: vec![Message::User(UserMessage {
                content: vec![Content::Text { text: "hi".into() }],
                timestamp: 0,
            })],
            tools: vec![],
            thinking_level: None,
        };
        let arr = messages_to_api(&ctx);
        assert_eq!(arr[0]["content"], json!("hi"));
    }

    fn tr(id: &str, is_error: bool) -> ToolResultMessage {
        ToolResultMessage {
            tool_call_id: id.into(),
            tool_name: "read".into(),
            content: vec![Content::Text { text: "out".into() }],
            is_error,
            usage: None,
            duration_ms: None,
            timestamp: 0,
        }
    }
}
