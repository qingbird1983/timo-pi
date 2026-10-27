//! 脚本选项行、输出格式与截断（设计 phase-3.5/11 §4.3/§4.4）。

use std::io::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use serde_json::json;
use timo_tools::{CallableTool, NestedCallRecord, NestedCallStatus, NestedToolHost, ToolOutput};

/// 脚本线程交回的原始输出。
pub(super) struct ScriptRun {
    /// `text()` 的文本项（按序）。
    pub items: Vec<String>,
    /// `console.*` 行（在所有文本项之后合成一个块）。
    pub console: Vec<String>,
    /// 脚本错误（None = 成功；`exit()` 不算错误）。
    pub error: Option<String>,
}

/// 默认输出 token 预算（pi 同值）。
pub(super) const DEFAULT_MAX_OUTPUT_TOKENS: usize = 10_000;
/// 每 token 估算字符数（pi 同值）。
const CHARS_PER_TOKEN: usize = 4;
/// 选项行的前缀。
const OPTIONS_PREFIX: &str = "// @options:";

/// 解析后的脚本源。
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Source {
    /// 去掉选项行后的脚本体。
    pub body: String,
    /// 输出 token 预算。
    pub max_output_tokens: usize,
    /// 硬 deadline（毫秒；None = 不设）。
    pub timeout_ms: Option<u64>,
}

/// 解析选项行；`An options line anywhere else fails the call before the script runs`。
pub(super) fn parse_source(code: &str) -> Result<Source, String> {
    let mut lines = code.split('\n');
    let first = lines.next().unwrap_or("").trim_end_matches('\r');
    let mut out = Source {
        body: code.to_string(),
        max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
        timeout_ms: None,
    };
    if let Some(rest) = first.trim_start().strip_prefix(OPTIONS_PREFIX) {
        let value: serde_json::Value = serde_json::from_str(rest.trim()).map_err(|error| {
            format!("bad @options line: {error}. Expected a JSON object on the first line.")
        })?;
        let object = value
            .as_object()
            .ok_or_else(|| "bad @options line: expected a JSON object".to_string())?;
        if let Some(tokens) = object.get("max_output_tokens") {
            out.max_output_tokens = positive(tokens, "max_output_tokens")?;
        }
        if let Some(ms) = object.get("timeout_ms") {
            out.timeout_ms = Some(positive(ms, "timeout_ms")? as u64);
        }
        out.body = lines.collect::<Vec<_>>().join("\n");
        return Ok(out);
    }
    if code
        .lines()
        .skip(1)
        .any(|line| line.trim_start().starts_with(OPTIONS_PREFIX))
    {
        return Err("the @options line must be the first line of the script".to_string());
    }
    Ok(out)
}

fn positive(value: &serde_json::Value, name: &str) -> Result<usize, String> {
    let number = value
        .as_u64()
        .filter(|number| *number > 0)
        .ok_or_else(|| format!("@options {name} must be a positive integer"))?;
    usize::try_from(number).map_err(|_| format!("@options {name} is too large"))
}

/// 把脚本输出合成模型可见文本；超预算时保留头尾并把全文写临时文件。
pub(super) fn render_output(
    items: &[String],
    console: &[String],
    max_output_tokens: usize,
) -> (String, Option<PathBuf>) {
    let total = items.len();
    let mut parts = Vec::with_capacity(items.len() + 1);
    for (index, text) in items.iter().enumerate() {
        if total > 1 {
            parts.push(format!("==> text {}/{} <==\n{text}", index + 1, total));
        } else {
            parts.push(text.clone());
        }
    }
    if !console.is_empty() {
        parts.push(format!(
            "<console_output>\n{}\n</console_output>",
            console.join("\n")
        ));
    }
    let text = parts.join("\n");
    let budget = max_output_tokens.saturating_mul(CHARS_PER_TOKEN);
    if char_len(&text) <= budget {
        return (text, None);
    }
    let head_chars = budget / 2;
    let tail_chars = budget.saturating_sub(head_chars);
    let original_tokens = char_len(&text).div_ceil(CHARS_PER_TOKEN);
    let lines = text.lines().count();
    let removed = char_len(&text)
        .saturating_sub(head_chars)
        .saturating_sub(tail_chars);
    let mut truncated = format!(
        "Warning: truncated output (original token count: {original_tokens})\n\
Total output lines: {lines}\n\n{}{}",
        slice_chars(&text, 0, head_chars),
        if tail_chars > 0 {
            format!(
                "…{} tokens truncated…{}",
                removed.div_ceil(CHARS_PER_TOKEN),
                slice_chars(
                    &text,
                    char_len(&text).saturating_sub(tail_chars),
                    tail_chars
                )
            )
        } else {
            String::new()
        }
    );
    match spill(&text) {
        Ok(path) => {
            truncated.push_str(&format!(
                "\n\n[Full output: {} (read with offset/limit)]",
                path.display()
            ));
            (truncated, Some(path))
        }
        Err(error) => {
            truncated.push_str(&format!("\n\n[Could not save the full output: {error}]"));
            (truncated, None)
        }
    }
}

