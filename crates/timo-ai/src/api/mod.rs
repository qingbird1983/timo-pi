//! 协议适配层（设计 01 §3~5；接缝 #3：ProviderApi 唯一 trait）。

pub mod anthropic;
pub mod http;
pub mod openai;
pub mod sse;
pub mod virtual_model;

pub use http::{ByteStream, ReqwestTransport, SseRequest, SseTransport};

use std::sync::Arc;

use async_trait::async_trait;

use crate::catalog::{ApiKind, Model};
use crate::stream::{AssistantMessageStream, StreamOptions};
use crate::types::Context;

/// 协议适配器唯一入口（接缝 #3：新协议 = 新实现 + 目录数据，不触碰上层）。
///
/// 契约：仅缺凭证可同步失败；此后一切失败经流内 `Error` 事件表达，适配器不
/// panic、不返回 Err。
#[async_trait]
pub trait ProviderApi: Send + Sync {
    /// 发起流式补全请求。
    async fn stream(
        &self,
        model: &Model,
        context: &Context,
        options: StreamOptions,
    ) -> AssistantMessageStream;
}

/// 按协议类型取适配器（进程内共享 HTTP 客户端）。
#[must_use]
pub fn api_for(kind: ApiKind) -> Arc<dyn ProviderApi> {
    api_with_transport(
        kind,
        Arc::new(ReqwestTransport::new(reqwest::Client::new())),
    )
}

/// 按协议类型取适配器并使用指定传输（fixture 回放 / 自定义网关）。
#[must_use]
pub fn api_with_transport(kind: ApiKind, transport: Arc<dyn SseTransport>) -> Arc<dyn ProviderApi> {
    match kind {
        ApiKind::AnthropicMessages => Arc::new(anthropic::AnthropicApi::with_transport(transport)),
        ApiKind::OpenaiCompletions => Arc::new(openai::OpenAiApi::with_transport(transport)),
        ApiKind::Virtual => Arc::new(virtual_model::VirtualApi),
    }
}
