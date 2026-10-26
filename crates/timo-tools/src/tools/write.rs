//! `write` 工具（设计 03 §3：父目录递归创建、覆盖写、与 edit 共享互斥队列）。

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::Result;
use crate::definition::{ToolCallCtx, ToolDefinition, ToolOutput};
use crate::error::ToolError;
use crate::mutate::with_file_mutation_queue;
use crate::tools::read::resolve;

/// 参数 schema。
#[must_use]
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string"},
            "content": {"type": "string"}
        },
        "required": ["path", "content"]
    })
}

/// 构造 write 工具定义。
#[must_use]
pub fn definition(cwd: Arc<std::path::Path>) -> ToolDefinition {
    ToolDefinition {
        name: "write".into(),
        label: "Write".into(),
        description: "Write content to a file, creating parent directories as needed. \
                      Overwrites the file if it exists."
            .into(),
        prompt_snippet: Some("write(path, content): create or overwrite a file".into()),
        prompt_guidelines: vec![
            "Prefer `edit` for modifying existing files; use `write` for new files or full rewrites.".into(),
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
    let content = str_param(&ctx.params, "content")?.to_string();
    with_file_mutation_queue(&path, || async { write_file(&path, &content).await }).await
}

async fn write_file(path: &std::path::Path, content: &str) -> Result<ToolOutput> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| ToolError::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
    }
    tokio::fs::write(path, content)
        .await
        .map_err(|e| ToolError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
    Ok(ToolOutput::text(format!(
        "Successfully wrote to {}",
        path.display()
    )))
}

/// 参数取值捷径（read/write/edit 共用）。
pub(crate) fn str_param<'a>(params: &'a Value, key: &str) -> Result<&'a str> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams {
            reason: format!("missing string param `{key}`"),
        })
}

/// 错误辅助：带路径的 IO 错误。
pub(crate) fn io_err(path: PathBuf, e: std::io::Error) -> ToolError {
    ToolError::Io { path, source: e }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::definition::ToolCallCtx;
    use std::path::Path;

    #[tokio::test]
    async fn creates_parents_and_overwrites() {
        let dir = std::env::temp_dir().join(format!("timo-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let target = dir.join("a/b/c.txt");
        write_file(&target, "v1").await.unwrap();
        write_file(&target, "v2").await.unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v2");
    }

    #[tokio::test]
    async fn full_ctx_run() {
        let dir = std::env::temp_dir().join(format!("timo-write-ctx-{}", std::process::id()));
        let cwd: Arc<std::path::Path> = Arc::from(PathBuf::from(&dir).as_path());
        let def = definition(cwd.clone());
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let ctx = ToolCallCtx {
            tool_call_id: "t".into(),
            params: json!({"path": "sub/out.txt", "content": "hi"}),
            cancel: tokio_util::sync::CancellationToken::new(),
            updates: tx,
            cwd,
        };
        let out = def.run(ctx).await.unwrap();
        let timo_ai::Content::Text { text } = &out.content[0] else {
            panic!("text")
        };
        assert!(text.contains("Successfully wrote to"));
        assert!(Path::new(&dir).join("sub/out.txt").exists());
    }
}
