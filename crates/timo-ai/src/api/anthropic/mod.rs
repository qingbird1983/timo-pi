//! anthropic-messages 适配器（设计 01 §4；pi: api/anthropic-messages.ts）。

pub(crate) mod convert;
pub(crate) mod request;
pub(crate) mod stream;

use std::time::Instant;

use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::api::{ProviderApi, http, sse::SseDecoder};
use crate::catalog::Model;
use crate::errors::AiError;
use crate::stream::{AssistantMessageStream, AssistantStreamEvent, StreamOptions, error_message};
use crate::types::{AssistantMessage, Context, StopReason};

/// Anthropic Messages API 适配器。
pub struct AnthropicApi {
    transport: std::sync::Arc<dyn http::SseTransport>,
}

impl AnthropicApi {
    /// 由共享 HTTP 客户端构造（reqwest 传输）。
    #[must_use]
    pub fn new(http: reqwest::Client) -> Self {
        Self::with_transport(std::sync::Arc::new(http::ReqwestTransport::new(http)))
    }

    /// 由任意传输实现构造（fixture 回放 / 自定义网关注入点）。
    #[must_use]
    pub fn with_transport(transport: std::sync::Arc<dyn http::SseTransport>) -> Self {
        Self { transport }
    }

    /// 端点：`{base_url}/v1/messages`。
    fn endpoint(model: &Model, options: &StreamOptions) -> String {
        let base = options.base_url.as_deref().unwrap_or(&model.base_url);
        let base = base.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{base}/messages")
        } else {
            format!("{base}/v1/messages")
        }
    }
}

#[async_trait]
impl ProviderApi for AnthropicApi {
    async fn stream(
        &self,
        model: &Model,
        context: &Context,
        options: StreamOptions,
    ) -> AssistantMessageStream {
        let transport = std::sync::Arc::clone(&self.transport);
        let model = model.clone();
        let context = context.clone();
        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(async move {
            let started = Instant::now();
            let base =
                AssistantMessage::pending("anthropic-messages", &model.provider, &model.id, 0);
            run(&transport, &model, &context, &options, &tx, base, started).await;
        });
        AssistantMessageStream::new(rx)
    }
}

#[allow(clippy::too_many_arguments)]
async fn run(
    transport: &std::sync::Arc<dyn http::SseTransport>,
    model: &Model,
    context: &Context,
    options: &StreamOptions,
    tx: &mpsc::Sender<AssistantStreamEvent>,
    base: AssistantMessage,
    started: Instant,
) {
    let outcome = (|| async {
        let api_key = options
            .api_key
            .clone()
            .ok_or_else(|| AiError::NoApiKey(format!("provider {}", model.provider)))?;
        let mut body = request::build_body(model, context, options);
        if let Some(hooks) = &options.provider_hooks
            && let Some(replaced) = hooks.before_request(body.clone()).await
        {
            body = replaced;
        }
        let url = AnthropicApi::endpoint(model, options);
        let mut headers = vec![
            ("x-api-key".to_string(), api_key),
            ("anthropic-version".to_string(), "2023-06-01".to_string()),
        ];
        headers.extend(model.headers.iter().cloned());
        headers.extend(options.headers.iter().cloned());
        if let Some(hooks) = &options.provider_hooks
            && let Some(patch) = hooks.before_headers(headers.clone()).await
        {
            headers = crate::hooks::apply_header_patch(headers, patch);
        }
        transport
            .post_sse(http::SseRequest {
                url: &url,
                headers,
                body: &body,
                options,
                provider: &model.provider,
            })
            .await
    })()
    .await;

    let mut body = match outcome {
        Ok(stream) => stream,
        Err(e) => {
            let _ = tx
                .send(AssistantStreamEvent::Error {
                    message: finalize(
                        error_message(&base, &e, options.cancel.is_cancelled()),
                        started,
                    ),
                })
                .await;
            return;
        }
    };

    let mut state = stream::AnthropicStream::new(base, model.cost.clone());
    let _ = tx
        .send(AssistantStreamEvent::Start {
            partial: state.partial().clone(),
        })
        .await;
    let mut dec = SseDecoder::new();
    let mut aborted = false;
    loop {
        tokio::select! {
            _ = options.cancel.cancelled() => { aborted = true; break; }
            chunk = body.next() => {
                let bytes = match chunk {
                    Some(Ok(b)) => b,
                    Some(Err(e)) => {
                        let err = AiError::Transport(e);
                        let _ = tx.send(AssistantStreamEvent::Error {
                            message: finalize(error_message(state.partial(), &err, false), started),
                        }).await;
                        return;
                    }
                    None => break,
                };
                let text = String::from_utf8_lossy(&bytes);
                for ev in dec.push(&text) {
                    match state.on_sse(&ev) {
                        Ok(Some(out)) => { if tx.send(out).await.is_err() { return; } }
                        Ok(None) => {}
                        Err(e) => {
                            let _ = tx.send(AssistantStreamEvent::Error {
                                message: finalize(error_message(state.partial(), &e, false), started),
                            }).await;
                            return;
                        }
                    }
                }
            }
        }
    }
    let terminal = if aborted {
        let mut m = state.partial().clone();
        m.stop_reason = StopReason::Aborted;
        m.error_message = Some("cancelled by caller".to_string());
        AssistantStreamEvent::Error {
            message: finalize(m, started),
        }
    } else {
        let rest = dec.finish();
        for ev in &rest {
            if let Ok(Some(out)) = state.on_sse(ev) {
                let _ = tx.send(out).await;
            }
        }
        let partial = state.partial().clone();
        match state.finish() {
            Ok(done) => {
                let reason = done.stop_reason;
                AssistantStreamEvent::Done {
                    reason,
                    message: finalize(done, started),
                }
            }
            Err(e) => AssistantStreamEvent::Error {
                message: finalize(error_message(&partial, &e, false), started),
            },
        }
    };
    let _ = tx.send(terminal).await;
}

fn finalize(mut m: AssistantMessage, started: Instant) -> AssistantMessage {
    m.duration_ms = Some(started.elapsed().as_millis() as u64);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_variants() {
        let mut m = sample_model("https://api.anthropic.com");
        let o = StreamOptions::default();
        assert_eq!(
            AnthropicApi::endpoint(&m, &o),
            "https://api.anthropic.com/v1/messages"
        );
        m.base_url = "https://proxy.example/v1".into();
        assert_eq!(
            AnthropicApi::endpoint(&m, &o),
            "https://proxy.example/v1/messages"
        );
    }

    fn sample_model(base: &str) -> Model {
        serde_json::from_value(serde_json::json!({
            "id": "claude-x", "name": "X", "provider": "anthropic",
            "api": "anthropic-messages", "base_url": base,
            "reasoning": true, "input": ["text"], "cost": {},
            "context_window": 200000, "max_tokens": 64000
        }))
        .expect("model")
    }
}
