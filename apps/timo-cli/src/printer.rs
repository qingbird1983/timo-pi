//! 事件渲染：stdout 流式正文、`[tool]` 起止行与 usage 摘要（从 main.rs 拆出守行数预算）。

use std::io::Write;

use timo_kernel::{AssistantStreamEvent, Content, KernelEvent, Message, Usage};

/// 事件渲染（stdout 正文与工具行；错误走 stderr）。
pub(crate) struct Printer {
    at_line_start: bool,
}

impl Printer {
    pub(crate) fn new() -> Self {
        Self {
            at_line_start: true,
        }
    }

    pub(crate) fn handle(&mut self, event: &KernelEvent) {
        match event {
            KernelEvent::StreamEvent { event } => self.stream(event),
            KernelEvent::ToolCallStarted {
                tool_name, args, ..
            } => {
                self.ensure_newline();
                let mut summary = args.to_string();
                if summary.chars().count() > 120 {
                    summary = summary.chars().take(119).chain(['…']).collect();
                }
                println!("[tool] {tool_name}({summary})");
                let _ = std::io::stdout().flush();
            }
            KernelEvent::ToolCallFinished {
                tool_name,
                result,
                duration_ms,
                ..
            } => {
                let failed = matches!(result.as_ref(), Message::ToolResult(r) if r.is_error);
                println!(
                    "[tool] {tool_name} {} ({} ms)",
                    if failed { "failed" } else { "done" },
                    duration_ms.unwrap_or(0)
                );
            }
            KernelEvent::ToolCallUpdate { partial, .. } => {
                let lines = update_lines(&partial.content);
                if !lines.is_empty() {
                    self.ensure_newline();
                    for line in lines {
                        println!("  {line}");
                    }
                    let _ = std::io::stdout().flush();
                }
            }
            KernelEvent::Error { context } => {
                self.ensure_newline();
                eprintln!("[error] {context}");
            }
            KernelEvent::AgentFinished { messages } => {
                self.ensure_newline();
                if let Some(usage) = last_usage(messages) {
                    println!(
                        "[usage] input={} output={} cache_read={} cache_write={}",
                        usage.input, usage.output, usage.cache_read, usage.cache_write
                    );
                }
            }
            _ => {}
        }
    }

    fn stream(&mut self, event: &AssistantStreamEvent) {
        match event {
            AssistantStreamEvent::Delta {
                index,
                delta,
                partial,
            } => {
                if matches!(partial.content.get(*index), Some(Content::Text { .. })) {
                    print!("{delta}");
                    let _ = std::io::stdout().flush();
                    self.at_line_start = false;
                }
            }
            AssistantStreamEvent::BlockEnd { index, partial } => {
                if matches!(partial.content.get(*index), Some(Content::Text { .. })) {
                    self.ensure_newline();
                }
            }
            _ => {}
        }
    }

    fn ensure_newline(&mut self) {
        if !self.at_line_start {
            println!();
            self.at_line_start = true;
        }
    }
}

fn last_usage(messages: &[Message]) -> Option<&Usage> {
    messages.iter().rev().find_map(|m| match m {
        Message::Assistant(a) => Some(&a.usage),
        _ => None,
    })
}

/// 工具部分结果里的文本行（图片等非文本块不打印）。
fn update_lines(content: &[Content]) -> Vec<&str> {
    content
        .iter()
        .filter_map(|block| match block {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn update_lines_keep_text_and_skip_images() {
        let content = vec![
            Content::Text {
                text: "→ tools.bash".into(),
            },
            Content::Image {
                data: "abcd".into(),
                mime_type: "image/png".into(),
            },
            Content::Text {
                text: "← bash ok".into(),
            },
        ];
        assert_eq!(update_lines(&content), ["→ tools.bash", "← bash ok"]);
    }
}
