//! 扩展事件与工具适配（设计 05 §3、接缝 #4）。

use std::sync::Arc;

use serde_json::{Value as JsonValue, json};

use timo_ext::{ExtensionHost, ToolMeta};

use crate::KernelEvent;

use super::now;

/// 扩展工具元数据 → 注册表定义（执行经宿主线程）。
#[must_use]
pub fn tool_definition(meta: &ToolMeta, host: Arc<ExtensionHost>) -> timo_tools::ToolDefinition {
    let name = meta.name.clone();
    let call_name = name.clone();
    timo_tools::ToolDefinition {
        name,
        label: meta.label.clone(),
        description: meta.description.clone(),
        prompt_snippet: meta.prompt_snippet.clone(),
        prompt_guidelines: meta.prompt_guidelines.clone(),
        parameters: meta.parameters.clone(),
        output_schema: meta.output_schema.clone(),
        exposure: match meta.exposure.as_str() {
            "model-only" => timo_tools::Exposure::ModelOnly,
            "codemode" => timo_tools::Exposure::Codemode,
            "deferred" => timo_tools::Exposure::Deferred,
            "hidden" => timo_tools::Exposure::Hidden,
            _ => timo_tools::Exposure::Direct,
        },
        namespace: None,
        annotations: annotations_of(meta),
        execution_mode: if meta.execution_mode.as_deref() == Some("sequential") {
            timo_tools::ExecutionMode::Sequential
        } else {
            timo_tools::ExecutionMode::Parallel
        },
        execute: Arc::new(move |ctx| {
            let host = Arc::clone(&host);
            let name = call_name.clone();
            Box::pin(async move {
                let args = ctx.params.clone();
                let result = tokio::task::spawn_blocking(move || host.call_tool(&name, args))
                    .await
                    .map_err(|error| {
                        timo_tools::ToolError::Execution(format!("extension host join: {error}"))
                    })?
                    .map_err(|error| timo_tools::ToolError::Execution(error.to_string()))?;
                let content = serde_json::from_value::<Vec<timo_ai::Content>>(result.content)
                    .map_err(|error| {
                        timo_tools::ToolError::Execution(format!(
                            "extension tool content is invalid: {error}"
                        ))
                    })?;
                Ok(timo_tools::ToolOutput {
                    content,
                    details: result.details,
                    structured_content: result.structured_content,
                    is_error: result.is_error,
                    terminate: result.terminate,
                })
            })
        }),
    }
}

fn annotations_of(meta: &ToolMeta) -> timo_tools::Annotations {
    let Some(value) = &meta.annotations else {
        return timo_tools::Annotations::default();
    };
    let flag = |key: &str| value.get(key).and_then(JsonValue::as_bool);
    timo_tools::Annotations {
        read_only_hint: flag("readOnlyHint"),
        destructive_hint: flag("destructiveHint"),
        idempotent_hint: flag("idempotentHint"),
        open_world_hint: flag("openWorldHint"),
    }
}

/// `KernelEvent` → 扩展通知事件（设计 05 §3；只通知，不回读）。
#[must_use]
pub fn event_payload(event: &KernelEvent) -> Option<(&'static str, JsonValue)> {
    let payload = match event {
        KernelEvent::AgentStarted => ("agent_start", json!({})),
        KernelEvent::AgentFinished { messages } => ("agent_end", json!({ "messages": messages })),
        KernelEvent::TurnStarted => ("turn_start", json!({ "timestamp": now() })),
        KernelEvent::TurnFinished {
            message,
            tool_results,
        } => (
            "turn_end",
            json!({ "message": message, "toolResults": tool_results }),
        ),
        KernelEvent::MessageAdded { message } => ("message_end", json!({ "message": message })),
        KernelEvent::StreamEvent { event } => {
            let (kind, index, delta, partial) = match event.as_ref() {
                timo_ai::AssistantStreamEvent::Start { partial } => {
                    ("start", None, None, Some(partial))
                }
                timo_ai::AssistantStreamEvent::BlockStart { index, partial } => {
                    ("block_start", Some(*index), None, Some(partial))
                }
                timo_ai::AssistantStreamEvent::Delta {
                    index,
                    delta,
                    partial,
                } => ("delta", Some(*index), Some(delta.as_str()), Some(partial)),
                timo_ai::AssistantStreamEvent::BlockEnd { index, partial } => {
                    ("block_end", Some(*index), None, Some(partial))
                }
                timo_ai::AssistantStreamEvent::Done { .. } => ("done", None, None, None),
                timo_ai::AssistantStreamEvent::Error { message } => {
                    ("error", None, None, Some(message))
                }
            };
            let mut payload = json!({ "type": kind, "assistantMessageEvent": kind });
            if let Some(index) = index {
                payload["index"] = json!(index);
            }
            if let Some(delta) = delta {
                payload["delta"] = json!(delta);
            }
            if let Some(partial) = partial {
                payload["message"] = json!(partial);
            }
            ("message_update", payload)
        }
        KernelEvent::ToolCallStarted {
            tool_call_id,
            tool_name,
            args,
        } => (
            "tool_execution_start",
            json!({ "toolCallId": tool_call_id, "toolName": tool_name, "args": args }),
        ),
        KernelEvent::ToolCallFinished {
            tool_call_id,
            tool_name,
            result,
            duration_ms,
        } => (
            "tool_execution_end",
            json!({
                "toolCallId": tool_call_id,
                "toolName": tool_name,
                "result": result,
                "durationMs": duration_ms,
            }),
        ),
        _ => return None,
    };
    Some(payload)
}

/// 事件转发任务：订阅总线，通知事件投递给扩展（不阻塞内核）。
pub fn forward_events(
    mut events: tokio::sync::broadcast::Receiver<KernelEvent>,
    host: Arc<ExtensionHost>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Ok(event) = events.recv().await {
            if let Some((name, payload)) = event_payload(&event) {
                host.notify_event(name, payload);
            }
        }
    })
}

/// 命令参数：`/name a b` → `["a","b"]`。
#[must_use]
pub fn split_command(line: &str) -> Option<(String, JsonValue)> {
    let line = line.trim().trim_start_matches('/');
    let mut parts = line.split_whitespace();
    let name = parts.next()?.to_string();
    let args: Vec<&str> = parts.collect();
    Some((name, json!(args)))
}
