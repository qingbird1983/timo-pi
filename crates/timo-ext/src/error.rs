//! 扩展宿主错误（设计 02 §5 的 Rust 侧形态；诊断码见 [`crate::diag::ErrorCode`]）。

use std::path::PathBuf;

/// timo-ext 的错误形态；`Display` 输出面向用户的一句话。
#[derive(Debug, thiserror::Error)]
pub enum ExtError {
    /// 读取扩展文件/目录失败（E206）。
    #[error("cannot read {path}: {reason}")]
    Read {
        /// 出错路径。
        path: PathBuf,
        /// 系统错误文案。
        reason: String,
    },
    /// 目录中找不到可装载入口（E101）。
    #[error("extension entry not found in {0}")]
    EntryNotFound(PathBuf),
    /// manifest 缺失或非法（E110/E205）。
    #[error("invalid manifest {path}: {reason}")]
    InvalidManifest {
        /// manifest 路径。
        path: PathBuf,
        /// 失败原因。
        reason: String,
    },
    /// 源码目录无法计算指纹（E109 的前置失败）。
    #[error("cannot hash source dir {path}: {reason}")]
    Hash {
        /// 源码目录。
        path: PathBuf,
        /// 失败原因。
        reason: String,
    },
}
