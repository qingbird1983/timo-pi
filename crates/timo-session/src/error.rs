//! 错误类型。

use std::path::PathBuf;

/// timo-session 的全部错误形态。
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// 文件/目录 IO 失败。
    #[error("session io ({path}): {source}")]
    Io {
        /// 涉及路径。
        path: PathBuf,
        /// 底层 IO 错误。
        source: std::io::Error,
    },
    /// 中间行损坏（真损坏不静默吞；末行截断由读取端容忍，见设计 04 §5）。
    #[error("corrupt session entry at {path}:{line}: {reason}")]
    CorruptEntry {
        /// 文件路径。
        path: PathBuf,
        /// 行号（1 起）。
        line: usize,
        /// 原因。
        reason: String,
    },
    /// 文件缺少 session.header 首条或格式不符。
    #[error("invalid session file {path}: {reason}")]
    InvalidFile {
        /// 文件路径。
        path: PathBuf,
        /// 原因。
        reason: String,
    },
    /// 会话未找到。
    #[error("session not found: {0}")]
    NotFound(String),
}
