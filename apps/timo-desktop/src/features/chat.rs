//! 会话聊天状态：内核事件归并成消息行与工具 trace（设计 01 §5~§6）。
//!
//! 这里只做数据与渲染；命令发送、会话切换由根视图负责。

use timo_kernel::{AssistantMessage, AssistantStreamEvent, Content, KernelEvent, Message, Usage};

/// 工具 trace 行状态（设计 01 §6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceStatus {
    /// 已请求、尚未开始。
    Queued,
    /// 等待用户审阅。
    Review,
    /// 执行中。
    Running,
    /// 成功完成。
    Done,
    /// 审阅被拒绝（无副作用）。
    Blocked,
    /// 失败（工具结果 is_error）。
    Failed,
    /// 恢复会话时缺少配对结果。
    Interrupted,
}

/// 一次工具调用在 trace 中的一行。
#[derive(Debug, Clone)]
pub struct TraceEntry {
    /// 调用 id。
    pub call_id: String,
    /// 工具名（技术标识，不翻译）。
    pub name: String,
    /// 关键参数摘要。
    pub summary: String,
    /// 状态。
    pub status: TraceStatus,
    /// 结果正文（折叠展示）。
    pub output: Option<String>,
    /// 执行时长。
    pub duration_ms: Option<u64>,
}

/// assistant 消息行：正文、思考、trace 与错误。
#[derive(Debug, Clone)]
pub struct AssistantRow {
    /// 正文。
    pub text: String,
    /// 思考摘要（推理模型）。
    pub thinking: String,
    /// 错误文案。
    pub error: Option<String>,
    /// 是否仍在流式生成。
    pub streaming: bool,
    /// 工具 trace。
    pub trace: Vec<TraceEntry>,
    /// trace 是否展开。
    pub trace_open: bool,
}

/// 一行聊天内容。
#[derive(Debug, Clone)]
pub enum RowKind {
    /// 用户消息。
    User(String),
    /// assistant 消息。
    Assistant(AssistantRow),
}

/// 聊天行。
#[derive(Debug, Clone)]
pub struct Row {
    /// 稳定 key（ElementId 用）。
    pub key: String,
    /// 内容。
    pub kind: RowKind,
}

/// 会话内的聊天状态。
#[derive(Default)]
pub struct ChatState {
    /// 消息行。
    pub rows: Vec<Row>,
    /// 最近一次 token 用量。
    pub usage: Option<Usage>,
    /// 是否正在运行。
    pub running: bool,
    /// 最近一次内核级错误。
    pub last_error: Option<String>,
    next_id: u64,
}

impl ChatState {
    /// 清空为草稿态。
    pub fn reset(&mut self) {
        self.rows.clear();
        self.usage = None;
        self.last_error = None;
        self.running = false;
    }

    /// 用投影历史填充消息流。
    pub fn load_history(&mut self, history: &[Message]) {
        self.reset();
        for message in history {
            match message {
                Message::User(user) => {
                    let text = text_of(&user.content);
                    if !text.trim().is_empty() {
                        self.push_user(&text);
                    }
                }
                Message::Assistant(assistant) => {
                    let mut row = AssistantRow {
                        text: text_of(&assistant.content),
                        thinking: thinking_of(&assistant.content),
                        error: assistant.error_message.clone(),
                        streaming: false,
                        trace: assistant
                            .content
                            .iter()
                            .filter_map(|content| match content {
                                Content::ToolCall {
                                    id,
                                    name,
                                    arguments,
                                    ..
                                } => Some(TraceEntry {
                                    call_id: id.clone(),
                                    name: name.clone(),
                                    summary: summarize(name, arguments),
                                    status: TraceStatus::Interrupted,
                                    output: None,
                                    duration_ms: None,
                                }),
                                _ => None,
                            })
                            .collect(),
                        trace_open: false,
                    };
                    row.trace_open = false;
                    self.push_assistant(row);
                }
                Message::ToolResult(result) => {
                    if let Some(entry) = self.current_assistant_mut().and_then(|assistant| {
                        assistant
                            .trace
                            .iter_mut()
                            .find(|entry| entry.call_id == result.tool_call_id)
                    }) {
                        entry.status = if result.is_error {
                            TraceStatus::Failed
                        } else {
                            TraceStatus::Done
                        };
                        entry.output = Some(text_of(&result.content));
                        entry.duration_ms = result.duration_ms;
                    }
                }
                Message::System(_) => {}
            }
        }
    }

