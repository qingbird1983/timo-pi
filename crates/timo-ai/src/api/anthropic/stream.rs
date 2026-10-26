//! anthropic SSE → 流事件状态机（设计 01 §4 映射表）。

use std::collections::HashMap;

use serde_json::Value;

use crate::api::sse::SseEvent;
use crate::catalog::{RateCard, calculate_cost};
use crate::errors::AiError;
use crate::stream::AssistantStreamEvent;
use crate::types::{AssistantMessage, Content, StopReason, Usage};

/// 事件驱动的流状态：维护 live partial 与 anthropic 块下标映射。
pub(crate) struct AnthropicStream {
    msg: AssistantMessage,
    block_map: HashMap<i64, usize>,
    args_raw: HashMap<usize, String>,
    stop_seen: bool,
    stop_reason: StopReason,
    raw_stop: Option<String>,
    cost: RateCard,
}

impl AnthropicStream {
    /// 以 pending 基底消息与费率卡初始化。
    pub(crate) fn new(base: AssistantMessage, cost: RateCard) -> Self {
        Self {
            msg: base,
            block_map: HashMap::new(),
            args_raw: HashMap::new(),
            stop_seen: false,
            stop_reason: StopReason::Pending,
            raw_stop: None,
            cost,
        }
    }

    /// 当前 partial 快照。
    pub(crate) fn partial(&self) -> &AssistantMessage {
        &self.msg
    }

    /// 消费一条 SSE 事件；产出 0~1 个流事件。
    ///
    /// # Errors
    /// JSON 解析失败 / `event: error` / 未知 stop_reason → [`AiError::Sse`]/[`AiError::Json`]。
    pub(crate) fn on_sse(
        &mut self,
        sse: &SseEvent,
    ) -> Result<Option<AssistantStreamEvent>, AiError> {
        if sse.event.as_deref() == Some("error") {
            return Err(AiError::Sse(format!("provider error event: {}", sse.data)));
        }
        let v: Value = serde_json::from_str(&sse.data).map_err(|e| AiError::Json {
            context: "sse payload".into(),
            detail: e.to_string(),
        })?;
        match v.get("type").and_then(Value::as_str) {
            Some("message_start") => self.on_message_start(&v),
            Some("content_block_start") => self.on_block_start(&v),
            Some("content_block_delta") => self.on_block_delta(&v),
            Some("content_block_stop") => self.on_block_stop(&v),
            Some("message_delta") => self.on_message_delta(&v),
            Some("message_stop") => {
                self.stop_seen = true;
                Ok(None)
            }
            _ => Ok(None), // ping 及未知事件忽略
        }
    }

    /// 流收尾：校验 message_stop 与 stop_reason，产出 Done 终事件。
    ///
    /// # Errors
    /// 未收到 message_stop / stop_reason 仍为 Pending → [`AiError::Sse`]。
    pub(crate) fn finish(mut self) -> Result<AssistantMessage, AiError> {
        if !self.stop_seen {
            return Err(AiError::Sse("stream ended before message_stop".into()));
        }
        if self.stop_reason == StopReason::Pending {
            return Err(AiError::Sse("message_stop without stop_reason".into()));
        }
        self.msg.stop_reason = self.stop_reason;
        self.msg.raw_stop_reason = self.raw_stop.take();
        Ok(self.msg)
    }

    fn on_message_start(&mut self, v: &Value) -> Result<Option<AssistantStreamEvent>, AiError> {
        let m = v
            .get("message")
            .ok_or_else(|| sse_err("message_start missing message"))?;
        self.msg.response_id = m.get("id").and_then(Value::as_str).map(String::from);
        if let Some(rm) = m.get("model").and_then(Value::as_str)
            && rm != self.msg.model
        {
            self.msg.response_model = Some(rm.to_string());
        }
        let mut usage = Usage::default();
        if let Some(u) = m.get("usage") {
            usage = parse_usage(u);
        }
        usage.total_tokens = usage.input + usage.output + usage.cache_read + usage.cache_write;
        usage.cost = calculate_cost(&self.cost, &usage);
        self.msg.usage = usage;
        Ok(None)
    }

    fn on_block_start(&mut self, v: &Value) -> Result<Option<AssistantStreamEvent>, AiError> {
        let idx = v
            .get("index")
            .and_then(Value::as_i64)
            .ok_or_else(|| sse_err("block index"))?;
        let block = v
            .get("content_block")
            .ok_or_else(|| sse_err("content_block missing"))?;
        let local = self.msg.content.len();
        let content = match block.get("type").and_then(Value::as_str) {
            Some("text") => Content::Text {
                text: String::new(),
            },
            Some("thinking") => Content::Thinking {
                thinking: block
                    .get("thinking")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                signature: block
                    .get("signature")
                    .and_then(Value::as_str)
                    .map(String::from),
                redacted: false,
            },
            Some("redacted_thinking") => Content::Thinking {
                thinking: "[Reasoning redacted]".into(),
                signature: block.get("data").and_then(Value::as_str).map(String::from),
                redacted: true,
            },
            Some("tool_use") => Content::ToolCall {
                id: normalize_tool_call_id(
                    block.get("id").and_then(Value::as_str).unwrap_or_default(),
                ),
                name: block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into(),
                arguments: block
                    .get("input")
                    .cloned()
                    .unwrap_or_else(|| Value::Object(serde_json::Map::new())),
                arguments_error: None,
            },
            other => {
                return Err(AiError::Sse(format!(
                    "unknown content_block type {other:?}"
                )));
            }
        };
        if matches!(content, Content::ToolCall { .. }) {
            self.args_raw.insert(local, String::new());
        }
        self.msg.content.push(content);
        self.block_map.insert(idx, local);
        Ok(Some(AssistantStreamEvent::BlockStart {
            index: local,
            partial: self.msg.clone(),
        }))
    }

