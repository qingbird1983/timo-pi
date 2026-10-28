//! rmcp 结果 → 统一 Content / ToolDefinition（设计 06 §1.3；pi convertMcpResult 同构）。

use rmcp::model::{CallToolResult, ContentBlock, Tool};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::Result;

/// 合并文本截断阈值（20KB，pi MCP_OUTPUT_MAX_BYTES 同值）。
const MCP_OUTPUT_MAX_BYTES: usize = 20 * 1024;

/// 已绑定 server+tool 的调用闭包（runtime 构造；命名冲突已在此前解决）。
pub(crate) type BoundCall = Arc<
    dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<CallToolResult>> + Send>> + Send + Sync,
>;

/// ContentBlock → 统一 Content（text/image 透传；其余给描述性文本）。
#[must_use]
pub fn block_to_content(block: &ContentBlock) -> timo_ai::Content {
    match block {
        ContentBlock::Text(t) => timo_ai::Content::Text {
            text: t.text.clone(),
        },
        ContentBlock::Image(i) => timo_ai::Content::Image {
            data: i.data.clone(),
            mime_type: i.mime_type.clone(),
        },
        ContentBlock::Audio(a) => timo_ai::Content::Text {
            text: format!("[audio block: {} ({} bytes)]", a.mime_type, a.data.len()),
        },
        ContentBlock::ResourceLink(r) => timo_ai::Content::Text {
            text: format!(
                "[resource link: {} ({}) — read_mcp_resource lands in Phase 2]",
                r.name, r.uri
            ),
        },
        ContentBlock::Resource(r) => timo_ai::Content::Text {
            text: format!("[embedded resource: {}]", resource_uri(&r.resource)),
        },
        _ => timo_ai::Content::Text {
            text: "[unknown content block]".into(),
        },
    }
}

fn resource_uri(rc: &rmcp::model::ResourceContents) -> String {
    match rc {
        rmcp::model::ResourceContents::TextResourceContents { uri, .. }
        | rmcp::model::ResourceContents::BlobResourceContents { uri, .. } => uri.clone(),
        _ => "<unknown>".into(),
    }
}

