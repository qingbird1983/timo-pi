//! timo-mcp：rmcp 薄封装——server 配置、惰性连接、工具并入统一管线
//! （设计：docs/design/phase-1/06 §1；ADR-0005；行为对齐 pi extensions/mcp）。

// 测试代码自由使用 unwrap/expect/panic（standards/04 §2）。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod config;
mod convert;
mod error;
mod naming;
mod runtime;

pub use config::{
    DEFAULT_TIMEOUT_SECS, KindFields, RegistryEntry, load_server_registry, valid_server_name,
};
pub use error::McpError;
pub use naming::mcp_tool_name;
pub use runtime::{McpRuntime, ServerState};

/// 结果别名：本 crate 一律以 [`McpError`] 报错。
pub type Result<T> = std::result::Result<T, McpError>;
