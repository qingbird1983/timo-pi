//! 内核 ↔ 扩展宿主的事件桥（设计 04 §3/§6/§7）。
//!
//! 把 `LoopHooks` / `ProviderHooks` 的请求-响应映射为 JS 事件派发；
//! 大载荷、abort 退休与错误隔离都在此处统一处理。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use serde_json::{Value as JsonValue, json};
use timo_agent::{
    BeforeAgentStart, BeforeToolCall, LoopHooks, RouteChoice, RouteRequest, ToolOutcome,
    ToolOutputPatch,
};
use timo_ai::{Content, Message, ProviderHooks};

use crate::diag::ErrorCode;
use crate::host::ExtensionHost;

/// 单事件序列化上限（设计 04 §6.5：超限不派发）。
pub const MAX_EVENT_BYTES: usize = 8 * 1024 * 1024;

/// 扩展钩子桥：一个实例同时实现循环钩子与供应商钩子。
pub struct ExtHooks {
    host: Arc<ExtensionHost>,
    abort: Arc<AtomicBool>,
}

impl ExtHooks {
    /// 由已装载的宿主构造。
    #[must_use]
    pub fn new(host: Arc<ExtensionHost>) -> Self {
        Self {
            host,
            abort: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 共享取消标志（内核 run abort 时置位，事件不再派发）。
    #[must_use]
    pub fn with_abort_flag(mut self, flag: Arc<AtomicBool>) -> Self {
        self.abort = flag;
        self
    }

    /// 底层扩展宿主。
    #[must_use]
    pub fn host(&self) -> &Arc<ExtensionHost> {
        &self.host
    }

    fn dispatch(&self, event: &str, payload: JsonValue) -> Option<JsonValue> {
        if self.abort.load(Ordering::Relaxed) {
            return None;
        }
        match serde_json::to_string(&payload) {
            Ok(text) if text.len() > MAX_EVENT_BYTES => {
                self.host.note(
                    ErrorCode::E304,
                    format!(
                        "event {event} payload is {} bytes (limit {MAX_EVENT_BYTES})",
                        text.len()
                    ),
                );
                return None;
            }
            Ok(_) => {}
            Err(error) => {
                self.host
                    .note(ErrorCode::E304, format!("event {event} encode: {error}"));
                return None;
            }
        }
        match self.host.dispatch(event, payload) {
            Ok(value) => {
                if value.is_null() {
                    None
                } else {
                    Some(value)
                }
            }
            Err(error) => {
                self.host
                    .note(error.code().unwrap_or(ErrorCode::E302), error.to_string());
                None
            }
        }
    }
}

#[async_trait]
impl LoopHooks for ExtHooks {
    async fn before_agent_start(&self, req: BeforeAgentStart<'_>) -> Option<String> {
        let payload = json!({
            "prompt": req.prompt,
            "systemPrompt": req.system_prompt,
        });
        let value = self.dispatch("before_agent_start", payload)?;
        let prompt = value
            .get("systemPrompt")
            .and_then(JsonValue::as_str)?
            .to_string();
        self.host.set_system_prompt(prompt.clone());
        Some(prompt)
    }

    async fn transform_context(&self, messages: Vec<Message>) -> Option<Vec<Message>> {
        let value = self.dispatch("context", json!({ "messages": messages }))?;
        serde_json::from_value(value.get("messages")?.clone()).ok()
    }

    async fn transform_full_context(
        &self,
        system_prompt: Option<String>,
        messages: Vec<Message>,
    ) -> Option<(Option<String>, Vec<Message>)> {
        let value = self.dispatch(
            "context_with_system",
            json!({ "messages": messages, "systemPrompt": system_prompt }),
        )?;
        let messages = serde_json::from_value(value.get("messages")?.clone()).ok()?;
        let system = value
            .get("systemPrompt")
            .and_then(JsonValue::as_str)
            .map(str::to_string)
            .or(system_prompt);
        Some((system, messages))
    }

    async fn route_model(&self, _req: RouteRequest<'_>) -> Option<RouteChoice> {
        // D23：虚拟模型路由在 kernel 注册面落地前不参与请求。
        None
    }

    async fn before_tool_call(&self, call: &Content) -> Option<BeforeToolCall> {
        let Content::ToolCall {
            id,
            name,
            arguments,
            ..
        } = call
        else {
            return None;
        };
        let value = self.dispatch(
            "tool_call",
            json!({ "toolCallId": id, "toolName": name, "input": arguments }),
        )?;
        let block = value.get("block").and_then(JsonValue::as_bool) == Some(true);
        let args = value
            .get("input")
            .filter(|input| *input != arguments)
            .cloned();
        Some(BeforeToolCall {
            block: block.then(|| {
                value
                    .get("reason")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("blocked by extension")
                    .to_string()
            }),
            args,
            terminate: value.get("terminate").and_then(JsonValue::as_bool) == Some(true),
        })
    }

    async fn after_tool_call(&self, outcome: &ToolOutcome<'_>) -> Option<ToolOutputPatch> {
        let payload = json!({
            "toolCallId": outcome.tool_call_id,
            "toolName": outcome.tool_name,
            "content": outcome.output.content,
            "details": outcome.output.details,
            "structuredContent": outcome.output.structured_content,
            "isError": outcome.output.is_error,
        });
        let value = self.dispatch("tool_result", payload)?;
        let patch = ToolOutputPatch {
            content: value
                .get("content")
                .and_then(|v| serde_json::from_value(v.clone()).ok()),
            details: value.get("details").cloned(),
            structured_content: value
                .get("structuredContent")
                .map(|v| if v.is_null() { None } else { Some(v.clone()) }),
            is_error: value.get("isError").and_then(JsonValue::as_bool),
            terminate: value.get("terminate").and_then(JsonValue::as_bool),
        };
        Some(patch)
    }

    fn system_prompt(&self) -> Option<String> {
        let prompt = self.host.system_prompt();
        if prompt.is_empty() {
            None
        } else {
            Some(prompt)
        }
    }
}

#[async_trait]
impl ProviderHooks for ExtHooks {
    async fn before_request(&self, payload: JsonValue) -> Option<JsonValue> {
        let value = self.dispatch("before_provider_request", payload)?;
        value.is_object().then_some(value)
    }

    async fn before_headers(
        &self,
        headers: Vec<(String, String)>,
    ) -> Option<Vec<(String, Option<String>)>> {
        let pairs: Vec<JsonValue> = headers
            .iter()
            .map(|(name, value)| json!([name, value]))
            .collect();
        let value = self.dispatch("before_provider_headers", json!({ "headers": pairs }))?;
        parse_header_patch(value.get("headers").unwrap_or(&value))
    }

    async fn after_response(&self, status: u16, headers: Vec<(String, String)>) {
        let pairs: Vec<JsonValue> = headers
            .iter()
            .map(|(name, value)| json!([name, value]))
            .collect();
        let _ = self.dispatch(
            "after_provider_response",
            json!({ "status": status, "headers": pairs }),
        );
    }
}

/// 接受对象表或 `[name, value|null]` 数组两种头补丁形态。
fn parse_header_patch(value: &JsonValue) -> Option<Vec<(String, Option<String>)>> {
    if let Some(array) = value.as_array() {
        let mut out = Vec::new();
        for item in array {
            let pair = item.as_array()?;
            let name = pair.first()?.as_str()?.to_string();
            let header = match pair.get(1) {
                Some(JsonValue::Null) | None => None,
                Some(value) => Some(value.as_str()?.to_string()),
            };
            out.push((name, header));
        }
        return Some(out);
    }
    let object = value.as_object()?;
    Some(
        object
            .iter()
            .map(|(name, value)| {
                let header = if value.is_null() {
                    None
                } else {
                    value.as_str().map(str::to_string)
                };
                (name.clone(), header)
            })
            .collect(),
    )
}
