//! 工具执行前的显式审阅接缝（ADR-0012；设计 04）。
//!
//! 只回答“允许/拒绝”，不参与参数校验、权限与缓存；`None` 审阅器保持
//! Phase 1/2 的自动执行语义。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::FutureExt as _;
use tokio_util::sync::CancellationToken;

use crate::error::ToolError;

/// 一次审阅请求（工具已算出最终副作用，尚未落地）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolReviewRequest {
    /// 工具调用 id（GUI 用它对回结果）。
    pub tool_call_id: String,
    /// 工具名（`write` / `edit` / `bash` / `powershell`）。
    pub tool_name: String,
    /// 待确认的副作用。
    pub kind: ReviewKind,
}

/// 待确认的副作用种类。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewKind {
    /// 文件将被写入/覆盖。
    FileChange {
        /// 绝对路径。
        path: PathBuf,
        /// 相对工作目录的展示路径（不可相对时为绝对路径）。
        display_path: String,
        /// 写入前内容；`None` = 新文件或二进制不可读。
        before: Option<String>,
        /// 写入后内容。
        after: String,
        /// 原文件不是 UTF-8：不做文本 diff。
        binary: bool,
    },
    /// 命令即将 spawn。
    Command {
        /// 原样命令文本（界面可折叠，不截断语义）。
        command: String,
        /// 执行目录。
        cwd: PathBuf,
        /// 实际 shell（`bash` / `powershell`）。
        shell: String,
    },
}

/// 审阅决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewDecision {
    /// 执行。
    Allow,
    /// 不执行；原因进入工具结果（模型可调整）。
    Reject {
        /// 拒绝原因。
        reason: String,
    },
}

/// 审阅器（GUI 实现；CLI 不注入）。
#[async_trait::async_trait]
pub trait ToolReview: Send + Sync {
    /// 等待决定；实现方不得在持锁状态下跨 await。
    async fn review(&self, request: ToolReviewRequest) -> ReviewDecision;
}

/// 询问审阅器：`None` 放行；等待中取消或审阅器 panic 都不执行副作用。
pub(crate) async fn ask(
    review: Option<&Arc<dyn ToolReview>>,
    cancel: &CancellationToken,
    request: ToolReviewRequest,
) -> crate::Result<()> {
    let Some(review) = review else {
        return Ok(());
    };
    let future = std::panic::AssertUnwindSafe(review.review(request)).catch_unwind();
    tokio::pin!(future);
    let decision = tokio::select! {
        _ = cancel.cancelled() => {
            return Err(ToolError::Rejected {
                reason: "run cancelled".into(),
            });
        }
        result = &mut future => match result {
            Ok(decision) => decision,
            Err(_) => return Err(ToolError::Execution("review failed".into())),
        },
    };
    match decision {
        ReviewDecision::Allow => Ok(()),
        ReviewDecision::Reject { reason } => Err(ToolError::Rejected { reason }),
    }
}

/// 相对工作目录的展示路径（统一 `/`，不可相对时退回绝对路径）。
pub(crate) fn display_path(cwd: &Path, path: &Path) -> String {
    let shown = path.strip_prefix(cwd).unwrap_or(path);
    shown.display().to_string().replace('\\', "/")
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{ReviewDecision, ToolReview, ToolReviewRequest};

    /// 记录调用次数与最后一次请求的假审阅器。
    pub(crate) struct RecordingReview {
        decision: ReviewDecision,
        calls: AtomicUsize,
        seen: Mutex<Vec<ToolReviewRequest>>,
    }

    impl RecordingReview {
        pub(crate) fn new(decision: ReviewDecision) -> Arc<Self> {
            Arc::new(Self {
                decision,
                calls: AtomicUsize::new(0),
                seen: Mutex::new(vec![]),
            })
        }

        pub(crate) fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        pub(crate) fn last(&self) -> ToolReviewRequest {
            self.seen
                .lock()
                .expect("test lock")
                .last()
                .cloned()
                .expect("review was called")
        }
    }

    #[async_trait::async_trait]
    impl ToolReview for RecordingReview {
        async fn review(&self, request: ToolReviewRequest) -> ReviewDecision {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.seen.lock().expect("test lock").push(request);
            self.decision.clone()
        }
    }
}
