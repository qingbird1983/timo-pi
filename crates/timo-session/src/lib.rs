//! timo-session：会话持久化（设计：docs/design/phase-1/04；格式定稿：architecture/05）。
//!
//! 本 crate 是磁盘格式的**唯一所有者**（architecture/01 分层原则 4）；
//! entry 流是唯一权威，其余一切（列表/恢复）都从它重建。

// 测试代码自由使用 unwrap/expect/panic（standards/04 §2）。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod entry;
mod error;
mod reader;
mod writer;

pub use entry::{ENTRY_VERSION, Entry, SessionHeader, kinds};
pub use error::SessionError;
pub use reader::{SessionInfo, list_sessions, project_context, read_entries, serialize_export};
pub use writer::{IdGen, SessionWriter};

/// 结果别名：本 crate 一律以 [`SessionError`] 报错。
pub type Result<T> = std::result::Result<T, SessionError>;