    /// 最近一条用户消息（命令面板“重发”用）。
    #[must_use]
    pub fn last_user_text(&self) -> Option<String> {
        self.rows.iter().rev().find_map(|row| match &row.kind {
            RowKind::User(text) => Some(text.clone()),
            RowKind::Assistant(_) => None,
        })
    }

    /// 追加用户消息。
    pub fn push_user(&mut self, text: &str) {
        self.push_row(RowKind::User(text.to_string()));
    }

    /// 追加 assistant 行。
    fn push_assistant(&mut self, row: AssistantRow) {
        self.push_row(RowKind::Assistant(row));
    }

    fn push_row(&mut self, kind: RowKind) {
        self.next_id += 1;
        self.rows.push(Row {
            key: format!("row-{}", self.next_id),
            kind,
        });
    }

    fn current_assistant_mut(&mut self) -> Option<&mut AssistantRow> {
        match &mut self.rows.last_mut()?.kind {
            RowKind::Assistant(row) => Some(row),
            _ => None,
        }
    }

    fn ensure_assistant(&mut self) -> &mut AssistantRow {
        if !matches!(
            self.rows.last().map(|row| &row.kind),
            Some(RowKind::Assistant(_))
        ) {
            self.push_assistant(AssistantRow {
                text: String::new(),
                thinking: String::new(),
                error: None,
                streaming: true,
                trace: vec![],
                trace_open: true,
            });
        }
        // 上一行刚插入 assistant 行，这里必然命中。
        let Some(row) = self.current_assistant_mut() else {
            unreachable!("an assistant row was just ensured");
        };
        row
    }

    /// 合并一条内核事件。
    pub fn apply(&mut self, event: &KernelEvent) {
        match event {
            KernelEvent::AgentStarted => {
                self.running = true;
                self.last_error = None;
            }
            KernelEvent::TurnStarted => {
                self.ensure_assistant();
            }
            KernelEvent::StreamEvent { event } => self.apply_stream(event),
            KernelEvent::ToolCallRequested { call } => {
                if let Content::ToolCall {
                    id,
                    name,
                    arguments,
                    ..
                } = call.as_ref()
                {
                    let row = self.ensure_assistant();
                    if !row.trace.iter().any(|entry| entry.call_id == *id) {
                        row.trace.push(TraceEntry {
                            call_id: id.clone(),
                            name: name.clone(),
                            summary: summarize(name, arguments),
                            status: TraceStatus::Queued,
                            output: None,
                            duration_ms: None,
                        });
                        row.trace_open = true;
                    }
                }
            }
            KernelEvent::ToolCallStarted {
                tool_call_id,
                tool_name,
                args,
            } => {
                let row = self.ensure_assistant();
                let summary = summarize(tool_name, args);
                match row
                    .trace
                    .iter_mut()
                    .find(|entry| entry.call_id == *tool_call_id)
                {
                    Some(entry) => {
                        entry.status = TraceStatus::Running;
                        entry.summary = summary;
                    }
                    None => row.trace.push(TraceEntry {
                        call_id: tool_call_id.clone(),
                        name: tool_name.clone(),
                        summary,
                        status: TraceStatus::Running,
                        output: None,
                        duration_ms: None,
                    }),
                }
                row.trace_open = true;
            }
            KernelEvent::ToolCallUpdate {
                tool_call_id,
                partial,
                ..
            } => {
                if let Some(entry) = self
                    .current_assistant_mut()
                    .and_then(|row| row.trace.iter_mut().find(|e| e.call_id == *tool_call_id))
                {
                    entry.status = TraceStatus::Running;
                    entry.output = Some(clip(&text_of(&partial.content), 400));
                }
            }
            KernelEvent::ToolCallFinished {
                tool_call_id,
                result,
                duration_ms,
                ..
            } => {
                if let Some(entry) = self
                    .current_assistant_mut()
                    .and_then(|row| row.trace.iter_mut().find(|e| e.call_id == *tool_call_id))
                {
                    let failed = matches!(result.as_ref(), Message::ToolResult(r) if r.is_error);
                    let text = match result.as_ref() {
                        Message::ToolResult(r) => Some(text_of(&r.content)),
                        _ => None,
                    };
                    let blocked = text
                        .as_deref()
                        .is_some_and(|text| text.trim_start().starts_with("Rejected by user:"));
                    entry.status = if blocked {
                        TraceStatus::Blocked
                    } else if failed {
                        TraceStatus::Failed
                    } else {
                        TraceStatus::Done
                    };
                    entry.output = text;
                    entry.duration_ms = *duration_ms;
                }
            }
            KernelEvent::TurnFinished { message, .. } => {
                if let Message::Assistant(assistant) = message.as_ref() {
                    let row = self.ensure_assistant();
                    let text = text_of(&assistant.content);
                    if !text.is_empty() {
                        row.text = text;
                    }
                    let thinking = thinking_of(&assistant.content);
                    if !thinking.is_empty() {
                        row.thinking = thinking;
                    }
                    row.error = assistant.error_message.clone();
                    row.streaming = false;
                }
            }
            KernelEvent::ProviderResponse { usage, .. } => {
                self.usage = Some(usage.clone());
            }
            KernelEvent::Error { context } => {
                self.last_error = Some(context.clone());
            }
            KernelEvent::AgentFinished { .. } => {
                self.running = false;
                for row in &mut self.rows {
                    if let RowKind::Assistant(assistant) = &mut row.kind {
                        assistant.streaming = false;
                        if !assistant.trace.is_empty() {
                            assistant.trace_open = false;
                        }
                    }
                }
            }
            KernelEvent::MessageAdded { .. } | KernelEvent::ProviderRequest { .. } => {}
            KernelEvent::Compacted { .. } => {
                // 压缩分隔条随 GUI 批次接入；内核侧已切换上下文。
            }
        }
    }

