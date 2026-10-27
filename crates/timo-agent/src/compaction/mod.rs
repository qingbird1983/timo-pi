//! 上下文压缩（phase-3.5/04）：阈值、切点、摘要请求与检查点载荷。

use std::sync::Arc;

use timo_ai::{
    Content, Context, Message, Model, ProviderApi, StopReason, StreamOptions, Usage, user_text,
};
use timo_config::{Clock, CompactionSettings, RetrySettings, ThinkingLevel};
use tokio_util::sync::CancellationToken;

use crate::events::CompactionReason;

/// 摘要请求的系统提示（自写措辞，结构对齐 pi）。
const SUMMARY_SYSTEM_PROMPT: &str = "You summarize conversations. Read the conversation and produce the structured checkpoint below. Do not continue the conversation, do not answer its questions, and output only the summary.";

/// 首次摘要的要求（固定标题）。
const SUMMARY_PROMPT: &str =
    "Produce a structured context checkpoint another assistant can use to continue this work.

Use exactly this format:

## Goal
[What the user is trying to accomplish; multiple items if the session covers several tasks.]

## Constraints & Preferences
- [Constraints, preferences or requirements mentioned by the user; \"(none)\" if none.]

## Progress
### Done
- [x] [Completed work]

### In Progress
- [ ] [Current work]

### Blocked
- [Blockers, if any]

## Key Decisions
- **[Decision]**: [Brief rationale]

## Next Steps
1. [Ordered next actions]

## Critical Context
- [Data, examples or references needed to continue; \"(none)\" if none]

Keep each section concise. Preserve exact file paths, function names and error messages.";

/// 已有摘要时的更新要求。
const UPDATE_PROMPT: &str = "Update the existing structured summary with the new messages above.

Rules:
- Preserve all existing information.
- Add new progress, decisions and context.
- Move finished items from In Progress to Done.
- Refresh Next Steps.
- Preserve exact file paths, function names and error messages.

Use exactly the same format as the existing summary. Keep each section concise.";

/// 压缩失败原因（调用方只打日志/事件，不中断 run）。
#[derive(Debug, thiserror::Error)]
pub enum CompactionError {
    /// 没有可摘要的内容（切点不成立或消息太少）。
    #[error("nothing to compact (session too small)")]
    Nothing,
    /// 凭证解析失败。
    #[error("summarization auth: {0}")]
    Auth(String),
    /// 摘要请求失败/被取消/响应不可用。
    #[error("summarization failed: {0}")]
    Failed(String),
}

/// 一次压缩的产出。
#[derive(Debug, Clone)]
pub struct CompactionOutcome {
    /// 摘要正文（不含包装标签）。
    pub summary: String,
    /// 保留的近期消息条数（投影用）。
    pub keep_recent_messages: usize,
    /// 压缩前估算 token。
    pub tokens_before: u64,
    /// 压缩后估算 token。
    pub tokens_after: u64,
    /// 摘要请求用量。
    pub usage: Usage,
    /// 只读文件（被摘要范围内）。
    pub read_files: Vec<String>,
    /// 修改文件（被摘要范围内）。
    pub modified_files: Vec<String>,
    /// 压缩后的内存上下文（摘要消息 + 保留段）。
    pub messages: Vec<Message>,
}

impl CompactionOutcome {
    /// 会话 entry 载荷（`session.compaction`）。
    #[must_use]
    pub fn entry_payload(&self, reason: CompactionReason) -> serde_json::Value {
        serde_json::json!({
            "summary": self.summary,
            "keep_recent_messages": self.keep_recent_messages,
            "tokens_before": self.tokens_before,
            "tokens_after": self.tokens_after,
            "reason": reason.as_str(),
            "usage": self.usage,
            "details": {
                "read_files": self.read_files,
                "modified_files": self.modified_files,
            }
        })
    }
}

/// 压缩请求的输入（借用调用方状态，避免复制配置）。
pub struct CompactInput<'a> {
    /// 当前上下文消息（全量）。
    pub messages: &'a [Message],
    /// 当前模型。
    pub model: &'a Model,
    /// 协议适配器。
    pub api: &'a Arc<dyn ProviderApi>,
    /// 凭证解析器。
    pub auth: &'a Arc<timo_ai::AuthResolver>,
    /// 重试设置。
    pub retry: &'a RetrySettings,
    /// 当前思考档位。
    pub thinking: ThinkingLevel,
    /// 压缩设置。
    pub settings: &'a CompactionSettings,
    /// 取消令牌（与当前 run 共用）。
    pub cancel: &'a CancellationToken,
    /// 时钟。
    pub clock: &'a Arc<dyn Clock>,
    /// 手动压缩的补充要求。
    pub instructions: Option<&'a str>,
}

/// 估算单条消息的 token（字符数 ÷ 4 向上取整；故意高估）。
#[must_use]
pub fn estimate_tokens(message: &Message) -> u64 {
    let chars: usize = match message {
        Message::System(m) => m.content.chars().count(),
        Message::User(m) => content_chars(&m.content),
        Message::Assistant(m) => m
            .content
            .iter()
            .map(|c| match c {
                Content::Text { text } => text.chars().count(),
                Content::Thinking { thinking, .. } => thinking.chars().count(),
                Content::ToolCall {
                    name, arguments, ..
                } => name.chars().count() + arguments.to_string().chars().count(),
                _ => 0,
            })
            .sum(),
        Message::ToolResult(m) => content_chars(&m.content),
    };
    chars.div_ceil(4) as u64
}

fn content_chars(content: &[Content]) -> usize {
    content
        .iter()
        .map(|c| match c {
            Content::Text { text } => text.chars().count(),
            // 图片按固定字符估算（pi 同值：4800 字符）
            Content::Image { .. } => 4800,
            _ => 0,
        })
        .sum()
}

/// 全上下文 token 估算：最后一条有效 assistant usage + 其后消息估算。
#[must_use]
pub fn context_tokens(messages: &[Message]) -> u64 {
    let usage_index = messages.iter().rposition(|m| match m {
        Message::Assistant(a) => {
            !matches!(a.stop_reason, StopReason::Error | StopReason::Aborted)
                && a.usage.total_tokens > 0
        }
        _ => false,
    });
    if let Some(index) = usage_index {
        let base = match &messages[index] {
            Message::Assistant(a) => a.usage.total_tokens,
            _ => 0,
        };
        return base
            + messages[index + 1..]
                .iter()
                .map(estimate_tokens)
                .sum::<u64>();
    }
    messages.iter().map(estimate_tokens).sum()
}

/// 是否达到阈值（窗口未知时 false）。
#[must_use]
pub fn should_compact(tokens: u64, context_window: u64, settings: &CompactionSettings) -> bool {
    settings.enabled
        && context_window > 0
        && tokens > context_window.saturating_sub(settings.reserve_tokens)
}

/// 选切点：从尾部累加到 `keep_recent_tokens`，再回退到最近的 user 消息。
///
/// 返回保留段起点；`None` 表示没有可摘要的内容（保持原样）。
#[must_use]
pub fn cut_index(messages: &[Message], keep_recent_tokens: u64) -> Option<usize> {
    if messages.len() < 2 {
        return None;
    }
    let mut accumulated = 0u64;
    let mut candidate = 0usize;
    for index in (0..messages.len()).rev() {
        accumulated += estimate_tokens(&messages[index]);
        if accumulated >= keep_recent_tokens {
            candidate = index;
            break;
        }
    }
    for index in (1..=candidate).rev() {
        if matches!(messages[index], Message::User(_)) {
            return Some(index);
        }
    }
    None
}

/// 会话序列化（给摘要模型读；thinking 不写，tool result 截断 2000 字符）。
#[must_use]
pub fn serialize_conversation(messages: &[Message]) -> String {
    let mut parts: Vec<String> = vec![];
    for message in messages {
        match message {
            Message::System(_) => {}
            Message::User(m) => {
                if timo_session::extract_summary(message).is_some() {
                    continue; // 摘要消息经 <previous-summary> 单独传递
                }
                let text = text_of(&m.content);
                if !text.is_empty() {
                    parts.push(format!("[User]: {text}"));
                }
            }
            Message::Assistant(m) => {
                let text = text_of(&m.content);
                if !text.is_empty() {
                    parts.push(format!("[Assistant]: {text}"));
                }
                let calls: Vec<String> = m
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        Content::ToolCall {
                            name, arguments, ..
                        } => Some(format!("{name}({})", arguments)),
                        _ => None,
                    })
                    .collect();
                if !calls.is_empty() {
                    parts.push(format!("[Assistant tool calls]: {}", calls.join("; ")));
                }
            }
            Message::ToolResult(m) => {
                let text = text_of(&m.content);
                if !text.is_empty() {
                    parts.push(format!("[Tool result]: {}", truncate(&text, 2000)));
                }
            }
        }
    }
    parts.join("\n\n")
}

