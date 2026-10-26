//! 错误类型。

/// timo-agent 的错误形态；运行期一切失败已编码为事件/终态消息，此类型供
/// 同步前置（装配失败）与调用方诊断。
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// 装配失败（模型/协议/注册表）。
    #[error("agent setup: {0}")]
    Setup(String),
    /// 协议层同步失败（缺凭证等）。
    #[error(transparent)]
    Api(#[from] timo_ai::AiError),
    /// 会话写入失败（消息已入上下文但落盘失败；诊断级，不中断循环）。
    #[error("session write: {0}")]
    Session(String),
    /// run 已在进行中（引导用 steer/follow_up）。
    #[error("a run is already active; use steer/follow_up instead")]
    RunActive,
    /// 未传 prompt 且队列为空。
    #[error("empty prompt")]
    EmptyPrompt,
}
