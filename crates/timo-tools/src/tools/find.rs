//! `find` 工具（设计 03 §3 D1：`ignore` 遍历 + glob；即 roadmap 所称"glob"）。
//!
//! 输出 posix 相对路径逐行；50KB 截断；limit=1000 默认（pi find.ts 同构）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::Result;
use crate::definition::{ToolCallCtx, ToolDefinition, ToolOutput};
use crate::error::ToolError;
use crate::tools::write::str_param;
use crate::truncate::truncate_head;

/// 参数 schema。
#[must_use]
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pattern": {"type": "string", "description": "Glob pattern"},
            "path": {"type": "string"},
            "limit": {"type": "number"}
        },
        "required": ["pattern"]
    })
}

/// 构造 find 工具定义。
#[must_use]
pub fn definition(cwd: Arc<Path>) -> ToolDefinition {
    ToolDefinition {
        name: "find".into(),
        label: "Find".into(),
        description: "Find files by glob pattern (fd semantics). Respects .gitignore; \
                      hidden files included; pattern with `/` matches full path."
            .into(),
        prompt_snippet: Some("find(pattern, path?, limit=1000?): find files by glob".into()),
        prompt_guidelines: vec![],
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
    let limit = ctx
        .params
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(1000) as usize;
    let search_path = base.clone();
    let out = tokio::task::spawn_blocking(move || find(&search_path, &pattern, limit))
        .await
        .map_err(|e| ToolError::Subprocess {
            source: std::io::Error::other(e.to_string()),
        })??;
    Ok(out)
}

/// glob 遍历（pattern 含 `/` → 全路径匹配，`**/` 前缀；fd --full-path 同构）。
fn find(base: &Path, pattern: &str, limit: usize) -> Result<ToolOutput> {
    let full_path = pattern.contains('/');
    let glob = if full_path {
        let norm = pattern.replace('\\', "/");
        format!("**/{norm}")
    } else {
        pattern.to_string()
    };
    let mut builder = ignore::WalkBuilder::new(base);
    builder.hidden(false);
    builder.threads(4);
    let mut overrides = ignore::overrides::OverrideBuilder::new(base);
    overrides.add(&glob).map_err(|e| ToolError::InvalidParams {
        reason: format!("bad glob: {e}"),
    })?;
    let ov = overrides.build().map_err(|e| ToolError::InvalidParams {
        reason: format!("bad overrides: {e}"),
    })?;
    builder.overrides(ov);

    let mut out: Vec<String> = vec![];
    for entry in builder.build().flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if out.len() >= limit {
            out.push(format!(
                "[{} results limit reached. Use limit={} for more, or refine pattern]",
                limit,
                limit * 2
            ));
            break;
        }
        let rel = entry
            .path()
            .strip_prefix(base)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        out.push(rel);
    }
    if out.is_empty() {
        return Ok(ToolOutput::text("No files found matching pattern"));
    }
    let joined = out.join("\n");
    let (body, truncated) = truncate_head(&joined);
    let mut body = body;
    if truncated {
        body.push_str("\n[50KB limit reached]");
    }
    Ok(ToolOutput::text(body))
}

#[allow(dead_code)]
fn unused(_: PathBuf) {}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn finds_by_glob_and_full_path() {
        let dir = std::env::temp_dir().join(format!("timo-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub/deep")).unwrap();
        std::fs::write(dir.join("a.rs"), "").unwrap();
        std::fs::write(dir.join("sub/b.rs"), "").unwrap();
        std::fs::write(dir.join("sub/deep/c.txt"), "").unwrap();
        let out = find(&dir, "*.rs", 1000).unwrap();
        let timo_ai::Content::Text { text } = &out.content[0] else {
            panic!("text")
        };
        assert!(
            text.contains("a.rs") && text.contains("b.rs") && !text.contains("c.txt"),
            "{text}"
        );
        let out2 = find(&dir, "sub/b.rs", 1000).unwrap();
        let timo_ai::Content::Text { text: t2 } = &out2.content[0] else {
            panic!("text")
        };
        assert!(t2.contains("sub/b.rs"), "{t2}");
    }

    #[tokio::test]
    async fn empty_result_message() {
        let dir = std::env::temp_dir().join(format!("timo-find-e-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = find(&dir, "*.nope", 1000).unwrap();
        assert_eq!(
            out.content[0],
            timo_ai::Content::Text {
                text: "No files found matching pattern".into()
            }
        );
    }
}
