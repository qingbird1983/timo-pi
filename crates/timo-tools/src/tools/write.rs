//! `write` 工具（设计 03 §3：父目录递归创建、覆盖写、与 edit 共享互斥队列）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::Result;
use crate::definition::{ToolCallCtx, ToolDefinition, ToolOutput};
use crate::error::ToolError;
use crate::mutate::with_file_mutation_queue;
use crate::review::{ReviewKind, ToolReview, ToolReviewRequest, ask, display_path};
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
    with_file_mutation_queue(&path, || {
        let path = path.clone();
        let content = content.clone();
        let review = ctx.review.clone();
        let cancel = ctx.cancel.clone();
        let tool_call_id = ctx.tool_call_id.clone();
        let cwd = cwd.clone();
        async move {
            match review {
                Some(review) => {
                    write_with_review(&path, &content, review, &tool_call_id, &cwd, &cancel).await
                }
                // CLI/Phase 1/2：无审阅器时走原路径，不做额外读取。
                None => write_file(&path, &content).await,
            }
        }
    })
    .await
}

/// 读旧内容 → 审阅 → 允许后才创建父目录并写入（拒绝/取消无副作用）。
async fn write_with_review(
    path: &Path,
    content: &str,
    review: Arc<dyn ToolReview>,
    tool_call_id: &str,
    cwd: &Path,
    cancel: &CancellationToken,
) -> Result<ToolOutput> {
    let raw = match tokio::fs::read(path).await {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(io_err(path.to_path_buf(), e)),
    };
    let (before, binary) = match raw.as_deref() {
        None => (None, false),
        Some(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => (Some(text.to_string()), false),
            Err(_) => (None, true),
        },
    };
    ask(
        Some(&review),
        cancel,
        ToolReviewRequest {
            tool_call_id: tool_call_id.to_string(),
            tool_name: "write".into(),
            kind: ReviewKind::FileChange {
                path: path.to_path_buf(),
                display_path: display_path(cwd, path),
                before,
                after: content.to_string(),
                binary,
            },
        },
    )
    .await?;
    write_file(path, content).await
}

