//! openai-completions chunk 流 → 流事件状态机（设计 01 §5 映射表）。

use serde_json::Value;

use crate::api::sse::SseEvent;
use crate::catalog::{Compat, RateCard, calculate_cost};
use crate::errors::AiError;
use crate::stream::AssistantStreamEvent;
use crate::types::{AssistantMessage, Content, StopReason, Usage};

use super::mapping::{REASONING_FIELDS, map_finish_reason};

/// chunk 驱动的流状态。
pub(crate) struct OpenAiStream {
    msg: AssistantMessage,
    cost: RateCard,
    compat: Compat,
    /// openai tool_calls 下标 → 本地内容块下标。
    tool_map: std::collections::HashMap<u64, usize>,
    args_raw: std::collections::HashMap<usize, String>,
    /// 当前开放的文本/思考块下标（每轮至多一个文本 + 一个思考块）。
    text_index: Option<usize>,
    thinking_index: Option<usize>,
    finish_reason: Option<StopReason>,
    raw_finish: Option<String>,
}

impl OpenAiStream {
    /// 初始化（携带费率卡与 compat 开关）。
    pub(crate) fn new(base: AssistantMessage, cost: RateCard, compat: Compat) -> Self {
        Self {
            msg: base,
            cost,
            compat,
            tool_map: std::collections::HashMap::new(),
            args_raw: std::collections::HashMap::new(),
            text_index: None,
            thinking_index: None,
            finish_reason: None,
            raw_finish: None,
        }
    }

    /// 当前 partial 快照。
    pub(crate) fn partial(&self) -> &AssistantMessage {
        &self.msg
    }

    /// 消费一条 SSE（chunk JSON 或 `[DONE]`）；产出 0~N 个流事件
    /// （块切换与 finish_reason 可能一次产生多个 BlockEnd）。
    ///
    /// # Errors
    /// JSON 解析失败 → [`AiError::Json`]。
    pub(crate) fn on_sse(&mut self, sse: &SseEvent) -> Result<Vec<AssistantStreamEvent>, AiError> {
        if sse.data.trim() == "[DONE]" {
            return Ok(vec![]);
        }
        let v: Value = serde_json::from_str(&sse.data).map_err(|e| AiError::Json {
            context: "sse chunk".into(),
            detail: e.to_string(),
        })?;
        if let Some(id) = v.get("id").and_then(Value::as_str) {
            self.msg.response_id.get_or_insert_with(|| id.to_string());
        }
        if let Some(m) = v.get("model").and_then(Value::as_str)
            && m != self.msg.model
        {
            self.msg.response_model.get_or_insert_with(|| m.to_string());
        }
        let mut out = vec![];
        if let Some(choice) = v.pointer("/choices/0") {
            self.on_delta(choice, &mut out)?;
            if let Some(fr) = choice.get("finish_reason").and_then(Value::as_str) {
                self.raw_finish = Some(fr.to_string());
                self.finish_reason = Some(map_finish_reason(fr));
                self.close_all_blocks(&mut out);
            }
        }
        if let Some(u) = v.get("usage").filter(|u| !u.is_null()) {
            self.apply_usage(u);
        }
        Ok(out)
    }

    /// 流收尾：终结全部块并按 finish_reason 产出终态。
    ///
    /// # Errors
    /// 缺 finish_reason 且 compat 校验开启 → [`AiError::Sse`]。
    pub(crate) fn finish(mut self) -> Result<AssistantMessage, AiError> {
        self.text_index = None;
        self.thinking_index = None;
        self.finalize_tool_args();
        let stop = match self.finish_reason {
            Some(r) => r,
            None if !self.compat.supports_finish_reason => {
                if self
                    .msg
                    .content
                    .iter()
                    .any(|c| matches!(c, Content::ToolCall { .. }))
                {
                    StopReason::ToolUse
                } else {
                    StopReason::Stop
                }
            }
            None => return Err(AiError::Sse("stream ended without finish_reason".into())),
        };
        self.msg.stop_reason = stop;
        if stop == StopReason::Error {
            self.msg.error_message = Some(format!(
                "Provider finish_reason: {}",
                self.raw_finish.as_deref().unwrap_or("unknown")
            ));
        }
        self.msg.raw_stop_reason = self.raw_finish;
        Ok(self.msg)
    }

