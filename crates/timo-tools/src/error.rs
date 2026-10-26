//! 错误类型（设计 03 §5）。

use std::path::PathBuf;

/// 工具执行错误：execute 返回 `Err` → 循环转为错误 tool_result（is_error=true）。
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// 注册表未命中。
    #[error("Tool {name} not found")]
    ToolNotFound {
        /// 工具名。
        name: String,
    },
    /// JSON Schema 校验失败。
    #[error("Invalid parameters: {reason}")]
    InvalidParams {
        /// 校验器文案。
        reason: String,
    },
    /// 文件系统错误（read/write/edit/grep/find）。
    #[error("Could not access {path}: {source}")]
    Io {
        /// 涉及路径。
        path: PathBuf,
        /// 底层错误。
        source: std::io::Error,
    },
    /// shell 工具 spawn/等待失败。
    #[error("Could not run command: {source}")]
    Subprocess {
        /// 底层错误。
        source: std::io::Error,
    },
    /// shell 工具超时。
    #[error("Command timed out after {seconds} seconds")]
    Timeout {
        /// 超时秒数。
        seconds: u64,
    },
    /// 取消（转 "Command aborted" 等文案）。
    #[error("Operation aborted")]
    Cancelled,
    /// MCP 工具错误（timo-mcp 透传）。
    #[error("MCP: {0}")]
    Mcp(String),
    /// 扩展工具错误（原始文案）。
    #[error("{0}")]
    Execution(String),
}
