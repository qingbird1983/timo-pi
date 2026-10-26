//! timo-agent：agent 循环、事件总线与取消（设计：docs/design/phase-1/02）。
//!
//! 循环内**零硬编码 hook/扩展分支**（接缝 #2 铁律）：一切外泄走事件总线，
//! 工具走统一注册表（内置/MCP 同管线）。语义对齐 pi agent-loop（ADR-0011）。

// 测试代码自由使用 unwrap/expect/panic（standards/04 §2）。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod agent;
mod error;
mod events;
mod hooks;
mod queue;
mod run;
mod tool_calls;

pub use agent::{AbortHandle, AgentHandle};
pub use error::AgentError;
pub use events::{EventBus, KernelEvent};
pub use hooks::{
    BeforeAgentStart, BeforeToolCall, LoopHooks, RouteChoice, RouteRequest, ToolOutcome,
    ToolOutputPatch,
};
pub use queue::PendingMessageQueue;
pub use run::{
    LoopConfig, RunChannels, RunOutcome, SessionSink, ToolExecutionMode, run_agent_loop,
};

/// 结果别名：本 crate 一律以 [`AgentError`] 报错。
pub type Result<T> = std::result::Result<T, AgentError>;
