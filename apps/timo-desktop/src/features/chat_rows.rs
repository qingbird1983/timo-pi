//! 聊天行渲染：用户气泡、assistant Markdown 与工具 trace（设计 01 §5~§6）。

use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::chat::{AssistantRow, ChatState, RowKind, TraceEntry, TraceStatus};

/// 渲染一行（`MessageScroller` 的回调）。
pub fn render_row(index: usize, chat: &Entity<ChatState>, cx: &mut App) -> AnyElement {
    let state = chat.read(cx);
    let Some(row) = state.rows.get(index) else {
        return div().into_any_element();
    };
    match &row.kind {
        RowKind::User(text) => h_flex()
            .w_full()
            .justify_end()
            .child(
                div()
                    .max_w(px(560.))
                    .rounded_lg()
                    .bg(cx.theme().secondary)
                    .px_3()
                    .py_2()
                    .text_sm()
                    .child(text.clone()),
            )
            .into_any_element(),
        RowKind::Assistant(assistant) => {
            assistant_row(&row.key, assistant, chat, cx).into_any_element()
        }
    }
}

fn assistant_row(
    key: &str,
    assistant: &AssistantRow,
    chat: &Entity<ChatState>,
    cx: &App,
) -> impl IntoElement {
    let has_text = !assistant.text.trim().is_empty();
    v_flex()
        .w_full()
        .gap_2()
        .when(!assistant.thinking.trim().is_empty(), |this| {
            this.child(
                div()
                    .pl_2()
                    .border_l_2()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(assistant.thinking.clone()),
            )
        })
        .when(has_text, |this| {
            this.child(TextView::markdown(
                format!("md-{key}"),
                assistant.text.clone(),
            ))
        })
        .when(!has_text && assistant.streaming, |this| {
            this.child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(Icon::new(IconName::Loader).xsmall())
                    .child(t!("chat.generating").to_string()),
            )
        })
        .when(!assistant.trace.is_empty(), |this| {
            this.child(trace_block(key, assistant, chat, cx))
        })
        .when_some(assistant.error.clone(), |this, error| {
            this.child(div().text_xs().text_color(cx.theme().danger).child(error))
        })
}

fn trace_block(
    key: &str,
    assistant: &AssistantRow,
    chat: &Entity<ChatState>,
    cx: &App,
) -> impl IntoElement {
    let expanded = assistant.trace_open;
    let chat = chat.clone();
    let row_key = key.to_string();
    v_flex()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .id(format!("trace-{key}"))
                .px_2()
                .py_1()
                .gap_2()
                .items_center()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(IconName::SquareTerminal).xsmall())
                .child(t!("chat.trace_title", n = assistant.trace.len()).to_string())
                .child(div().flex_1())
                .child(
                    Icon::new(if expanded {
                        IconName::ChevronUp
                    } else {
                        IconName::ChevronDown
                    })
                    .xsmall(),
                )
                .on_click(move |_, _, cx| {
                    chat.update(cx, |chat, cx| {
                        chat.toggle_trace(&row_key);
                        cx.notify();
                    });
                }),
        )
        .when(expanded, |this| {
            this.child(
                v_flex()
                    .px_2()
                    .pb_2()
                    .gap_1()
                    .children(assistant.trace.iter().map(|entry| trace_entry(entry, cx))),
            )
        })
}

fn trace_entry(entry: &TraceEntry, cx: &App) -> impl IntoElement {
    let (icon, color, status) = match entry.status {
        TraceStatus::Queued => (
            IconName::Loader,
            cx.theme().muted_foreground,
            t!("chat.tool_queued").to_string(),
        ),
        TraceStatus::Running => (
            IconName::Loader,
            cx.theme().primary,
            t!("chat.tool_running").to_string(),
        ),
        TraceStatus::Done => (
            IconName::Check,
            cx.theme().success,
            t!("chat.tool_done").to_string(),
        ),
        TraceStatus::Failed => (
            IconName::CircleX,
            cx.theme().danger,
            t!("chat.tool_failed").to_string(),
        ),
        TraceStatus::Interrupted => (
            IconName::CircleAlert,
            cx.theme().warning,
            t!("chat.tool_interrupted").to_string(),
        ),
    };
    let duration = entry
        .duration_ms
        .map(|ms| format!(" · {:.2}s", ms as f64 / 1000.))
        .unwrap_or_default();
    let output = entry
        .output
        .as_deref()
        .map(trace_output)
        .filter(|text| !text.trim().is_empty());
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .text_xs()
                .child(Icon::new(icon).xsmall().text_color(color))
                .child(
                    div()
                        .text_color(cx.theme().foreground)
                        .child(tool_label(&entry.name)),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(cx.theme().muted_foreground)
                        .child(entry.summary.clone()),
                )
                .child(div().flex_1())
                .child(div().text_color(color).child(format!("{status}{duration}"))),
        )
        .when_some(output, |this, text| {
            this.child(
                div()
                    .pl_5()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text),
            )
        })
}

/// trace 内联输出上限：200 行或 32 KiB（设计 01 §6）。
fn trace_output(text: &str) -> String {
    const MAX_LINES: usize = 200;
    const MAX_BYTES: usize = 32 * 1024;
    let total_lines = text.lines().count();
    let mut out = String::new();
    for (index, line) in text.lines().enumerate() {
        if index == MAX_LINES || out.len() + line.len() + 1 > MAX_BYTES {
            out.push_str(t!("chat.trace_truncated", n = total_lines).as_ref());
            out.push('\n');
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim_end().to_string()
}

fn tool_label(name: &str) -> String {
    match name {
        "read" => t!("tool.read").to_string(),
        "write" => t!("tool.write").to_string(),
        "edit" => t!("tool.edit").to_string(),
        "bash" => t!("tool.bash").to_string(),
        "powershell" => t!("tool.powershell").to_string(),
        "grep" => t!("tool.grep").to_string(),
        "find" => t!("tool.find").to_string(),
        other if other.starts_with("mcp__") => "MCP".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::trace_output;

    #[test]
    fn short_output_is_untouched() {
        assert_eq!(trace_output("a\nb"), "a\nb");
    }

    #[test]
    fn long_output_is_truncated_to_200_lines() {
        crate::i18n::init();
        let text = (0..500)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = trace_output(&text);
        assert_eq!(out.lines().count(), 201);
        assert!(out.lines().last().is_some_and(|line| line.contains("500")));
    }
}
