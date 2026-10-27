//! 嵌套工具执行接缝（设计 phase-3.5/11 §4.1；接缝 #8）：钩子/审阅、id、取消、目录。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use serde_json::json;
use timo_agent::{AgentHandle, ToolExecutionMode};
use timo_ai::{Content, Message};
use timo_tools::{Exposure, ToolDefinition, ToolOutput, ToolRegistry};
use tokio_util::sync::CancellationToken;

fn probe_one() -> ToolDefinition {
    probe("probe_one", |ctx| {
        Box::pin(async move {
            let nested = ctx.nested.clone().unwrap();
            let output = nested
                .execute(call(&ctx, "target", json!({"k": "v"})))
                .await
                .output;
            text_of(output)
        })
    })
}

fn result_text(out: &[Message], index: usize) -> (String, bool) {
    match &out[index] {
        Message::ToolResult(result) => {
            let text = match &result.content[0] {
                Content::Text { text } => text.clone(),
                other => panic!("text expected: {other:?}"),
            };
            (text, result.is_error)
        }
        other => panic!("tool result expected: {other:?}"),
    }
}

async fn run_one_tool(agent: AgentHandle) -> (String, bool) {
    let out = agent.run("go".into(), vec![], None).await.unwrap();
    result_text(&out, 2)
}

#[tokio::test]
async fn nested_calls_run_through_hooks_review_and_report_scope() {
    let mut tools = ToolRegistry::new();
    tools.register(probe_one());
    tools.register(target_definition());
    let hooks = Arc::new(TestHooks {
        prefix_target: true,
        ..TestHooks::default()
    });
    let agent = handle(
        scripted(vec![
            tool_call_done("call_1", "probe_one"),
            text_done("done"),
        ]),
        tools,
        Some(hooks.clone()),
        Some(Arc::new(AllowingReview)),
        ToolExecutionMode::Parallel,
    );
    let (text, is_error) = run_one_tool(agent).await;
    assert!(!is_error);
    assert_eq!(
        text, "hook:id=call_1/1 review=yes nested=no k=v",
        "嵌套调用必须带审阅器、id 用父调用前缀、且 after 钩子生效"
    );
    assert_eq!(
        hooks.seen(),
        ["probe_one", "target"],
        "两条调用都过 before 钩子"
    );
}

#[tokio::test]
async fn blocked_nested_call_reports_the_extension_reason() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    let mut tools = ToolRegistry::new();
    tools.register(probe_one());
    tools.register(definition(
        "target",
        Exposure::Direct,
        timo_tools::ExecutionMode::Parallel,
        move |_ctx| {
            let counter = Arc::clone(&counter);
            Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(ToolOutput::text("should not run"))
            })
        },
    ));
    let hooks = Arc::new(TestHooks {
        block: Some("target"),
        ..TestHooks::default()
    });
    let agent = handle(
        scripted(vec![
            tool_call_done("call_1", "probe_one"),
            text_done("done"),
        ]),
        tools,
        Some(hooks),
        None,
        ToolExecutionMode::Parallel,
    );
    let (text, is_error) = run_one_tool(agent).await;
    assert!(is_error, "被阻止的嵌套调用必须标记失败");
    assert_eq!(text, "blocked by extension: nope");
    assert_eq!(hits.load(Ordering::SeqCst), 0, "阻止后目标工具不得执行");
}

#[tokio::test]
async fn nested_ids_are_scoped_to_the_parent_call() {
    let mut tools = ToolRegistry::new();
    tools.register(probe_two("probe_two", "target"));
    tools.register(target_definition());
    let agent = handle(
        scripted(vec![
            tool_call_done("call_7", "probe_two"),
            text_done("done"),
        ]),
        tools,
        None,
        None,
        ToolExecutionMode::Parallel,
    );
    let (text, _) = run_one_tool(agent).await;
    assert!(text.contains("id=call_7/1 "), "{text}");
    assert!(text.contains("id=call_7/2 "), "{text}");
}

#[tokio::test]
async fn canceled_nested_call_ends_without_running_the_tool() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    let mut tools = ToolRegistry::new();
    tools.register(probe("probe_cancel", |ctx| {
        Box::pin(async move {
            let nested = ctx.nested.clone().unwrap();
            let canceled = CancellationToken::new();
            canceled.cancel();
            let mut request = call(&ctx, "target", json!({}));
            request.cancel = canceled;
            let outcome = nested.execute(request).await;
            let summary = nested.snapshot(&ctx.tool_call_id).expect("summary");
            let status = format!("{:?}", summary.calls[0].status);
            ToolOutput::text(format!(
                "{}|{status}",
                timo_tools::output_text(&outcome.output)
            ))
        })
    }));
    tools.register(definition(
        "target",
        Exposure::Direct,
        timo_tools::ExecutionMode::Parallel,
        move |_ctx| {
            let counter = Arc::clone(&counter);
            Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(ToolOutput::text("ran"))
            })
        },
    ));
    let agent = handle(
        scripted(vec![
            tool_call_done("call_1", "probe_cancel"),
            text_done("done"),
        ]),
        tools,
        None,
        None,
        ToolExecutionMode::Parallel,
    );
    let (text, _) = run_one_tool(agent).await;
    assert_eq!(text, "Operation aborted|Cancelled");
    assert_eq!(hits.load(Ordering::SeqCst), 0, "取消后目标工具不得执行");
}

#[tokio::test]
async fn catalog_skips_hidden_tools_and_carries_metadata() {
    let mut tools = ToolRegistry::new();
    tools.register(target_definition());
    tools.register(definition(
        "hidden_tool",
        Exposure::Hidden,
        timo_tools::ExecutionMode::Parallel,
        |_ctx| Box::pin(async move { Ok(ToolOutput::text("hidden")) }),
    ));
    let mut deferred = definition(
        "deferred_tool",
        Exposure::Deferred,
        timo_tools::ExecutionMode::Sequential,
        |_ctx| Box::pin(async move { Ok(ToolOutput::text("deferred")) }),
    );
    deferred.namespace = Some(timo_tools::Namespace {
        name: "mcp__demo".into(),
        description: Some("demo".into()),
    });
    deferred.prompt_guidelines = vec!["be nice".into()];
    tools.register(deferred);
    tools.register(probe("probe_catalog", |ctx| {
        Box::pin(async move {
            let nested = ctx.nested.clone().unwrap();
            let mut lines: Vec<String> = nested
                .catalog()
                .into_iter()
                .map(|tool| {
                    format!(
                        "{}:{:?}:{}:{}",
                        tool.name,
                        tool.exposure,
                        tool.namespace
                            .as_ref()
                            .map(|namespace| namespace.name.as_str())
                            .unwrap_or("-"),
                        tool.prompt_guidelines.len()
                    )
                })
                .collect();
            lines.sort();
            ToolOutput::text(lines.join("\n"))
        })
    }));
    let agent = handle(
        scripted(vec![
            tool_call_done("call_1", "probe_catalog"),
            text_done("done"),
        ]),
        tools,
        None,
        None,
        ToolExecutionMode::Parallel,
    );
    let (text, _) = run_one_tool(agent).await;
    assert!(!text.contains("hidden_tool"), "Hidden 不得进目录:\n{text}");
    assert!(text.contains("target:Direct:-:0"), "{text}");
    assert!(
        text.contains("deferred_tool:Deferred:mcp__demo:1"),
        "{text}"
    );
}
