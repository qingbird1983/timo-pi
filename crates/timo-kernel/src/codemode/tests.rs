//! codemode 工具面与沙箱行为（设计 phase-3.5/11 §6）。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use timo_tools::{
    CallableTool, ExecutionMode, Exposure, NestedCall, NestedCallRecord, NestedCallStatus,
    NestedCallSummary, NestedOutcome, NestedToolHost, ToolCallCtx, ToolOutput,
};
use tokio_util::sync::CancellationToken;

use super::{Limits, sandbox};

struct FakeHost {
    tools: Vec<CallableTool>,
    records: Mutex<Vec<NestedCallRecord>>,
    next: AtomicU64,
    hold: Option<Arc<tokio::sync::Notify>>,
}

impl FakeHost {
    fn new(tools: Vec<CallableTool>) -> Self {
        Self {
            tools,
            records: Mutex::new(Vec::new()),
            next: AtomicU64::new(1),
            hold: None,
        }
    }
}

#[async_trait::async_trait]
impl NestedToolHost for FakeHost {
    fn catalog(&self) -> Vec<CallableTool> {
        self.tools.clone()
    }

    fn is_sequential(&self) -> bool {
        false
    }

    fn execute(
        &self,
        call: NestedCall,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = NestedOutcome> + Send + '_>> {
        Box::pin(async move {
            if call.name == "hold"
                && let Some(hold) = &self.hold
            {
                hold.notified().await;
            }
            let output = match call.name.as_str() {
                "echo" => ToolOutput::text(
                    call.arguments
                        .get("text")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("echo"),
                ),
                "obj" => ToolOutput {
                    structured_content: Some(json!({"a": 1})),
                    ..ToolOutput::text("ignored")
                },
                "fail" => ToolOutput::error_text("boom tool"),
                "hold" => ToolOutput::text("held"),
                other => ToolOutput::error_text(format!("no such tool {other}")),
            };
            let number = self.next.fetch_add(1, Ordering::SeqCst);
            self.records.lock().unwrap().push(NestedCallRecord {
                id: format!("{}/{number}", call.parent_id),
                name: call.name.clone(),
                arguments: None,
                arguments_bytes: None,
                status: if output.is_error {
                    NestedCallStatus::Error
                } else {
                    NestedCallStatus::Ok
                },
                duration_ms: Some(1),
                error: output.is_error.then(|| timo_tools::output_text(&output)),
            });
            NestedOutcome {
                output,
                duration_ms: 1,
            }
        })
    }

    fn snapshot(&self, _parent_id: &str) -> Option<NestedCallSummary> {
        Some(NestedCallSummary {
            calls: self.records.lock().unwrap().clone(),
            complete: true,
            usage: None,
        })
    }

    fn take(&self, parent_id: &str) -> Option<NestedCallSummary> {
        self.snapshot(parent_id)
    }
}

fn callable(name: &str, output_schema: Option<serde_json::Value>) -> CallableTool {
    CallableTool {
        name: name.into(),
        description: format!("{name} tool"),
        parameters: json!({"type": "object", "properties": {}}),
        output_schema,
        namespace: None,
        prompt_guidelines: vec![],
        execution_mode: ExecutionMode::Parallel,
        exposure: Exposure::Direct,
    }
}

fn host() -> Arc<FakeHost> {
    let mut lazy = callable("lazy", None);
    lazy.exposure = Exposure::Deferred;
    Arc::new(FakeHost::new(vec![
        callable("echo", None),
        callable("fail", None),
        callable("hold", None),
        callable("obj", Some(json!({"type": "object"}))),
        callable("mcp__docs.search", None),
        lazy,
    ]))
}

fn ctx(
    nested: Arc<dyn NestedToolHost>,
    code: &str,
    cancel: CancellationToken,
) -> (ToolCallCtx, tokio::sync::mpsc::Receiver<ToolOutput>) {
    let (updates, rx) = tokio::sync::mpsc::channel(8);
    (
        ToolCallCtx {
            tool_call_id: "call_1".into(),
            params: json!({ "code": code }),
            cancel,
            updates,
            cwd: Arc::from(Path::new(".")),
            review: None,
            nested: Some(nested),
        },
        rx,
    )
}

/// 测试上界：任何一次脚本调用都必须在此时限内返回（设计 T12 的外层兜底）。
const BOUND: Duration = Duration::from_secs(3);

