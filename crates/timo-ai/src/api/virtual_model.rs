//! 虚拟模型条目的拒绝式适配器（设计 10 §4.2）。
//!
//! 蓝本 `unroutedStream`：虚拟模型没经过路由就被请求时，在流内报错而不是抛异常，
//! 于是任何绕过路由的调用方都拿到与真实适配器同构的 `Error` 终态，且**不发出网络请求**。

use std::time::Instant;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::api::ProviderApi;
use crate::catalog::Model;
use crate::errors::AiError;
use crate::stream::{AssistantMessageStream, AssistantStreamEvent, StreamOptions, error_message};
use crate::types::{AssistantMessage, Context};

/// 虚拟模型的占位适配器：只有拒绝行为，永远不碰传输层。
pub struct VirtualApi;

#[async_trait]
impl ProviderApi for VirtualApi {
    async fn stream(
        &self,
        model: &Model,
        _context: &Context,
        options: StreamOptions,
    ) -> AssistantMessageStream {
        let started = Instant::now();
        let base = AssistantMessage::pending(model.api.as_str(), &model.provider, &model.id, 0);
        let err = AiError::ModelMustBeRouted {
            provider: model.provider.clone(),
            id: model.id.clone(),
        };
        let mut message = error_message(&base, &err, options.cancel.is_cancelled());
        message.duration_ms = Some(started.elapsed().as_millis() as u64);
        let (tx, rx) = mpsc::channel(2);
        let _ = tx.send(AssistantStreamEvent::Start { partial: base }).await;
        let _ = tx.send(AssistantStreamEvent::Error { message }).await;
        AssistantMessageStream::new(rx)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::api::{ByteStream, SseRequest, SseTransport};
    use crate::catalog::ApiKind;
    use crate::types::StopReason;

    /// 只数调用次数的传输层：虚拟模型绝不该走到这里。
    struct CountingTransport {
        posts: AtomicUsize,
    }

    #[async_trait]
    impl SseTransport for CountingTransport {
        async fn post_sse(&self, _request: SseRequest<'_>) -> Result<ByteStream, AiError> {
            self.posts.fetch_add(1, Ordering::SeqCst);
            Err(AiError::Transport("must not be reached".to_string()))
        }
    }

    fn entry() -> Model {
        Model::virtual_entry("router", "auto", "Auto")
    }

    #[tokio::test]
    async fn an_unrouted_virtual_model_fails_inside_the_stream_without_network() {
        let transport = std::sync::Arc::new(CountingTransport {
            posts: AtomicUsize::new(0),
        });
        let api = crate::api::api_with_transport(ApiKind::Virtual, transport.clone());
        let mut stream = api
            .stream(
                &entry(),
                &Context {
                    system_prompt: None,
                    messages: vec![crate::types::user_text("hi", 0)],
                    tools: vec![],
                    thinking_level: None,
                },
                StreamOptions::default(),
            )
            .await;

        let first = stream.next_event().await.expect("start event");
        assert!(
            matches!(first, AssistantStreamEvent::Start { .. }),
            "首事件必须是 Start"
        );
        let final_msg = stream.result().await;
        assert_eq!(final_msg.stop_reason, StopReason::Error);
        assert_eq!(
            final_msg.error_message.as_deref(),
            Some("Virtual model router/auto must be routed before streaming")
        );
        assert_eq!(final_msg.provider, "router");
        assert_eq!(final_msg.model, "auto");
        assert_eq!(final_msg.api, "pi-virtual");
        assert_eq!(
            transport.posts.load(Ordering::SeqCst),
            0,
            "虚拟模型不得发出网络请求"
        );
    }

    #[tokio::test]
    async fn the_api_marker_round_trips_through_the_catalog_name() {
        assert_eq!(ApiKind::Virtual.as_str(), "pi-virtual");
        assert_eq!(ApiKind::parse("pi-virtual"), Some(ApiKind::Virtual));
        assert!(entry().is_virtual());
        let json = serde_json::to_string(&entry()).expect("serialize");
        assert!(json.contains("\"pi-virtual\""), "{json}");
        let back: Model = serde_json::from_str(&json).expect("deserialize");
        assert!(back.is_virtual());
    }
}
