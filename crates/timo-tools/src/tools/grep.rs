//! `grep` 工具（设计 03 §3 D1：`ignore` 遍历 + `regex` 正则，ripgrep 同方言）。
//!
//! 输出格式对齐 pi grep.ts：`relpath:LINE: text`（上下文行 `relpath:LINE- text`）、
//! 行截断 500、总量 50KB 头截断、limit 注记。

use std::path::Path;
use std::sync::Arc;

use regex::RegexBuilder;
use serde_json::{Value, json};

use crate::Result;
use crate::definition::{ToolCallCtx, ToolDefinition, ToolOutput};
use crate::error::ToolError;
use crate::tools::write::str_param;
use crate::truncate::{GREP_MAX_LINE, truncate_head};

/// 参数 schema。
#[must_use]
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pattern": {"type": "string"},
            "path": {"type": "string"},
            "glob": {"type": "string"},
            "ignoreCase": {"type": "boolean"},
            "literal": {"type": "boolean"},
            "context": {"type": "number"},
            "limit": {"type": "number"}
        },
        "required": ["pattern"]
    })
}

/// 构造 grep 工具定义。
#[must_use]
pub fn definition(cwd: Arc<Path>) -> ToolDefinition {
    ToolDefinition {
        name: "grep".into(),
        label: "Grep".into(),
        description: "Search file contents with a regular expression (ripgrep dialect). \
                      Respects .gitignore; hidden files included."
            .into(),
        prompt_snippet: Some(
            "grep(pattern, path?, glob?, ignoreCase?, literal?, context?, limit=100?)".into(),
        ),
        prompt_guidelines: vec![
            "Use `literal: true` for plain-text patterns containing regex metacharacters.".into(),
        ],
        parameters: schema(),
        output_schema: None,
        exposure: crate::definition::Exposure::Direct,
        namespace: None,
        annotations: crate::definition::Annotations {
            read_only_hint: Some(true),
            ..Default::default()
        },
        execution_mode: crate::definition::ExecutionMode::Parallel,
        execute: Arc::new(move |ctx| Box::pin(execute(ctx, cwd.clone()))),
    }
}

async fn execute(ctx: ToolCallCtx, cwd: Arc<Path>) -> Result<ToolOutput> {
    let _ = ctx.updates;
    let pattern = str_param(&ctx.params, "pattern")?.to_string();
    let base = match ctx.params.get("path").and_then(Value::as_str) {
        Some(p) => crate::tools::read::resolve(&cwd, p)?,
        None => cwd.to_path_buf(),
    };
    let ignore_case = flag(&ctx.params, "ignoreCase");
    let literal = flag(&ctx.params, "literal");
    let context = ctx
        .params
        .get("context")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let limit = ctx
        .params
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(100) as usize;
    let glob = ctx
        .params
        .get("glob")
        .and_then(Value::as_str)
        .map(String::from);
    let regex = build_regex(&pattern, ignore_case, literal)?;
    let search_path = base.clone();
    let out = tokio::task::spawn_blocking(move || {
        search(&search_path, glob.as_deref(), &regex, context, limit)
    })
    .await
    .map_err(|e| ToolError::Subprocess {
        source: std::io::Error::other(e.to_string()),
    })??;
    Ok(out)
}

