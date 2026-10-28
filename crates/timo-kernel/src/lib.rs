//! timo-kernel：内核门面（设计：docs/design/phase-1/06 §2；ADR-0009）。
//!
//! 薄壳（cli / 未来 desktop）**只 import 本 crate**，不穿透内部模块
//! （architecture/01 分层原则 2）。装配链：配置 → 目录 → 模型解析 →
//! 工具注册表（内置 + MCP）→ agent + 会话。

// 测试代码自由使用 unwrap/expect/panic（standards/04 §2）。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod cache_stats;
mod codemode;
mod commands;
mod error;
mod extensions;
mod kernel;
pub mod plugins;
mod prompt;
mod resolve;
mod session_route;
mod usage;
mod warmer;

pub use cache_stats::{CacheMiss, CacheStats};
pub use commands::{CommandSource, CommandStatus, SlashCommand};

pub use error::KernelError;
pub use extensions::{
    ExtensionOptions, ExtensionRuntime, ExtensionSummary, install_extension, load_extensions,
    permissions_path, remove_extension, split_command, summarize, trust_extension,
};
pub use kernel::{Kernel, KernelOptions};
pub use prompt::{ResourceWarning, WarningKind};
pub use timo_ext::{
    Capabilities, DiagnosticReport, ExtensionHost, GrantState, PermissionStore, Scope, discover,
};

/// 平台数据目录（扩展与授权文件的默认根）。
///
/// # Errors
/// 平台目录不可得 → [`KernelError::Config`]。
pub fn data_dir() -> Result<PathBuf> {
    Ok(timo_config::agent_dir()?)
}

/// 薄壳需要的配置面（凭证引用、钥匙串、供应商文件）。
///
/// 只暴露壳层实际消费的项；薄壳仍只依赖 `timo-kernel`（architecture/01 §2）。
pub mod config {
    pub use timo_config::{
        CredentialSource, KEYCHAIN_SERVICE, ValueRef, keychain_delete, keychain_get, keychain_set,
        load_custom_providers, models_path, set_provider_api_key,
    };
}
// 门面消费用重导出（architecture/01 原则 2：薄壳只依赖本 crate）。
pub use resolve::ResolvedModel;
use std::path::PathBuf;
pub use timo_agent::{AbortHandle, KernelEvent, SessionSink};
pub use timo_ai::{
    AssistantMessage, AssistantStreamEvent, Content, Message, StopReason, ToolResultMessage, Usage,
    UserMessage,
};
pub use timo_config::ThinkingLevel;
pub use timo_session::{SessionInfo, format_utc_minutes};
pub use timo_tools::{ReviewDecision, ReviewKind, ToolOutput, ToolReview, ToolReviewRequest};
pub use usage::{UsageBucket, UsageSummary, UsageTotals};
pub use warmer::WarmingState;

/// 结果别名：本 crate 一律以 [`KernelError`] 报错。
pub type Result<T> = std::result::Result<T, KernelError>;
