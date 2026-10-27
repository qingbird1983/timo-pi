//! 错误类型（设计 01 §9）。

/// timo-ai 的全部错误形态；协议层失败一律编码为流内 Error 事件，本类型用于
/// 同步前置失败（缺凭证、构造失败）与调用方诊断。
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    /// 认证解析失败（设计 01 §7 优先级穷尽）。
    #[error("no api key for provider: {0}")]
    NoApiKey(String),
    /// 非 2xx 响应（重试穷尽后）。
    #[error("http {status} from {provider}: {body}")]
    Http {
        /// HTTP 状态码。
        status: u16,
        /// 供应商 id。
        provider: String,
        /// 响应体（截断后）。
        body: String,
    },
    /// SSE 流异常（提前结束/未知事件/未知 stop_reason）。
    #[error("sse stream error: {0}")]
    Sse(String),
    /// 响应体/工具参数 JSON 解析失败。
    #[error("json error ({context}): {detail}")]
    Json {
        /// 出错环节描述。
        context: String,
        /// 底层 serde 错误文案。
        detail: String,
    },
    /// 传输层（reqwest/IO）错误。
    #[error("transport: {0}")]
    Transport(String),
    /// 调用到 Phase 1 未实现面（如 OAuth）。
    #[error("not supported in phase 1: {0}")]
    Unsupported(String),
    /// 虚拟模型未经路由就被请求（设计 10 §4.2；文案对齐蓝本）。
    #[error("Virtual model {provider}/{id} must be routed before streaming")]
    ModelMustBeRouted {
        /// 供应商 id。
        provider: String,
        /// 模型 id。
        id: String,
    },
    /// 上游配置错误透传。
    #[error(transparent)]
    Config(#[from] timo_config::ConfigError),
}

/// HTTP 响应体截断上限（错误信息可读且不刷屏）。
pub(crate) const ERROR_BODY_CAP: usize = 2048;
