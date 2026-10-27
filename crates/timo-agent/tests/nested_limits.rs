//! 嵌套调用的顺序、用量合并与记录上限（设计 phase-3.5/11 §4.1/§6）。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::*;
use serde_json::json;
use timo_agent::ToolExecutionMode;
use timo_ai::{Content, Message};
use timo_tools::{ExecutionMode, Exposure, ToolOutput, ToolRegistry};

#[tokio::test]
async fn sequential_tools_serialize_and_parallel_tools_overlap() {
    async fn peak_with(mode: ExecutionMode) -> usize {
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let (a, p) = (Arc::clone(&active), Arc::clone(&peak));
        let mut tools = ToolRegistry::new();
        tools.register(probe("probe_parallel", |ctx| {
            Box::pin(async move {
                let nested = ctx.nested.clone().unwrap();
                let first = nested.execute(call(&ctx, "slow", json!({})));
                let second = nested.execute(call(&ctx, "slow", json!({})));
                let (one, two) = futures::join!(first, second);
                ToolOutput::text(format!("{}{}", one.output.is_error, two.output.is_error))
            })
        }));
        tools.register(definition("slow", Exposure::Direct, mode, move |_ctx| {
            let active = Arc::clone(&a);
            let peak = Arc::clone(&p);
            Box::pin(async move {
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(40)).await;
                active.fetch_sub(1, Ordering::SeqCst);
                Ok(ToolOutput::text("ok"))
            })
        }));
        let agent = handle(
            scripted(vec![
                tool_call_done("call_1", "probe_parallel"),
                text_done("done"),
            ]),
            tools,
            None,
            None,
            ToolExecutionMode::Parallel,
        );
        agent.run("go".into(), vec![], None).await.unwrap();
        peak.load(Ordering::SeqCst)
    }
    assert_eq!(
        peak_with(ExecutionMode::Sequential).await,
        1,
        "串行工具不得并发"
    );
    assert!(
        peak_with(ExecutionMode::Parallel).await >= 2,
        "并行工具应重叠"
    );
}

#[tokio::test]
async fn nested_usage_is_merged_into_the_parent_result() {
    let mut tools = ToolRegistry::new();
    tools.register(probe_two("probe_two", "costly"));
    tools.register(definition(
        "costly",
        Exposure::Direct,
        ExecutionMode::Parallel,
        |_ctx| {
            Box::pin(async move {
                Ok(ToolOutput {
                    usage: Some(timo_ai::Usage {
                        input: 10,
                        output: 2,
                        total_tokens: 12,
                        cost: timo_ai::Cost {
                            total: 0.5,
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ..ToolOutput::text("costly")
                })
            })
        },
    ));
    let agent = handle(
        scripted(vec![
            tool_call_done("call_1", "probe_two"),
            text_done("done"),
        ]),
        tools,
        None,
        None,
        ToolExecutionMode::Parallel,
    );
    let out = agent.run("go".into(), vec![], None).await.unwrap();
    let usage = match &out[2] {
        Message::ToolResult(result) => result.usage.clone().expect("merged usage"),
        other => panic!("tool result expected: {other:?}"),
    };
    assert_eq!(usage.input, 20);
    assert_eq!(usage.output, 4);
    assert_eq!(usage.total_tokens, 24);
    assert!((usage.cost.total - 1.0).abs() < 1e-12, "{usage:?}");
}

fn limit_probes() -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    // 256 条封顶 + 错误文本 500 字符
    tools.register(probe("probe_cap", |ctx| {
        Box::pin(async move {
            let nested = ctx.nested.clone().unwrap();
            for _ in 0..260 {
                nested.execute(call(&ctx, "fails", json!({}))).await;
            }
            let summary = nested.snapshot(&ctx.tool_call_id).expect("summary");
            ToolOutput::text(format!(
                "calls={} complete={} error_len={}",
                summary.calls.len(),
                summary.complete,
                summary.calls[0]
                    .error
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .count()
            ))
        })
    }));
    tools.register(definition(
        "fails",
        Exposure::Direct,
        ExecutionMode::Parallel,
        |_ctx| Box::pin(async move { Ok(ToolOutput::error_text("E".repeat(600))) }),
    ));
    // 单次参数 >8 KiB：参数省略、字节数保留
    tools.register(probe("probe_big", |ctx| {
        Box::pin(async move {
            let nested = ctx.nested.clone().unwrap();
            nested
                .execute(call(&ctx, "echo_len", json!({"pad": "x".repeat(9000)})))
                .await;
            let summary = nested.snapshot(&ctx.tool_call_id).expect("summary");
            let record = &summary.calls[0];
            ToolOutput::text(format!(
                "recorded={} bytes={} complete={}",
                record.arguments.is_some(),
                record.arguments_bytes.unwrap_or(0),
                summary.complete
            ))
        })
    }));
    tools.register(definition(
        "echo_len",
        Exposure::Direct,
        ExecutionMode::Parallel,
        |ctx| {
            Box::pin(async move {
                Ok(ToolOutput::text(
                    ctx.params["pad"].as_str().unwrap_or("").len().to_string(),
                ))
            })
        },
    ));
    // 累计参数 >32 KiB：第 5 条省略
    tools.register(probe("probe_total", |ctx| {
        Box::pin(async move {
            let nested = ctx.nested.clone().unwrap();
            for _ in 0..5 {
                nested
                    .execute(call(&ctx, "echo_len", json!({"pad": "y".repeat(7000)})))
                    .await;
            }
            let summary = nested.snapshot(&ctx.tool_call_id).expect("summary");
            let recorded = summary
                .calls
                .iter()
                .filter(|record| record.arguments.is_some())
                .count();
            ToolOutput::text(format!("recorded={recorded} complete={}", summary.complete))
        })
    }));
    tools
}

#[tokio::test]
async fn nested_record_obeys_every_limit() {
    let tools = limit_probes();
    for (probe_name, expected) in [
        ("probe_cap", "calls=256 complete=false error_len=500"),
        ("probe_big", "recorded=false bytes=9010 complete=false"),
        ("probe_total", "recorded=4 complete=false"),
    ] {
        let agent = handle(
            scripted(vec![
                tool_call_done("call_1", probe_name),
                text_done("done"),
            ]),
            tools.clone(),
            None,
            None,
            ToolExecutionMode::Parallel,
        );
        let out = agent.run("go".into(), vec![], None).await.unwrap();
        let text = match &out[2] {
            Message::ToolResult(result) => match &result.content[0] {
                Content::Text { text } => text.clone(),
                other => panic!("text expected: {other:?}"),
            },
            other => panic!("tool result expected: {other:?}"),
        };
        assert_eq!(text, expected, "{probe_name}");
    }
}