async fn run(host: &Arc<FakeHost>, code: &str, limits: Limits) -> ToolOutput {
    let (ctx, rx) = ctx(
        Arc::clone(host) as Arc<dyn NestedToolHost>,
        code,
        CancellationToken::new(),
    );
    let catalog = host.catalog();
    let output = tokio::time::timeout(
        BOUND,
        sandbox::execute(ctx, Arc::new(catalog), Arc::new(limits)),
    )
    .await
    .expect("script call must return within the test bound")
    .unwrap();
    drop(rx);
    output
}

fn text(output: &ToolOutput) -> String {
    timo_tools::output_text(output)
}

#[tokio::test]
async fn text_console_return_and_exit_shape_the_output() {
    let host = host();
    let output = run(
        &host,
        "text(\"a\"); console.log(\"c\"); console.warn(\"w\"); exit(); text(\"unreachable\");",
        Limits::default(),
    )
    .await;
    let body = text(&output);
    assert!(!output.is_error, "{body}");
    assert!(
        body.contains("Output:\na\n<console_output>\nc\nw\n</console_output>"),
        "{body}"
    );
    assert!(
        body.contains("<console_output>\nc\nw\n</console_output>"),
        "{body}"
    );
    assert!(!body.contains("unreachable"), "{body}");

    let returned = run(&host, "return {x: 2};", Limits::default()).await;
    assert!(text(&returned).contains("{\"x\":2}"), "{}", text(&returned));
}

#[tokio::test]
async fn script_calls_tools_and_maps_their_results() {
    let host = host();
    let output = run(
        &host,
        r#"
const one = await tools.echo({text: "hi"});
const two = await tools.obj({});
let caught = "";
try { await tools.fail({}); } catch (error) { caught = error.message; }
let unknown = "";
try { await tools.nope({}); } catch (error) { unknown = error.message; }
text(one + "|" + two.a + "|" + caught + "|" + unknown);
"#,
        Limits::default(),
    )
    .await;
    let body = text(&output);
    assert!(!output.is_error, "{body}");
    assert!(
        body.contains("hi|1|boom tool|Unknown tool \"nope\""),
        "{body}"
    );
}

#[tokio::test]
async fn sanitized_names_reach_the_real_tool() {
    let host = host();
    let output = run(
        &host,
        r#"text(await tools.mcp__docs_search({}))"#,
        Limits::default(),
    )
    .await;
    assert!(
        text(&output).contains("no such tool mcp__docs.search"),
        "{}",
        text(&output)
    );
    let records = host.records.lock().unwrap();
    assert_eq!(records[0].name, "mcp__docs.search");
}

#[tokio::test]
async fn failure_keeps_partial_output_and_lists_completed_calls() {
    let host = host();
    let output = run(
        &host,
        r#"text("before"); await tools.echo({text: "x"}); throw new Error("boom");"#,
        Limits::default(),
    )
    .await;
    let body = text(&output);
    assert!(output.is_error, "{body}");
    assert!(body.contains("Script failed"), "{body}");
    assert!(body.contains("before"), "{body}");
    assert!(body.contains("Script error:\nError: boom"), "{body}");
    assert!(
        body.contains("Tool calls made before the failure (they are not undone): echo (ok)"),
        "{body}"
    );
}

