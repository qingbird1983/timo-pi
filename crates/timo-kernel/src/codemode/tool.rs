//! `codemode` 工具定义（设计 phase-3.5/11 §4.2）。

use std::sync::Arc;

use serde_json::json;
use timo_tools::{Annotations, CallableTool, ExecutionMode, Exposure, ToolDefinition};

use super::{Limits, describe, sandbox};

/// 构造 `codemode` 工具；暴露面由内核按激活状态覆盖。
pub(super) fn definition(catalog: Vec<CallableTool>, limits: Arc<Limits>) -> ToolDefinition {
    let callable: Vec<CallableTool> = catalog
        .into_iter()
        .filter(|tool| tool.name != "codemode")
        .collect();
    let description = describe::render(&callable, describe::DEFAULT_INLINE_BUDGET);
    let callable = Arc::new(callable);
    ToolDefinition {
        name: "codemode".into(),
        label: "Codemode".into(),
        description,
        prompt_snippet: Some("codemode(code): run JavaScript that calls other tools".into()),
        prompt_guidelines: vec![
            "Use codemode to batch independent tool calls, chain them, or filter large output instead of many separate calls.".into(),
        ],
        parameters: json!({
            "type": "object",
            "properties": {"code": {"type": "string", "description": "Raw JavaScript source."}},
            "required": ["code"]
        }),
        output_schema: None,
        exposure: Exposure::ModelOnly,
        namespace: None,
        annotations: Annotations {
            open_world_hint: Some(true),
            ..Annotations::default()
        },
        execution_mode: ExecutionMode::Parallel,
        execute: Arc::new(move |ctx| {
            let callable = Arc::clone(&callable);
            let limits = Arc::clone(&limits);
            Box::pin(sandbox::execute(ctx, callable, limits))
        }),
    }
}
