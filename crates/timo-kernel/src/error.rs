//! 错误类型（设计 06 §4）。

/// timo-kernel 的错误形态：统一为门面级展示，内部错误保留原因链。
#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    /// 配置错误（settings / models.json / mcp.json）。
    #[error("config: {0}")]
    Config(#[from] timo_config::ConfigError),
    /// 会话错误。
    #[error("session: {0}")]
    Session(#[from] timo_session::SessionError),
    /// 协议层同步错误（缺凭证等）。
    #[error("ai: {0}")]
    Ai(#[from] timo_ai::AiError),
    /// 模型解析未命中。
    #[error("model not found: {0}")]
    ModelNotFound(String),
    /// agent 前置错误。
    #[error("agent: {0}")]
    Agent(#[from] timo_agent::AgentError),
    /// MCP 装配错误（单 server 失败不阻断；此处为注册表级错误）。
    #[error("mcp: {0}")]
    Mcp(#[from] timo_mcp::McpError),
    /// 扩展装载/打包错误（逐扩展诊断为主路径；此处为装配级失败）。
    #[error("extension: {0}")]
    Extension(String),
}