    fn apply_stream(&mut self, event: &AssistantStreamEvent) {
        match event {
            AssistantStreamEvent::Start { partial }
            | AssistantStreamEvent::BlockStart { partial, .. }
            | AssistantStreamEvent::Delta { partial, .. }
            | AssistantStreamEvent::BlockEnd { partial, .. } => {
                self.update_from_partial(partial);
            }
            AssistantStreamEvent::Done { message, .. }
            | AssistantStreamEvent::Error { message } => {
                let row = self.ensure_assistant();
                row.text = text_of(&message.content);
                row.thinking = thinking_of(&message.content);
                row.error = message.error_message.clone();
                row.streaming = false;
            }
        }
    }

    fn update_from_partial(&mut self, partial: &AssistantMessage) {
        let row = self.ensure_assistant();
        row.text = text_of(&partial.content);
        row.thinking = thinking_of(&partial.content);
        if !row.text.is_empty() {
            row.streaming = true;
        }
    }

    /// 审阅请求到达：把对应 trace 行标成待确认。
    pub fn mark_review(&mut self, call_id: &str) {
        if let Some(entry) = self.trace_entry_mut(call_id) {
            entry.status = TraceStatus::Review;
        }
    }

    /// 审阅结束：允许回到等待执行，拒绝标成已拒绝。
    pub fn resolve_review(&mut self, call_id: &str, allowed: bool) {
        if let Some(entry) = self.trace_entry_mut(call_id) {
            entry.status = if allowed {
                TraceStatus::Queued
            } else {
                TraceStatus::Blocked
            };
        }
    }

    fn trace_entry_mut(&mut self, call_id: &str) -> Option<&mut TraceEntry> {
        self.rows
            .iter_mut()
            .rev()
            .find_map(|row| match &mut row.kind {
                RowKind::Assistant(assistant) => assistant
                    .trace
                    .iter_mut()
                    .find(|entry| entry.call_id == call_id),
                RowKind::User(_) => None,
            })
    }

    /// 展开/收起某一行的 trace。
    pub fn toggle_trace(&mut self, row_key: &str) {
        if let Some(Row {
            kind: RowKind::Assistant(row),
            ..
        }) = self.rows.iter_mut().find(|row| row.key == row_key)
        {
            row.trace_open = !row.trace_open;
        }
    }
}

#[cfg(test)]
#[path = "chat_tests.rs"]
mod tests;

pub(super) fn summarize(name: &str, args: &serde_json::Value) -> String {
    let key = match name {
        "bash" | "powershell" => "command",
        "grep" => "pattern",
        "find" => "pattern",
        _ => "path",
    };
    if let Some(value) = args.get(key).and_then(serde_json::Value::as_str) {
        return clip(value, 120);
    }
    for key in ["path", "command", "query", "pattern", "text"] {
        if let Some(value) = args.get(key).and_then(serde_json::Value::as_str) {
            return clip(value, 120);
        }
    }
    String::new()
}

pub(super) fn text_of(content: &[Content]) -> String {
    content
        .iter()
        .filter_map(|content| match content {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn thinking_of(content: &[Content]) -> String {
    content
        .iter()
        .filter_map(|content| match content {
            Content::Thinking { thinking, .. } => Some(thinking.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn clip(text: &str, max_chars: usize) -> String {
    let mut out: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        out.push('…');
    }
    out
}

pub use crate::features::chat_rows::render_row;