/// 失败时的调用清单（未发生的调用不回滚）。
pub(super) fn call_summary(calls: &[timo_tools::NestedCallRecord]) -> Option<String> {
    if calls.is_empty() {
        return None;
    }
    let rows: Vec<String> = calls
        .iter()
        .map(|record| {
            let status = match record.status {
                timo_tools::NestedCallStatus::Ok => "ok",
                timo_tools::NestedCallStatus::Error => "error",
                timo_tools::NestedCallStatus::Cancelled | timo_tools::NestedCallStatus::Running => {
                    "cancelled"
                }
            };
            format!("{} ({status})", record.name)
        })
        .collect();
    Some(format!(
        "Tool calls made before the failure (they are not undone): {}",
        rows.join(", ")
    ))
}

fn char_len(text: &str) -> usize {
    text.chars().count()
}

fn slice_chars(text: &str, start: usize, len: usize) -> String {
    text.chars().skip(start).take(len).collect()
}

fn spill(text: &str) -> std::io::Result<PathBuf> {
    let mut file = tempfile::Builder::new()
        .prefix("timo-codemode-")
        .suffix(".txt")
        .tempfile_in(std::env::temp_dir())?;
    file.write_all(text.as_bytes())?;
    file.flush()?;
    let path = file.into_temp_path().keep().map_err(|error| error.error)?;
    Ok(path)
}

/// 嵌套结果 → 脚本值（设计 §4.2 值映射）。
pub(super) fn script_value(tool: &CallableTool, name: &str, output: &ToolOutput) -> (bool, String) {
    if tool.output_schema.is_some()
        && let Some(structured) = &output.structured_content
    {
        return (
            true,
            serde_json::to_string(structured).unwrap_or_else(|_| "null".to_string()),
        );
    }
    let text = script_text(output);
    if output.is_error {
        (
            false,
            if text.is_empty() {
                format!("Tool \"{name}\" failed")
            } else {
                text
            },
        )
    } else {
        (
            true,
            serde_json::to_string(&text).unwrap_or_else(|_| "\"\"".to_string()),
        )
    }
}