    fn on_delta(
        &mut self,
        choice: &Value,
        out: &mut Vec<AssistantStreamEvent>,
    ) -> Result<(), AiError> {
        let delta = match choice.get("delta") {
            Some(d) => d,
            None => return Ok(()),
        };
        if let Some(t) = delta
            .get("content")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            let idx = self.open_text(out);
            self.push_delta(idx, t);
            out.push(AssistantStreamEvent::Delta {
                index: idx,
                delta: t.to_string(),
                partial: self.msg.clone(),
            });
        }
        let reasoning = REASONING_FIELDS.iter().find(|f| {
            delta
                .get(**f)
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty())
        });
        if let Some(field) = reasoning {
            let t = delta.get(*field).and_then(Value::as_str).unwrap_or("");
            let idx = self.open_thinking(field, out);
            self.push_delta(idx, t);
            out.push(AssistantStreamEvent::Delta {
                index: idx,
                delta: t.to_string(),
                partial: self.msg.clone(),
            });
        }
        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                self.on_tool_delta(call, out)?;
            }
        }
        Ok(())
    }

    fn on_tool_delta(
        &mut self,
        call: &Value,
        out: &mut Vec<AssistantStreamEvent>,
    ) -> Result<(), AiError> {
        let index = call.get("index").and_then(Value::as_u64).unwrap_or(0);
        let local = match self.tool_map.get(&index) {
            Some(l) => *l,
            None => {
                self.close_stream_blocks(out);
                let local = self.msg.content.len();
                let id = call
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&format!("call_{index}"))
                    .to_string();
                let name = call
                    .pointer("/function/name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                self.msg.content.push(Content::ToolCall {
                    id,
                    name,
                    arguments: Value::Object(serde_json::Map::new()),
                    arguments_error: None,
                });
                self.args_raw.insert(local, String::new());
                self.tool_map.insert(index, local);
                out.push(AssistantStreamEvent::BlockStart {
                    index: local,
                    partial: self.msg.clone(),
                });
                local
            }
        };
        if let Some(frag) = call.pointer("/function/arguments").and_then(Value::as_str)
            && !frag.is_empty()
        {
            self.args_raw.entry(local).or_default().push_str(frag);
            out.push(AssistantStreamEvent::Delta {
                index: local,
                delta: frag.to_string(),
                partial: self.msg.clone(),
            });
        }
        Ok(())
    }

    fn open_text(&mut self, out: &mut Vec<AssistantStreamEvent>) -> usize {
        if let Some(i) = self.text_index {
            return i;
        }
        self.close_stream_blocks(out);
        let idx = self.msg.content.len();
        self.msg.content.push(Content::Text {
            text: String::new(),
        });
        self.text_index = Some(idx);
        out.push(AssistantStreamEvent::BlockStart {
            index: idx,
            partial: self.msg.clone(),
        });
        idx
    }

    fn open_thinking(&mut self, field: &str, out: &mut Vec<AssistantStreamEvent>) -> usize {
        if let Some(i) = self.thinking_index {
            return i;
        }
        self.close_stream_blocks(out);
        let idx = self.msg.content.len();
        self.msg.content.push(Content::Thinking {
            thinking: String::new(),
            signature: Some(field.to_string()),
            redacted: false,
        });
        self.thinking_index = Some(idx);
        out.push(AssistantStreamEvent::BlockStart {
            index: idx,
            partial: self.msg.clone(),
        });
        idx
    }

    fn push_delta(&mut self, index: usize, frag: &str) {
        match self.msg.content.get_mut(index) {
            Some(Content::Text { text }) => text.push_str(frag),
            Some(Content::Thinking { thinking, .. }) => thinking.push_str(frag),
            _ => {}
        }
    }

    /// 关闭当前文本/思考块（发 BlockEnd）；工具块由 [`Self::close_all_blocks`] 关闭。
    fn close_stream_blocks(&mut self, out: &mut Vec<AssistantStreamEvent>) {
        if let Some(index) = self.text_index.take() {
            out.push(AssistantStreamEvent::BlockEnd {
                index,
                partial: self.msg.clone(),
            });
        }
        if let Some(index) = self.thinking_index.take() {
            out.push(AssistantStreamEvent::BlockEnd {
                index,
                partial: self.msg.clone(),
            });
        }
    }

    /// finish_reason 到达：先解析工具参数（BlockEnd 时参数完整），再关闭全部块。
    fn close_all_blocks(&mut self, out: &mut Vec<AssistantStreamEvent>) {
        self.finalize_tool_args();
        self.close_stream_blocks(out);
        let mut tool_blocks: Vec<usize> = self.tool_map.values().copied().collect();
        tool_blocks.sort_unstable();
        tool_blocks.dedup();
        for index in tool_blocks {
            out.push(AssistantStreamEvent::BlockEnd {
                index,
                partial: self.msg.clone(),
            });
        }
    }

    fn finalize_tool_args(&mut self) {
        let entries: Vec<(usize, String)> = self.args_raw.drain().collect();
        for (local, raw) in entries {
            if let Some(Content::ToolCall {
                arguments,
                arguments_error,
                ..
            }) = self.msg.content.get_mut(local)
            {
                match serde_json::from_str::<Value>(&raw) {
                    Ok(v) => *arguments = v,
                    Err(e) => *arguments_error = Some(e.to_string()),
                }
            }
        }
    }

    fn apply_usage(&mut self, u: &Value) {
        let prompt = u.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0);
        let cached = u
            .pointer("/prompt_tokens_details/cached_tokens")
            .or_else(|| u.get("prompt_cache_hit_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let cache_write = u
            .pointer("/prompt_tokens_details/cache_write_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let output = u
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let mut usage = Usage {
            input: prompt.saturating_sub(cached).saturating_sub(cache_write),
            output,
            cache_read: cached,
            cache_write,
            reasoning: u
                .pointer("/completion_tokens_details/reasoning_tokens")
                .and_then(Value::as_u64),
            ..Usage::default()
        };
        usage.total_tokens = usage.input + usage.output + usage.cache_read + usage.cache_write;
        usage.cost = calculate_cost(&self.cost, &usage);
        if usage.total_tokens > 0 {
            self.msg.usage = usage;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chunk(delta: Value) -> SseEvent {
        SseEvent {
            event: None,
            data: format!(
                r#"{{"id":"c1","model":"gpt-x","choices":[{{"index":0,"delta":{delta},"finish_reason":null}}]}}"#
            ),
        }
    }

    fn finish(fr: &str, usage: bool) -> SseEvent {
        let usage_part = if usage {
            r#","usage":{"prompt_tokens":10,"completion_tokens":5,"prompt_tokens_details":{"cached_tokens":4}}"#
        } else {
            ""
        };
        SseEvent {
            event: None,
            data: format!(
                r#"{{"id":"c1","choices":[{{"index":0,"delta":{{}},"finish_reason":"{fr}"}}]{usage_part}}}"#
            ),
        }
    }

    fn state() -> OpenAiStream {
        OpenAiStream::new(
            AssistantMessage::pending("openai-completions", "openai", "gpt-x", 0),
            RateCard::default(),
            Compat::default(),
        )
    }

    #[test]
    fn emits_block_and_delta_events() {
        let mut s = state();
        let events = s.on_sse(&chunk(json!({"content": "Hi"}))).unwrap();
        assert!(
            matches!(events[0], AssistantStreamEvent::BlockStart { index: 0, .. }),
            "{events:?}"
        );
        assert!(
            matches!(&events[1], AssistantStreamEvent::Delta { index: 0, delta, .. } if delta == "Hi"),
            "{events:?}"
        );
        let events = s
            .on_sse(&chunk(json!({
                "tool_calls": [{"index": 0, "id": "call_a", "function": {"name": "read", "arguments": "{\"path\":\"a.txt\"}"}}]
            })))
            .unwrap();
        assert!(
            matches!(events[0], AssistantStreamEvent::BlockEnd { index: 0, .. }),
            "{events:?}"
        );
        assert!(
            matches!(events[1], AssistantStreamEvent::BlockStart { index: 1, .. }),
            "{events:?}"
        );
        assert!(
            matches!(&events[2], AssistantStreamEvent::Delta { index: 1, delta, .. } if delta == "{\"path\":\"a.txt\"}"),
            "{events:?}"
        );
        // finish_reason 关闭工具块（参数此时已完整）
        let events = s.on_sse(&finish("tool_calls", false)).unwrap();
        let AssistantStreamEvent::BlockEnd { index: 1, partial } = &events[0] else {
            panic!("block end expected: {events:?}");
        };
        match &partial.content[1] {
            Content::ToolCall { arguments, .. } => {
                assert_eq!(arguments["path"], serde_json::json!("a.txt"));
            }
            other => panic!("tool call expected: {other:?}"),
        }
    }

    #[test]
    fn text_then_tool_calls() {
        let mut s = state();
        s.on_sse(&chunk(json!({"content": "Hi"}))).unwrap();
        s.on_sse(&chunk(
            json!({"tool_calls": [{"index": 0, "id": "call_a", "function": {"name": "read", "arguments": ""}}]}),
        ))
        .unwrap();
        s.on_sse(&chunk(
            json!({"tool_calls": [{"index": 0, "function": {"arguments": "{\"path\""}}]}),
        ))
        .unwrap();
        s.on_sse(&chunk(
            json!({"tool_calls": [{"index": 0, "function": {"arguments": ":\"a\"}"}}]}),
        ))
        .unwrap();
        s.on_sse(&finish("tool_calls", true)).unwrap();
        let fin = s.finish().unwrap();
        assert_eq!(fin.stop_reason, StopReason::ToolUse);
        // input = prompt(10) - cached(4) = 6
        assert_eq!(fin.usage.input, 6);
        assert_eq!(fin.usage.cache_read, 4);
        match &fin.content[1] {
            Content::ToolCall {
                name, arguments, ..
            } => {
                assert_eq!(name, "read");
                assert_eq!(arguments["path"], "a");
            }
            other => panic!("toolcall expected: {other:?}"),
        }
    }

    #[test]
    fn reasoning_field_becomes_thinking() {
        let mut s = state();
        s.on_sse(&chunk(json!({"reasoning_content": "think"})))
            .unwrap();
        s.on_sse(&chunk(json!({"content": "answer"}))).unwrap();
        s.on_sse(&finish("stop", false)).unwrap();
        let fin = s.finish().unwrap();
        assert_eq!(fin.stop_reason, StopReason::Stop);
        match &fin.content[0] {
            Content::Thinking {
                thinking,
                signature,
                ..
            } => {
                assert_eq!(thinking, "think");
                assert_eq!(signature.as_deref(), Some("reasoning_content"));
            }
            other => panic!("thinking expected: {other:?}"),
        }
    }

    #[test]
    fn content_filter_maps_to_error_with_message() {
        let mut s = state();
        s.on_sse(&finish("content_filter", false)).unwrap();
        let fin = s.finish().unwrap();
        assert_eq!(fin.stop_reason, StopReason::Error);
        assert!(
            fin.error_message
                .unwrap_or_default()
                .contains("content_filter")
        );
    }

    #[test]
    fn missing_finish_reason_fails_when_checked() {
        let mut s = state();
        s.on_sse(&chunk(json!({"content": "x"}))).unwrap();
        assert!(s.finish().is_err());
    }
}
