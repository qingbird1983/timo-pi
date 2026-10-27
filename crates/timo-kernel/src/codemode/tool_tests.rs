//! codemode 注册面、描述渲染与预算（设计 phase-3.5/11 §4.2）。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use serde_json::json;
use timo_tools::{CallableTool, ExecutionMode, Exposure, ToolDefinition, ToolOutput, ToolRegistry};

use super::{catalog, describe, limits, tool};

fn callable(name: &str, exposure: Exposure) -> CallableTool {
    CallableTool {
        name: name.into(),
        description: format!("{name} tool"),
        parameters: json!({"type": "object", "properties": {}}),
        output_schema: None,
        namespace: None,
        prompt_guidelines: vec![],
        execution_mode: ExecutionMode::Parallel,
        exposure,
    }
}

#[test]
fn catalog_drops_hidden_and_definition_drops_itself() {
    let mut registry = ToolRegistry::new();
    for (name, exposure) in [
        ("read", Exposure::Direct),
        ("deferred", Exposure::Deferred),
        ("hidden", Exposure::Hidden),
        ("codemode", Exposure::ModelOnly),
    ] {
        registry.register(ToolDefinition {
            name: name.into(),
            label: name.into(),
            description: name.into(),
            prompt_snippet: None,
            prompt_guidelines: vec![],
            parameters: json!({"type": "object", "properties": {}}),
            output_schema: None,
            exposure,
            namespace: None,
            annotations: timo_tools::Annotations::default(),
            execution_mode: ExecutionMode::Parallel,
            execute: Arc::new(|_| Box::pin(async { Ok(ToolOutput::text("x")) })),
        });
    }
    let mut names: Vec<String> = catalog(&registry)
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    names.sort();
    assert_eq!(names, ["codemode", "deferred", "read"]);

    let definition = tool::definition(catalog(&registry), limits());
    assert_eq!(definition.name, "codemode");
    assert_eq!(definition.exposure, Exposure::ModelOnly);
    assert!(definition.description.contains("### `read`"));
    assert!(!definition.description.contains("### `codemode`"));
    assert!(!definition.description.contains("### `hidden`"));
    assert!(!definition.description.contains("### `deferred`"));
}

#[test]
fn description_budget_keeps_costly_groups_partially_listed() {
    let tools: Vec<CallableTool> = (0..40)
        .map(|index| {
            let mut tool = callable(&format!("tool{index:02}"), Exposure::Direct);
            tool.description = "d".repeat(200);
            tool
        })
        .collect();
    let rendered = describe::render(&tools, 400);
    assert!(rendered.contains("### `tool"), "{rendered}");
    assert!(
        rendered.contains("Some tools are callable but not listed here"),
        "{rendered}"
    );
    let shown = rendered.matches("### `tool").count();
    assert!(shown > 0 && shown < 40, "{shown}");
}