/// CallToolResult → ToolOutput（合并文本 > 20KB → 中截断 + 全文落临时文件，pi 同构）。
pub(crate) fn result_to_output(result: &CallToolResult) -> timo_tools::ToolOutput {
    let contents: Vec<timo_ai::Content> = result.content.iter().map(block_to_content).collect();
    let total: usize = contents
        .iter()
        .map(|c| match c {
            timo_ai::Content::Text { text } => text.len(),
            timo_ai::Content::Image { data, .. } => data.len(),
            _ => 0,
        })
        .sum();
    let is_error = result.is_error.unwrap_or(false);
    if total <= MCP_OUTPUT_MAX_BYTES {
        return timo_tools::ToolOutput {
            content: contents,
            details: Value::Null,
            structured_content: result.structured_content.clone(),
            is_error,
            terminate: false,
            usage: None,
        };
    }
    let full_text: String = contents
        .iter()
        .map(|c| match c {
            timo_ai::Content::Text { text } => text.clone(),
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let full_path = save_full(&full_text);
    let (body, truncated) = timo_tools::truncate_middle(&full_text, MCP_OUTPUT_MAX_BYTES);
    let body = if truncated {
        body
    } else {
        full_text.chars().take(MCP_OUTPUT_MAX_BYTES).collect()
    };
    let mut out = format!(
        "Warning: truncated output (original size: ~{} bytes)\n{body}",
        full_text.len()
    );
    if let Some(p) = full_path {
        out.push_str(&format!("\n[Full output: {}]", p.display()));
    }
    timo_tools::ToolOutput {
        content: vec![timo_ai::Content::Text { text: out }],
        details: Value::Null,
        structured_content: result.structured_content.clone(),
        is_error,
        terminate: false,
        usage: None,
    }
}

fn save_full(text: &str) -> Option<std::path::PathBuf> {
    let mut f = tempfile::Builder::new()
        .prefix("timo-mcp")
        .rand_bytes(6)
        .tempfile()
        .ok()?;
    std::io::Write::write_all(&mut f, text.as_bytes()).ok()?;
    f.into_temp_path().keep().ok()
}

/// rmcp Tool → 统一 ToolDefinition（schema/annotations 透传；namespace = mcp__<server>）。
#[must_use]
pub fn rmcp_tool_to_definition(
    server: &str,
    tool: &Tool,
    exposure: timo_tools::Exposure,
    call: BoundCall,
) -> timo_tools::ToolDefinition {
    let name = crate::naming::mcp_tool_name(server, &tool.name);
    let desc = tool
        .description
        .as_ref()
        .map(|d| d.to_string())
        .unwrap_or_default();
    let annotations = tool
        .annotations
        .as_ref()
        .map(|a| timo_tools::Annotations {
            read_only_hint: a.read_only_hint,
            destructive_hint: a.destructive_hint,
            idempotent_hint: a.idempotent_hint,
            open_world_hint: a.open_world_hint,
        })
        .unwrap_or_default();
    let parameters = Value::Object((*tool.input_schema).clone());
    timo_tools::ToolDefinition {
        name,
        label: format!("{server}: {}", tool.name),
        description: desc,
        prompt_snippet: None,
        prompt_guidelines: vec![],
        parameters,
        output_schema: None,
        exposure,
        namespace: Some(timo_tools::Namespace {
            name: format!("mcp__{}", server.replace('-', "_")),
            description: Some(format!("MCP server `{server}`")),
        }),
        annotations,
        execution_mode: timo_tools::ExecutionMode::Parallel,
        execute: Arc::new(move |ctx: timo_tools::ToolCallCtx| {
            let call = call.clone();
            Box::pin(async move {
                let cancel = ctx.cancel.clone();
                let args = match ctx.params {
                    Value::Object(m) => m,
                    other => {
                        return Err(timo_tools::ToolError::Mcp(format!(
                            "params must be an object: {other}"
                        )));
                    }
                };
                // 接缝 #7：本侧等待受取消约束；底层 rmcp 请求的收尾由客户端负责。
                let result = tokio::select! {
                    () = cancel.cancelled() => return Err(timo_tools::ToolError::Cancelled),
                    result = call(Value::Object(args)) => {
                        result.map_err(|e| timo_tools::ToolError::Mcp(e.to_string()))?
                    }
                };
                Ok(result_to_output(&result))
            })
        }),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn text_passthrough_and_error_flag() {
        let mut r = CallToolResult::success(vec![ContentBlock::text("hello")]);
        r.is_error = Some(true);
        let out = result_to_output(&r);
        assert_eq!(
            out.content[0],
            timo_ai::Content::Text {
                text: "hello".into()
            }
        );
        assert!(out.is_error);
    }

    #[test]
    fn oversized_result_truncated_with_tempfile_note() {
        let r = CallTool::big();
        let out = result_to_output(&r);
        let timo_ai::Content::Text { text } = &out.content[0] else {
            panic!("text")
        };
        assert!(text.contains("Warning: truncated output"));
        assert!(text.contains("chars truncated") || text.contains("[Full output:"));
    }

    /// 构造超限结果（>20KB）。
    struct CallTool;
    impl CallTool {
        fn big() -> CallToolResult {
            CallToolResult::success(vec![ContentBlock::text("x".repeat(64 * 1024))])
        }
    }

    #[tokio::test]
    async fn cancelled_call_returns_without_waiting_for_rmcp() {
        let tool = Tool::new("slow", "slow tool", serde_json::Map::new());
        let call: BoundCall = Arc::new(|_args| {
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                Ok(CallToolResult::success(vec![ContentBlock::text("late")]))
            })
        });
        let def = rmcp_tool_to_definition("srv", &tool, timo_tools::Exposure::Direct, call);
        let token = tokio_util::sync::CancellationToken::new();
        token.cancel();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let ctx = timo_tools::ToolCallCtx {
            tool_call_id: "t1".into(),
            params: serde_json::json!({}),
            cancel: token,
            updates: tx,
            cwd: Arc::from(std::path::Path::new(".")),
            review: None,
            nested: None,
        };
        let err = def.run(ctx).await.unwrap_err();
        assert!(matches!(err, timo_tools::ToolError::Cancelled), "{err}");
    }
}