#[tokio::test]
async fn output_limits_abort_the_script() {
    let host = host();
    let small = Limits {
        max_output_chars: 10,
        ..Limits::default()
    };
    let output = run(&host, r#"text("x".repeat(20))"#, small).await;
    assert!(output.is_error, "{}", text(&output));
    assert!(
        text(&output).contains("Script output limit exceeded (10 characters)"),
        "{}",
        text(&output)
    );

    let few = Limits {
        max_output_calls: 1,
        ..Limits::default()
    };
    let output = run(&host, "text(1); text(2);", few).await;
    assert!(
        text(&output).contains("Script output limit exceeded (1 output calls)"),
        "{}",
        text(&output)
    );
}

#[tokio::test]
async fn timeout_and_cancel_interrupt_a_busy_script() {
    let host = host();
    let output = run(
        &host,
        "// @options: {\"timeout_ms\": 100}\nwhile (true) {}",
        Limits::default(),
    )
    .await;
    assert!(output.is_error, "{}", text(&output));
    assert!(
        text(&output).contains("Script timed out: execution timed out after 100 ms"),
        "{}",
        text(&output)
    );

    let cancel = CancellationToken::new();
    let (ctx, rx) = ctx(
        Arc::clone(&host) as Arc<dyn NestedToolHost>,
        "while (true) {}",
        cancel.clone(),
    );
    let catalog = host.catalog();
    let task = tokio::spawn(sandbox::execute(
        ctx,
        Arc::new(catalog),
        Arc::new(Limits::default()),
    ));
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.cancel();
    let output = tokio::time::timeout(BOUND, task)
        .await
        .expect("cancel must be bounded")
        .unwrap()
        .unwrap();
    drop(rx);
    assert!(output.is_error, "{}", text(&output));
    assert!(
        text(&output).contains("Script aborted: execution aborted"),
        "{}",
        text(&output)
    );
}

#[tokio::test]
async fn memory_limit_surfaces_as_a_script_error() {
    let host = host();
    let tiny = Limits {
        memory_bytes: 4 * 1024 * 1024,
        ..Limits::default()
    };
    let output = run(
        &host,
        "const a = []; while (true) { a.push(\"x\".repeat(4096)); }",
        tiny,
    )
    .await;
    assert!(output.is_error, "{}", text(&output));
    assert!(
        text(&output).to_lowercase().contains("memory"),
        "{}",
        text(&output)
    );
}

#[tokio::test]
async fn a_busy_worker_pool_fails_fast_instead_of_queueing_forever() {
    // 占满进程级 worker（默认 2 个），让新脚本在 worker 等待上超时。
    let permits = sandbox::workers(2).acquire_many_owned(2).await.unwrap();
    let host = host();
    let output = run(
        &host,
        "// @options: {\"timeout_ms\": 100}\ntext(1)",
        Limits::default(),
    )
    .await;
    drop(permits);
    assert!(output.is_error, "{}", text(&output));
    assert!(
        text(&output).contains("while waiting for a free codemode worker"),
        "{}",
        text(&output)
    );
}

#[tokio::test]
async fn all_tools_and_describe_tool_expose_the_declared_set() {
    let host = host();
    let output = run(
        &host,
        r#"text(ALL_TOOLS.map((tool) => tool.name).join(",") + "|" + describeTool("echo"));"#,
        Limits::default(),
    )
    .await;
    let body = text(&output);
    assert!(
        body.contains("echo,fail,hold,obj,mcp__docs.search,lazy|echo() -> string: echo tool"),
        "{body}"
    );
}

#[tokio::test]
async fn nested_progress_carries_call_details() {
    let host = host();
    let (call_ctx, mut rx) = ctx(
        Arc::clone(&host) as Arc<dyn NestedToolHost>,
        "await tools.echo({});",
        CancellationToken::new(),
    );
    let catalog = host.catalog();
    let output = sandbox::execute(call_ctx, Arc::new(catalog), Arc::new(Limits::default()))
        .await
        .unwrap();
    assert!(!output.is_error, "{}", text(&output));
    let mut lines = Vec::new();
    let mut last_calls = 0;
    while let Ok(update) = rx.try_recv() {
        lines.push(timo_tools::output_text(&update));
        last_calls = update
            .details
            .get("calls")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len);
    }
    assert_eq!(lines, ["→ tools.echo", "← echo ok"]);
    assert_eq!(last_calls, 1);
}

#[tokio::test]
async fn output_budget_truncates_and_records_the_full_output_path() {
    let host = host();
    let output = run(
        &host,
        "// @options: {\"max_output_tokens\": 5}\ntext(\"x\".repeat(100));",
        Limits::default(),
    )
    .await;
    let body = text(&output);
    assert!(!output.is_error, "{body}");
    assert!(body.contains("Warning: truncated output"), "{body}");
    let path = output.details["full_output_path"]
        .as_str()
        .expect("spilled path");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "x".repeat(100));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn unserializable_arguments_reject_before_reaching_the_host() {
    let host = host();
    let output = run(
        &host,
        r#"let message = ""; const cyclic = {}; cyclic.self = cyclic;
try { await tools.echo(cyclic); } catch (error) { message = error.message; }
text(message);"#,
        Limits::default(),
    )
    .await;
    let body = text(&output);
    let missing = "arguments are not JSON-serializable";
    assert!(body.contains(missing), "{body}");
    assert!(host.records.lock().unwrap().is_empty());
}