async fn write_file(path: &Path, content: &str) -> Result<ToolOutput> {
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
    use crate::review::test_support::RecordingReview;
    use crate::review::{ReviewDecision, ReviewKind, ToolReview, ToolReviewRequest};
    use std::path::Path;

    fn ctx(
        params: Value,
        cwd: Arc<Path>,
        review: Option<Arc<dyn ToolReview>>,
        cancel: CancellationToken,
    ) -> ToolCallCtx {
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        ToolCallCtx {
            tool_call_id: "call-1".into(),
            params,
            cancel,
            updates: tx,
            cwd,
            review,
        }
    }

    fn test_dir(name: &str) -> Arc<Path> {
        let dir = std::env::temp_dir().join(format!("timo-write-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Arc::from(dir.as_path())
    }

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
        let ctx = ctx(
            json!({"path": "sub/out.txt", "content": "hi"}),
            cwd,
            None,
            CancellationToken::new(),
        );
        let out = def.run(ctx).await.unwrap();
        let timo_ai::Content::Text { text } = &out.content[0] else {
            panic!("text")
        };
        assert!(text.contains("Successfully wrote to"));
        assert!(Path::new(&dir).join("sub/out.txt").exists());
    }

    #[tokio::test]
    async fn reject_leaves_no_file_and_reports_reason() {
        let cwd = test_dir("reject");
        let review = RecordingReview::new(ReviewDecision::Reject {
            reason: "keep it".into(),
        });
        let def = definition(cwd.clone());
        let err = def
            .run(ctx(
                json!({"path": "sub/out.txt", "content": "hi"}),
                cwd.clone(),
                Some(review.clone()),
                CancellationToken::new(),
            ))
            .await
            .expect_err("rejected");
        assert!(err.to_string().contains("Rejected by user: keep it"));
        assert!(!cwd.join("sub/out.txt").exists());
        assert_eq!(review.calls(), 1);
        match review.last().kind {
            ReviewKind::FileChange {
                display_path,
                before,
                after,
                binary,
                ..
            } => {
                assert_eq!(display_path, "sub/out.txt");
                assert_eq!(before, None);
                assert_eq!(after, "hi");
                assert!(!binary);
            }
            other => panic!("file change expected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn accept_writes_and_reports_the_previous_content() {
        let cwd = test_dir("accept");
        std::fs::write(cwd.join("out.txt"), "old").unwrap();
        let review = RecordingReview::new(ReviewDecision::Allow);
        let def = definition(cwd.clone());
        def.run(ctx(
            json!({"path": "out.txt", "content": "new"}),
            cwd.clone(),
            Some(review.clone()),
            CancellationToken::new(),
        ))
        .await
        .expect("allowed");
        assert_eq!(std::fs::read_to_string(cwd.join("out.txt")).unwrap(), "new");
        match review.last().kind {
            ReviewKind::FileChange { before, after, .. } => {
                assert_eq!(before.as_deref(), Some("old"));
                assert_eq!(after, "new");
            }
            other => panic!("file change expected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn binary_before_is_flagged_and_not_shown_as_text() {
        let cwd = test_dir("binary");
        std::fs::write(cwd.join("bin.dat"), [0xFF_u8, 0x00, 0xFE]).unwrap();
        let review = RecordingReview::new(ReviewDecision::Reject {
            reason: "stop".into(),
        });
        let def = definition(cwd.clone());
        let _ = def
            .run(ctx(
                json!({"path": "bin.dat", "content": "text"}),
                cwd.clone(),
                Some(review.clone()),
                CancellationToken::new(),
            ))
            .await;
        match review.last().kind {
            ReviewKind::FileChange { before, binary, .. } => {
                assert!(binary);
                assert_eq!(before, None);
            }
            other => panic!("file change expected: {other:?}"),
        }
        // 拒绝后原字节不变
        assert_eq!(
            std::fs::read(cwd.join("bin.dat")).unwrap(),
            [0xFF_u8, 0x00, 0xFE]
        );
    }

    #[tokio::test]
    async fn cancelling_while_waiting_leaves_no_file() {
        struct HangingReview;
        #[async_trait::async_trait]
        impl ToolReview for HangingReview {
            async fn review(&self, _: ToolReviewRequest) -> ReviewDecision {
                std::future::pending().await
            }
        }

        let cwd = test_dir("cancel");
        let cancel = CancellationToken::new();
        let def = definition(cwd.clone());
        let task = tokio::spawn({
            let cwd = cwd.clone();
            let cancel = cancel.clone();
            async move {
                def.run(ctx(
                    json!({"path": "out.txt", "content": "x"}),
                    cwd,
                    Some(Arc::new(HangingReview)),
                    cancel,
                ))
                .await
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        cancel.cancel();
        let err = task.await.unwrap().expect_err("cancelled");
        assert!(err.to_string().contains("run cancelled"));
        assert!(!cwd.join("out.txt").exists());
    }

    #[tokio::test]
    async fn panicking_reviewer_does_not_write() {
        struct PanicReview;
        #[async_trait::async_trait]
        impl ToolReview for PanicReview {
            async fn review(&self, _: ToolReviewRequest) -> ReviewDecision {
                panic!("boom")
            }
        }

        let cwd = test_dir("panic");
        let def = definition(cwd.clone());
        let err = def
            .run(ctx(
                json!({"path": "out.txt", "content": "x"}),
                cwd.clone(),
                Some(Arc::new(PanicReview)),
                CancellationToken::new(),
            ))
            .await
            .expect_err("panic captured");
        assert!(err.to_string().contains("review failed"));
        assert!(!cwd.join("out.txt").exists());
    }
}
