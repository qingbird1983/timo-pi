//! timo-ai：LLM 协议层——统一消息模型、anthropic-messages / openai-completions
//! 流式适配、模型目录、认证与重试（设计：docs/design/phase-1/01）。
//!
//! 行为语义以 pi（packages/ai）为唯一蓝本（ADR-0011）；偏离登记见设计 00 §5。

// 测试代码自由使用 unwrap/expect/panic（standards/04 §2）。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod api;
pub mod auth;
pub mod catalog;
pub mod errors;
pub mod hooks;
pub mod retry;
pub mod stream;
pub mod types;

/// 结果别名：同步路径以 [`errors::AiError`] 报错。
pub type Result<T> = std::result::Result<T, errors::AiError>;

pub use api::{ByteStream, ProviderApi, SseRequest, SseTransport, api_for, api_with_transport};
pub use auth::AuthResolver;
pub use catalog::{ApiKind, Catalog, Compat, Model, RateCard, calculate_cost};
pub use errors::AiError;
pub use hooks::{ProviderHooks, apply_header_patch, merge_headers};
pub use stream::{AssistantMessageStream, AssistantStreamEvent, StreamOptions};
pub use types::{
    AssistantMessage, Content, Context, Cost, Message, StopReason, SystemMessage, Tool,
    ToolResultMessage, Usage, UserMessage, user_text,
};