fn flag(params: &Value, key: &str) -> bool {
    params.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn build_regex(pattern: &str, ignore_case: bool, literal: bool) -> Result<regex::Regex> {
    let source = if literal {
        regex::escape(pattern)
    } else {
        pattern.to_string()
    };
    RegexBuilder::new(&source)
        .case_insensitive(ignore_case)
        .build()
        .map_err(|e| ToolError::InvalidParams {
            reason: format!("bad pattern: {e}"),
        })
}

/// 遍历 + 匹配（阻塞执行；尊重 .gitignore、含隐藏文件——pi --hidden 同构）。
fn search(
    base: &Path,
    glob: Option<&str>,
    regex: &regex::Regex,
    context: usize,
    limit: usize,
) -> Result<ToolOutput> {
    let mut builder = ignore::WalkBuilder::new(base);
    builder.hidden(false); // 含隐藏文件（pi --hidden）
    builder.threads(4);
    let mut overrides = ignore::overrides::OverrideBuilder::new(base);
    if let Some(g) = glob {
        overrides.add(g).map_err(|e| ToolError::InvalidParams {
            reason: format!("bad glob: {e}"),
        })?;
    }
    let ov = overrides.build().map_err(|e| ToolError::InvalidParams {
        reason: format!("bad overrides: {e}"),
    })?;
    builder.overrides(ov);

    let mut lines_out: Vec<String> = vec![];
    let mut match_count = 0usize;
    let mut limit_hit = false;
    for entry in builder.build() {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        if let Some(hits) = match_file(path, regex, context) {
            for line in hits {
                if match_count >= limit {
                    limit_hit = true;
                    break;
                }
                if is_match_line(&line) {
                    match_count += 1;
                }
                lines_out.push(line);
            }
        }
        if limit_hit {
            break;
        }
    }
    if lines_out.is_empty() {
        return Ok(ToolOutput::text("No matches found"));
    }
    if limit_hit {
        lines_out.push(format!(
            "[{match_count} matches limit reached. Use limit={} for more, or refine pattern]",
            limit * 2
        ));
    }
    let joined = lines_out.join("\n");
    let (body, truncated) = truncate_head(&joined);
    let mut body = body;
    if truncated {
        body.push_str("\n[50KB limit reached]");
    }
    Ok(ToolOutput::text(body))
}

/// 单文件匹配 → 输出行（含上下文；行截断 500）。
fn match_file(path: &Path, regex: &regex::Regex, context: usize) -> Option<Vec<String>> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None; // 二进制跳过（rg 同构）
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let rel = relative_to(path);
    let mut out: Vec<String> = vec![];
    let mut last_end: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if regex.is_match(line) {
            let start = i.saturating_sub(context);
            // 组间分隔
            if let Some(prev_end) = last_end
                && start > prev_end + 1
            {
                out.push("-".to_string());
            }
            for j in start..i {
                out.push(format!("{rel}:{}- {}", j + 1, cut(line_text(&lines, j))));
            }
            out.push(format!("{rel}:{}: {}", i + 1, cut(line)));
            for j in (i + 1)..=(i + context) {
                if j >= lines.len() {
                    break;
                }
                out.push(format!("{rel}:{}- {}", j + 1, cut(line_text(&lines, j))));
            }
            last_end = Some(i + context);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn line_text<'a>(lines: &[&'a str], i: usize) -> &'a str {
    lines.get(i).copied().unwrap_or("")
}

fn is_match_line(s: &str) -> bool {
    // 匹配行含 ":LINE:"；上下文行是 ":LINE-"
    s.matches(':').count() >= 2 && !s.ends_with('-') && s != "-"
}

fn cut(line: &str) -> String {
    if line.len() > GREP_MAX_LINE {
        let mut c = GREP_MAX_LINE;
        while !line.is_char_boundary(c) {
            c += 1;
        }
        format!("{}... [truncated]", &line[..c])
    } else {
        line.to_string()
    }
}

/// 相对路径显示（相对 cwd；posix 分隔符）。
fn relative_to(path: &Path) -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    path.strip_prefix(&cwd)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn literal_vs_regex() {
        assert!(build_regex("a.b", false, true).is_ok());
        assert!(build_regex("[invalid", false, false).is_err());
    }

    #[tokio::test]
    async fn finds_with_format() {
        let dir = std::env::temp_dir().join(format!("timo-grep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "alpha\nbeta target\ngamma\n").unwrap();
        std::fs::write(dir.join("b.txt"), "nope\n").unwrap();
        let regex = build_regex("target", false, false).unwrap();
        let out = search(&dir, None, &regex, 1, 100).unwrap();
        let timo_ai::Content::Text { text } = &out.content[0] else {
            panic!("text")
        };
        assert!(text.contains(":2: beta target"), "{text}");
        assert!(text.contains(":1- alpha"));
    }

    #[tokio::test]
    async fn no_matches_message() {
        let dir = std::env::temp_dir().join(format!("timo-grep-nm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let regex = build_regex("zzz-not-there", false, false).unwrap();
        let out = search(&dir, None, &regex, 0, 100).unwrap();
        assert_eq!(
            out.content[0],
            timo_ai::Content::Text {
                text: "No matches found".into()
            }
        );
    }
}
