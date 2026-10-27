//! timo-tools：工具元模型与内置工具（设计：docs/design/phase-1/03）。
//!
//! 内置/MCP/（Phase 2）扩展工具共用同一注册表与执行管线（接缝 #4）；
//! 参数校验统一 JSON Schema（接缝 #5）。行为契约对齐 pi core/tools（ADR-0011）。

// 测试代码自由使用 unwrap/expect/panic（standards/04 §2）。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod definition;
mod error;
mod mutate;
pub mod nested;
mod registry;
mod review;
pub mod tools;
mod truncate;

pub use definition::{
    Annotations, ExecutionMode, Exposure, Namespace, ToolCallCtx, ToolDefinition, ToolFuture,
    ToolOutput,
};
pub use error::ToolError;
pub use mutate::with_file_mutation_queue;
pub use nested::{
    CallableTool, NestedCall, NestedCallRecord, NestedCallStatus, NestedCallSummary, NestedOutcome,
    NestedToolHost, limits, merge_usage, output_text,
};
pub use registry::ToolRegistry;
pub use review::{ReviewDecision, ReviewKind, ToolReview, ToolReviewRequest};
pub use truncate::{
    GREP_MAX_LINE, MAX_BYTES, MAX_LINES, truncate_head, truncate_middle, truncate_tail,
};

/// 结果别名：工具执行以 [`ToolError`] 报错（循环转为错误 result）。
pub type Result<T> = std::result::Result<T, ToolError>;