    fn on_block_delta(&mut self, v: &Value) -> Result<Option<AssistantStreamEvent>, AiError> {
        let idx = v
            .get("index")
            .and_then(Value::as_i64)
            .ok_or_else(|| sse_err("delta index"))?;
        let delta = v.get("delta").ok_or_else(|| sse_err("delta missing"))?;
        let local = *self
            .block_map
            .get(&idx)
            .ok_or_else(|| sse_err("delta before start"))?;
        match delta.get("type").and_then(Value::as_str) {
            Some("text_delta") => self.append_text(local, delta, "text"),
            Some("thinking_delta") => self.append_text(local, delta, "thinking"),
            Some("input_json_delta") => {
                let frag = delta
                    .get("partial_json")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                self.args_raw.entry(local).or_default().push_str(frag);
                Ok(Some(AssistantStreamEvent::Delta {
                    index: local,
                    delta: frag.to_string(),
                    partial: self.msg.clone(),
                }))
            }
            Some("signature_delta") => {
                if let Some(Content::Thinking { signature, .. }) = self.msg.content.get_mut(local) {
                    let sig = signature.get_or_insert_with(String::new);
                    sig.push_str(delta.get("signature").and_then(Value::as_str).unwrap_or(""));
                }
                Ok(None)
            }
            other => Err(AiError::Sse(format!("unknown delta type {other:?}"))),
        }
    }

    fn append_text(
        &mut self,
        local: usize,
        delta: &Value,
        field: &str,
    ) -> Result<Option<AssistantStreamEvent>, AiError> {
        let frag = delta.get(field).and_then(Value::as_str).unwrap_or("");
        let emitted = match self.msg.content.get_mut(local) {
            Some(Content::Text { text }) => {
                text.push_str(frag);
                Some(frag)
            }
            Some(Content::Thinking { thinking, .. }) => {
                thinking.push_str(frag);
                Some(frag)
            }
            _ => None,
        };
        Ok(emitted.map(|f| AssistantStreamEvent::Delta {
            index: local,
            delta: f.to_string(),
            partial: self.msg.clone(),
        }))
    }

    fn on_block_stop(&mut self, v: &Value) -> Result<Option<AssistantStreamEvent>, AiError> {
        let idx = v
            .get("index")
            .and_then(Value::as_i64)
            .ok_or_else(|| sse_err("stop index"))?;
        let local = *self
            .block_map
            .get(&idx)
            .ok_or_else(|| sse_err("stop before start"))?;
        if let Some(raw) = self.args_raw.remove(&local)
            && let Some(Content::ToolCall {
                arguments,
                arguments_error,
                ..
            }) = self.msg.content.get_mut(local)
        {
            match serde_json::from_str::<Value>(&raw) {
                Ok(parsed) => *arguments = parsed,
                Err(e) => {
                    *arguments = Value::Object(serde_json::Map::new());
                    *arguments_error = Some(e.to_string());
                }
            }
        }
        Ok(Some(AssistantStreamEvent::BlockEnd {
            index: local,
            partial: self.msg.clone(),
        }))
    }

    fn on_message_delta(&mut self, v: &Value) -> Result<Option<AssistantStreamEvent>, AiError> {
        let delta = v
            .get("delta")
            .ok_or_else(|| sse_err("message_delta missing delta"))?;
        if let Some(sr) = delta.get("stop_reason").and_then(Value::as_str) {
            self.raw_stop = Some(sr.to_string());
            self.stop_reason = map_stop_reason(sr)?;
        }
        if let Some(u) = v.get("usage") {
            merge_usage(&mut self.msg.usage, u);
            let usage = &mut self.msg.usage;
            usage.total_tokens = usage.input + usage.output + usage.cache_read + usage.cache_write;
            usage.cost = calculate_cost(&self.cost, usage);
        }
        Ok(None)
    }
}

/// stop_reason 映射（pi mapStopReason；未知 → 错误）。
fn map_stop_reason(raw: &str) -> Result<StopReason, AiError> {
    Ok(match raw {
        "end_turn" | "pause_turn" | "stop_sequence" => StopReason::Stop,
        "max_tokens" => StopReason::Length,
        "tool_use" => StopReason::ToolUse,
        "refusal" | "sensitive" => StopReason::Error,
        other => return Err(AiError::Sse(format!("unknown stop_reason: {other}"))),
    })
}