fn text_of(content: &[Content]) -> String {
    content
        .iter()
        .filter_map(|c| match c {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max).collect();
    format!("{kept}\n\n[... more characters truncated]")
}

/// 扫描被摘要范围内的文件操作（read/write/edit 的 `path` 参数）。
#[must_use]
pub fn file_operations(messages: &[Message]) -> (Vec<String>, Vec<String>) {
    use std::collections::BTreeSet;
    let mut read = BTreeSet::new();
    let mut modified = BTreeSet::new();
    for message in messages {
        let Message::Assistant(assistant) = message else {
            continue;
        };
        for content in &assistant.content {
            let Content::ToolCall {
                name, arguments, ..
            } = content
            else {
                continue;
            };
            let Some(path) = arguments.get("path").and_then(|v| v.as_str()) else {
                continue;
            };
            match name.as_str() {
                "read" => {
                    read.insert(path.to_string());
                }
                "write" | "edit" => {
                    modified.insert(path.to_string());
                }
                _ => {}
            }
        }
    }
    for path in &modified {
        read.remove(path);
    }
    (read.into_iter().collect(), modified.into_iter().collect())
}

/// 执行一次压缩（自动或手动）；不写会话 entry（由调用方负责）。
///
/// # Errors
/// 见 [`CompactionError`]。
pub async fn compact(input: CompactInput<'_>) -> Result<CompactionOutcome, CompactionError> {
    let tokens_before = context_tokens(input.messages);
    let cut = cut_index(input.messages, input.settings.keep_recent_tokens)
        .ok_or(CompactionError::Nothing)?;
    let kept = &input.messages[cut..];
    let summarized = &input.messages[..cut];
    let previous = summarized
        .first()
        .and_then(timo_session::extract_summary)
        .map(str::to_string);
    let conversation = serialize_conversation(summarized);
    let mut prompt = format!("<conversation>\n{conversation}\n</conversation>\n\n");
    if let Some(previous) = &previous {
        prompt.push_str(&format!(
            "<previous-summary>\n{previous}\n</previous-summary>\n\n"
        ));
    }
    prompt.push_str(if previous.is_some() {
        UPDATE_PROMPT
    } else {
        SUMMARY_PROMPT
    });
    if let Some(extra) = input.instructions.filter(|s| !s.trim().is_empty()) {
        prompt.push_str(&format!("\n\nAdditional focus: {extra}"));
    }

    let api_key = input
        .auth
        .resolve()
        .await
        .map_err(|e| CompactionError::Auth(e.to_string()))?;
    let request = Context {
        system_prompt: Some(SUMMARY_SYSTEM_PROMPT.to_string()),
        messages: vec![user_text(&prompt, input.clock.now_ms())],
        tools: vec![],
        thinking_level: (input.model.reasoning && input.thinking != ThinkingLevel::Off)
            .then_some(input.thinking),
    };
    let options = StreamOptions {
        api_key: Some(api_key),
        cancel: input.cancel.clone(),
        max_tokens: None,
        temperature: None,
        max_retries: if input.retry.enabled {
            input.retry.max_retries
        } else {
            0
        },
        max_retry_delay_ms: input.retry.max_retry_delay_ms,
        base_url: None,
        headers: vec![],
        provider_hooks: None,
    };
    let response = input
        .api
        .stream(input.model, &request, options)
        .await
        .result()
        .await;
    let usage = response.usage.clone();
    if response.stop_reason == StopReason::Length {
        return Err(CompactionError::Failed(
            "generation hit the token cap; summary would be incomplete".into(),
        ));
    }
    if response
        .content
        .iter()
        .any(|c| matches!(c, Content::ToolCall { .. }))
    {
        return Err(CompactionError::Failed(
            "summarization attempted to call a tool".into(),
        ));
    }
    let text = text_of(&response.content);
    if text.trim().is_empty() {
        return Err(CompactionError::Failed("empty summary".into()));
    }

    let (read_files, modified_files) = file_operations(summarized);
    let mut summary = text;
    append_file_lists(&mut summary, &read_files, &modified_files);

    let mut messages = Vec::with_capacity(kept.len() + 1);
    messages.push(timo_session::summary_message(
        &summary,
        input.clock.now_ms(),
    ));
    messages.extend_from_slice(kept);

    // 压缩后最后一条 assistant 的 usage 仍是压缩前的上下文读数，不能当基线；
    // 压缩后长度一律按全量估算（与 pi 一致：只记录 tokensBefore，After 是估算）。
    let tokens_after = messages.iter().map(estimate_tokens).sum();
    Ok(CompactionOutcome {
        summary,
        keep_recent_messages: kept.len(),
        tokens_before,
        tokens_after,
        usage,
        read_files,
        modified_files,
        messages,
    })
}

fn append_file_lists(summary: &mut String, read: &[String], modified: &[String]) {
    let mut sections = vec![];
    if !read.is_empty() {
        sections.push(format!("<read-files>\n{}\n</read-files>", read.join("\n")));
    }
    if !modified.is_empty() {
        sections.push(format!(
            "<modified-files>\n{}\n</modified-files>",
            modified.join("\n")
        ));
    }
    if !sections.is_empty() {
        summary.push_str(&format!("\n\n{}", sections.join("\n\n")));
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
