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
    /// 选中了未路由的虚拟模型（设计 10 §4.2；参数为 `provider/id`）。
    #[error("Virtual model {0} must be routed before streaming")]
    ModelMustBeRouted(String),
    /// agent 前置错误。
    #[error("agent: {0}")]
    Agent(#[from] timo_agent::AgentError),
    /// 手动压缩错误（隔离于 run 装配，CLI 只打一行原因）。
    #[error("compaction: {0}")]
    Compaction(#[from] timo_agent::compaction::CompactionError),
    /// 命令前置条件不满足（如草稿会话执行 `/compact`）。
    #[error("{0}")]
    Unavailable(String),
    /// MCP 装配错误（单 server 失败不阻断；此处为注册表级错误）。
    #[error("mcp: {0}")]
    Mcp(#[from] timo_mcp::McpError),
    /// 扩展装载/打包错误（逐扩展诊断为主路径；此处为装配级失败）。
    #[error("extension: {0}")]
    Extension(String),
    /// 插件代理的工具执行错误（`actions.runTool`）。
    #[error("tool: {0}")]
    Tool(#[from] timo_tools::ToolError),
}
