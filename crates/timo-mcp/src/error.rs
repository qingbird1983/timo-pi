//! 错误类型（设计 06 §4）。

use std::path::PathBuf;

/// timo-mcp 的全部错误形态。
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    /// mcp.json 非法。
    #[error("invalid mcp config {path}: {reason}")]
    ConfigInvalid {
        /// 文件路径。
        path: PathBuf,
        /// 原因。
        reason: String,
    },
    /// server 配置项非法（键名冲突等）。
    #[error("invalid mcp server config ({server}): {reason}")]
    ServerInvalid {
        /// server 名。
        server: String,
        /// 原因。
        reason: String,
    },
    /// 连接失败（惰性；诊断进事件总线，不阻断内核）。
    #[error("mcp server `{server}` connect failed: {detail}")]
    ConnectFailed {
        /// server 名。
        server: String,
        /// 底层错误文案。
        detail: String,
    },
    /// 工具调用失败。
    #[error("mcp tool `{tool}` failed: {detail}")]
    CallFailed {
        /// 工具名。
        tool: String,
        /// 底层错误文案。
        detail: String,
    },
    /// 请求超时。
    #[error("mcp server `{server}` timeout after {secs}s")]
    Timeout {
        /// server 名。
        server: String,
        /// 超时秒数。
        secs: u64,
    },
}
