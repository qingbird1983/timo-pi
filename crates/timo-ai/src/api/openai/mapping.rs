//! OpenAI 字段映射：finish_reason → 统一停止原因、推理字段候选。

use crate::types::StopReason;

/// 推理内容字段候选（首个非空者胜；signature 记字段名供回放路由）。
pub(crate) const REASONING_FIELDS: &[&str] = &["reasoning_content", "reasoning", "reasoning_text"];

/// finish_reason → StopReason（`stop/end→Stop`、`length→Length`、
/// `tool_calls/function_call→ToolUse`，其余含 content_filter → Error）。
#[must_use]
pub(crate) fn map_finish_reason(raw: &str) -> StopReason {
    match raw {
        "stop" | "end" => StopReason::Stop,
        "length" => StopReason::Length,
        "tool_calls" | "function_call" => StopReason::ToolUse,
        _ => StopReason::Error,
    }
}
