//! `edit` 工具（设计 03 §3；pi edit.ts 同构：精确优先 + 规范化模糊回退）。
//!
//! 模糊匹配规则：NFKC；逐行 trimEnd；智能引号→ASCII；Unicode 破折号族→`-`；
//! NBSP 族→空格。模糊命中时新文本按行写回（仅触及行），原文件其他行原样保留。

use std::sync::Arc;

use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization as _;

use crate::Result;
use crate::definition::{ToolCallCtx, ToolDefinition, ToolOutput};
use crate::error::ToolError;
use crate::mutate::with_file_mutation_queue;
use crate::tools::read::resolve;
use crate::tools::write::str_param;

mod apply;
use self::apply::{apply_to_file, apply_with_review};

/// 参数 schema。
#[must_use]
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string"},
            "edits": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "oldText": {"type": "string"},
                        "newText": {"type": "string"}
                    },
                    "required": ["oldText", "newText"]
                }
            }
        },
        "required": ["path", "edits"]
    })
}

/// 构造 edit 工具定义。
#[must_use]
pub fn definition(cwd: Arc<std::path::Path>) -> ToolDefinition {
    ToolDefinition {
        name: "edit".into(),
        label: "Edit".into(),
        description: "Replace exact text blocks in a file. oldText must match the file exactly \
                      (whitespace included) and be unique; a normalized fuzzy pass runs as fallback."
            .into(),
        prompt_snippet: Some(
            "edit(path, edits=[{oldText,newText},…]): exact-match text replacement".into(),
        ),
        prompt_guidelines: vec![
            "Include enough surrounding context in oldText to make it unique.".into(),
            "Multiple edits are matched against the original file; ranges must not overlap.".into(),
        ],
        parameters: schema(),
        output_schema: None,
        exposure: crate::definition::Exposure::Direct,
        namespace: None,
        annotations: crate::definition::Annotations {
            destructive_hint: Some(true),
            ..Default::default()
        },
        execution_mode: crate::definition::ExecutionMode::Parallel,
        execute: Arc::new(move |ctx| Box::pin(execute(ctx, cwd.clone()))),
    }
}

async fn execute(ctx: ToolCallCtx, cwd: Arc<std::path::Path>) -> Result<ToolOutput> {
    let _ = ctx.updates;
    let path = resolve(&cwd, str_param(&ctx.params, "path")?)?;
    let edits = parse_edits(&ctx.params)?;
    with_file_mutation_queue(&path, || {
        let path = path.clone();
        let edits = edits.clone();
        let review = ctx.review.clone();
        let cancel = ctx.cancel.clone();
        let tool_call_id = ctx.tool_call_id.clone();
        let cwd = cwd.clone();
        async move {
            match review {
                Some(review) => {
                    apply_with_review(&path, &edits, review, &tool_call_id, &cwd, &cancel).await
                }
                // CLI/Phase 1/2：无审阅器时走原路径，不构造请求。
                None => apply_to_file(&path, &edits).await,
            }
        }
    })
    .await
}

#[derive(Debug, Clone)]
struct Edit {
    old: String,
    new: String,
}

fn parse_edits(params: &Value) -> Result<Vec<Edit>> {
    let arr = params
        .get("edits")
        .and_then(Value::as_array)
        .ok_or_else(|| ToolError::InvalidParams {
            reason: "missing array param `edits`".into(),
        })?;
    let mut out = vec![];
    for e in arr {
        let old =
            e.get("oldText")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::InvalidParams {
                    reason: "edit missing `oldText`".into(),
                })?;
        if old.is_empty() {
            return Err(ToolError::InvalidParams {
                reason: "oldText must not be empty".into(),
            });
        }
        let new = e.get("newText").and_then(Value::as_str).unwrap_or("");
        if old == new {
            return Err(ToolError::InvalidParams {
                reason: "oldText equals newText (no-op replacement)".into(),
            });
        }
        out.push(Edit {
            old: old.to_string(),
            new: new.to_string(),
        });
    }
    if out.is_empty() {
        return Err(ToolError::InvalidParams {
            reason: "`edits` must not be empty".into(),
        });
    }
    Ok(out)
}

/// 规范化（pi fuzzy 规则）。
#[must_use]
pub fn normalize(s: &str) -> String {
    s.nfkc()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{201C}' | '\u{201D}' => '\'',
            '\u{2013}' | '\u{2014}' | '\u{2212}' => '-',
            '\u{00A0}' | '\u{2007}' | '\u{202F}' => ' ',
            other => other,
        })
        .collect::<String>()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}
