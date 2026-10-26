//! timo-kernel：内核门面（设计：docs/design/phase-1/06 §2；ADR-0009）。
//!
//! 薄壳（cli / 未来 desktop）**只 import 本 crate**，不穿透内部模块
//! （architecture/01 分层原则 2）。装配链：配置 → 目录 → 模型解析 →
//! 工具注册表（内置 + MCP）→ agent + 会话。

// 测试代码自由使用 unwrap/expect/panic（standards/04 §2）。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod error;
mod kernel;
mod prompt;
mod resolve;

pub use error::KernelError;
pub use kernel::{Kernel, KernelOptions};
// 门面消费用重导出（architecture/01 原则 2：薄壳只依赖本 crate）。
pub use resolve::ResolvedModel;
pub use timo_agent::{AbortHandle, KernelEvent, SessionSink};
pub use timo_ai::{AssistantStreamEvent, Content, Message, Usage};
pub use timo_config::ThinkingLevel;
pub use timo_session::SessionInfo;

/// 结果别名：本 crate 一律以 [`KernelError`] 报错。
pub type Result<T> = std::result::Result<T, KernelError>;