fn parse_usage(u: &Value) -> Usage {
    Usage {
        input: num(u, "input_tokens"),
        output: num(u, "output_tokens"),
        cache_read: num(u, "cache_read_input_tokens"),
        cache_write: num(u, "cache_creation_input_tokens"),
        cache_write_1h: u
            .pointer("/cache_creation/ephemeral_1h_input_tokens")
            .and_then(Value::as_u64),
        reasoning: u
            .pointer("/output_tokens_details/thinking_tokens")
            .and_then(Value::as_u64),
        ..Usage::default()
    }
}

fn merge_usage(target: &mut Usage, u: &Value) {
    // 代理可能省略 input 字段：非空才覆盖（pi 同构）
    let fresh = parse_usage(u);
    if u.get("input_tokens").is_some() {
        target.input = fresh.input;
    }
    if u.get("output_tokens").is_some() {
        target.output = fresh.output;
    }
    if u.get("cache_read_input_tokens").is_some() {
        target.cache_read = fresh.cache_read;
    }
    if u.get("cache_creation_input_tokens").is_some() {
        target.cache_write = fresh.cache_write;
    }
    if fresh.cache_write_1h.is_some() {
        target.cache_write_1h = fresh.cache_write_1h;
    }
    if fresh.reasoning.is_some() {
        target.reasoning = fresh.reasoning;
    }
}

fn num(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// 工具 id 规范化：`[^A-Za-z0-9_-]→_`，截 64（pi normalizeToolCallId）。
#[must_use]
pub fn normalize_tool_call_id(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    cleaned.chars().take(64).collect()
}

fn sse_err(what: &str) -> AiError {
    AiError::Sse(format!("malformed sse: {what}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn ev(event: &str, data: &str) -> SseEvent {
        SseEvent {
            event: Some(event.into()),
            data: data.into(),
        }
    }

    fn state() -> AnthropicStream {
        AnthropicStream::new(
            AssistantMessage::pending("anthropic-messages", "anthropic", "m", 0),
            RateCard::default(),
        )
    }

    #[test]
    fn full_text_flow() {
        let mut s = state();
        s.on_sse(&ev("message_start", r#"{"type":"message_start","message":{"id":"msg_1","model":"m","usage":{"input_tokens":10,"output_tokens":1}}}"#)).unwrap();
        assert_eq!(s.partial().usage.input, 10);
        let start = s.on_sse(&ev("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#)).unwrap().unwrap();
        let AssistantStreamEvent::BlockStart { index, .. } = start else {
            panic!("start")
        };
        assert_eq!(index, 0);
        let delta = s.on_sse(&ev("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"he"}}"#)).unwrap().unwrap();
        let AssistantStreamEvent::Delta { delta: d, .. } = delta else {
            panic!("delta")
        };
        assert_eq!(d, "he");
        s.on_sse(&ev(
            "content_block_stop",
            r#"{"type":"content_block_stop","index":0}"#,
        ))
        .unwrap();
        s.on_sse(&ev("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}"#)).unwrap();
        s.on_sse(&ev("message_stop", r#"{"type":"message_stop"}"#))
            .unwrap();
        let fin = s.finish().unwrap();
        assert_eq!(fin.stop_reason, StopReason::Stop);
        assert_eq!(fin.usage.output, 3);
        assert_eq!(fin.usage.total_tokens, 13);
        match &fin.content[0] {
            Content::Text { text } => assert_eq!(text, "he"),
            other => panic!("text expected, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_args_parse_and_error() {
        let mut s = state();
        s.on_sse(&ev("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"read","input":{}}}"#)).unwrap();
        s.on_sse(&ev("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"pa"}}"#)).unwrap();
        s.on_sse(&ev("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"th\":\"a\"}"}}"#)).unwrap();
        s.on_sse(&ev(
            "content_block_stop",
            r#"{"type":"content_block_stop","index":0}"#,
        ))
        .unwrap();
        s.on_sse(&ev(
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"}}"#,
        ))
        .unwrap();
        s.on_sse(&ev("message_stop", r#"{"type":"message_stop"}"#))
            .unwrap();
        let fin = s.finish().unwrap();
        assert_eq!(fin.stop_reason, StopReason::ToolUse);
        match &fin.content[0] {
            Content::ToolCall {
                arguments,
                arguments_error,
                ..
            } => {
                assert_eq!(arguments["path"], "a");
                assert!(arguments_error.is_none());
            }
            other => panic!("toolcall expected, got {other:?}"),
        }
    }

    #[test]
    fn missing_message_stop_is_error() {
        let mut s = state();
        s.on_sse(&ev(
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
        ))
        .unwrap();
        assert!(s.finish().is_err());
    }

    #[test]
    fn tool_id_normalization() {
        assert_eq!(normalize_tool_call_id("a b/c"), "a_b_c");
        assert_eq!(normalize_tool_call_id(&"x".repeat(100)).len(), 64);
    }
}