fn script_text(output: &ToolOutput) -> String {
    output
        .content
        .iter()
        .filter_map(|block| match block {
            timo_ai::Content::Text { text } => Some(text.clone()),
            timo_ai::Content::Image { data, mime_type } => Some(format!(
                "[image omitted: {mime_type}, {} base64 chars]",
                data.len()
            )),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 脚本结束后合成模型可见结果（含调用清单与全文路径）。
pub(super) fn finish_output(
    result: Result<ScriptRun, tokio::sync::oneshot::error::RecvError>,
    host: &dyn NestedToolHost,
    parent_id: &str,
    started: Instant,
    max_output_tokens: usize,
) -> ToolOutput {
    let calls = host
        .snapshot(parent_id)
        .map(|summary| summary.calls)
        .unwrap_or_default();
    let run = match result {
        Ok(run) => run,
        Err(error) => ScriptRun {
            items: Vec::new(),
            console: Vec::new(),
            error: Some(format!("Script sandbox failed: {error}")),
        },
    };
    let failed = run.error.is_some();
    let (body, full_path) = render_output(&run.items, &run.console, max_output_tokens);
    let elapsed = started.elapsed().as_secs_f64();
    let mut text = format!(
        "{}\nWall time {elapsed:.1} seconds\nOutput:\n{body}",
        if failed {
            "Script failed"
        } else {
            "Script completed"
        }
    );
    if let Some(error) = run.error {
        text.push_str("\n\nScript error:\n");
        text.push_str(&error);
        if let Some(summary) = call_summary(&calls) {
            text.push_str("\n\n");
            text.push_str(&summary);
        }
    }
    let mut details = json!({ "calls": records_json(&calls) });
    if let Some(path) = full_path {
        details["full_output_path"] = json!(path.display().to_string());
    }
    ToolOutput {
        content: vec![timo_ai::Content::Text { text }],
        details,
        is_error: failed,
        ..ToolOutput::default()
    }
}

/// 脚本在启动前就失败（选项行、worker 准入等）的结果。
pub(super) fn failure_output(
    error: &str,
    started: Instant,
    calls: &[NestedCallRecord],
) -> ToolOutput {
    let elapsed = started.elapsed().as_secs_f64();
    let mut text = format!("Script failed\nWall time {elapsed:.1} seconds\nOutput:\n");
    text.push_str("\nScript error:\n");
    text.push_str(error);
    if let Some(summary) = call_summary(calls) {
        text.push_str("\n\n");
        text.push_str(&summary);
    }
    ToolOutput {
        content: vec![timo_ai::Content::Text { text }],
        details: json!({ "calls": records_json(calls) }),
        is_error: true,
        ..ToolOutput::default()
    }
}

/// 嵌套调用状态变化的进度快照（`content` 一行文本，`details.calls` 给 UI）。
pub(super) fn progress(host: &dyn NestedToolHost, parent_id: &str, line: String) -> ToolOutput {
    let calls = host
        .snapshot(parent_id)
        .map(|summary| summary.calls)
        .unwrap_or_default();
    ToolOutput {
        content: vec![timo_ai::Content::Text { text: line }],
        details: json!({ "calls": records_json(&calls) }),
        ..ToolOutput::default()
    }
}

fn records_json(calls: &[NestedCallRecord]) -> Vec<serde_json::Value> {
    calls
        .iter()
        .map(|record| {
            json!({
                "id": record.id,
                "name": record.name,
                "status": match record.status {
                    NestedCallStatus::Running => "running",
                    NestedCallStatus::Ok => "ok",
                    NestedCallStatus::Error => "error",
                    NestedCallStatus::Cancelled => "cancelled",
                },
                "duration_ms": record.duration_ms,
                "error": record.error,
            })
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn options_must_be_the_first_line() {
        let source =
            parse_source("// @options: {\"max_output_tokens\": 7, \"timeout_ms\": 50}\nreturn 1")
                .unwrap();
        assert_eq!(source.max_output_tokens, 7);
        assert_eq!(source.timeout_ms, Some(50));
        assert_eq!(source.body, "return 1");

        let late = parse_source("text(1);\n// @options: {\"timeout_ms\": 1}").unwrap_err();
        assert!(late.contains("first line"), "{late}");
        let bad = parse_source("// @options: nope\nreturn 1").unwrap_err();
        assert!(bad.contains("bad @options"), "{bad}");
        let zero = parse_source("// @options: {\"timeout_ms\": 0}").unwrap_err();
        assert!(zero.contains("positive"), "{zero}");
        assert_eq!(
            parse_source("return 1").unwrap(),
            Source {
                body: "return 1".into(),
                max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
                timeout_ms: None,
            }
        );
    }

    #[test]
    fn multi_text_items_and_console_are_separated() {
        let (text, path) = render_output(
            &["a".into(), "b".into()],
            &["one".into(), "two".into()],
            10_000,
        );
        assert!(path.is_none());
        assert_eq!(
            text,
            "==> text 1/2 <==\na\n==> text 2/2 <==\nb\n<console_output>\none\ntwo\n</console_output>"
        );
    }

    #[test]
    fn long_output_keeps_head_and_tail_and_spills_the_rest() {
        let body = "x".repeat(1000);
        let (text, path) = render_output(std::slice::from_ref(&body), &[], 10);
        let path = path.expect("spilled");
        assert!(text.starts_with("Warning: truncated output"), "{text}");
        assert!(text.contains("tokens truncated"), "{text}");
        assert!(text.ends_with(&format!(
            "[Full output: {} (read with offset/limit)]",
            path.display()
        )));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn value_mapping_prefers_structured_and_marks_images() {
        let callable = |schema| CallableTool {
            name: "obj".into(),
            description: String::new(),
            parameters: json!({"type": "object"}),
            output_schema: schema,
            namespace: None,
            prompt_guidelines: vec![],
            execution_mode: timo_tools::ExecutionMode::Parallel,
            exposure: timo_tools::Exposure::Direct,
        };
        let structured = ToolOutput {
            structured_content: Some(json!({"a": 1})),
            ..ToolOutput::text("ignored")
        };
        let with_schema = callable(Some(json!({"type": "object"})));
        assert_eq!(
            script_value(&with_schema, "obj", &structured),
            (true, "{\"a\":1}".to_string())
        );
        let failed_structured = ToolOutput {
            is_error: true,
            ..structured
        };
        assert!(
            script_value(&with_schema, "obj", &failed_structured).0,
            "错误结果带结构化值也按结构化解析（设计 11 §4.2）"
        );

        let failed = ToolOutput::error_text("nope");
        assert_eq!(
            script_value(&callable(None), "t", &failed),
            (false, "nope".to_string())
        );
        assert_eq!(
            script_value(&callable(None), "t", &ToolOutput::error_text("")),
            (false, "Tool \"t\" failed".to_string())
        );

        let image = ToolOutput {
            content: vec![timo_ai::Content::Image {
                data: "abcd".into(),
                mime_type: "image/png".into(),
            }],
            ..ToolOutput::default()
        };
        assert_eq!(
            script_text(&image),
            "[image omitted: image/png, 4 base64 chars]"
        );
    }

    #[test]
    fn call_summary_lists_statuses() {
        use timo_tools::{NestedCallRecord, NestedCallStatus};
        let record = |name: &str, status| NestedCallRecord {
            id: "call/1".into(),
            name: name.into(),
            arguments: None,
            arguments_bytes: None,
            status,
            duration_ms: Some(1),
            error: None,
        };
        let summary = call_summary(&[
            record("bash", NestedCallStatus::Ok),
            record("read", NestedCallStatus::Error),
            record("edit", NestedCallStatus::Running),
        ])
        .unwrap();
        assert_eq!(
            summary,
            "Tool calls made before the failure (they are not undone): bash (ok), read (error), edit (cancelled)"
        );
        assert!(call_summary(&[]).is_none());
    }
}
