//! openai-completions 适配器（设计 01 §5；pi: api/openai-completions.ts）。

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

/// OpenAI Chat Completions 适配器（含兼容网关：deepseek/moonshot/openrouter 等）。
pub struct OpenAiApi {
    http: reqwest::Client,
}

impl OpenAiApi {
    /// 由共享 HTTP 客户端构造。
    #[must_use]
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    /// 端点：`{base_url}/chat/completions`。
    fn endpoint(model: &Model, options: &StreamOptions) -> String {
        let base = options.base_url.as_deref().unwrap_or(&model.base_url);
        format!("{}/chat/completions", base.trim_end_matches('/'))
    }
}

#[async_trait]
impl ProviderApi for OpenAiApi {
    async fn stream(
        &self,
        model: &Model,
        context: &Context,
        options: StreamOptions,
    ) -> AssistantMessageStream {
        let http = self.http.clone();
        let model = model.clone();
        let context = context.clone();
        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(async move {
            let started = Instant::now();
            let base =
                AssistantMessage::pending("openai-completions", &model.provider, &model.id, 0);
            run(&http, &model, &context, &options, &tx, base, started).await;
        });
        AssistantMessageStream::new(rx)
    }
}

async fn run(
    http: &reqwest::Client,
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
        let body = request::build_body(model, context, options);
        let mut headers = vec![("authorization".to_string(), format!("Bearer {api_key}"))];
        headers.extend(model.headers.iter().cloned());
        headers.extend(options.headers.iter().cloned());
        http::post_sse(
            http,
            http::SseRequest {
                url: &OpenAiApi::endpoint(model, options),
                headers,
                body: &body,
                options,
                provider: &model.provider,
            },
        )
        .await
    })()
    .await;

    let resp = match outcome {
        Ok(r) => r,
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

    let mut state = stream::OpenAiStream::new(base, model.cost.clone(), model.compat.clone());
    let _ = tx
        .send(AssistantStreamEvent::Start {
            partial: state.partial().clone(),
        })
        .await;
    let mut dec = SseDecoder::new();
    let mut body = resp.bytes_stream();
    let mut aborted = false;
    loop {
        tokio::select! {
            _ = options.cancel.cancelled() => { aborted = true; break; }
            chunk = body.next() => {
                let bytes = match chunk {
                    Some(Ok(b)) => b,
                    Some(Err(e)) => {
                        let err = AiError::Transport(e.to_string());
                        let _ = tx.send(AssistantStreamEvent::Error {
                            message: finalize(error_message(state.partial(), &err, false), started),
                        }).await;
                        return;
                    }
                    None => break,
                };
                for ev in dec.push(&String::from_utf8_lossy(&bytes)) {
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
        for ev in dec.finish() {
            if let Ok(Some(out)) = state.on_sse(&ev) {
                let _ = tx.send(out).await;
            }
        }
        let partial = state.partial().clone();
        match state.finish() {
            Ok(m) => AssistantStreamEvent::Done {
                reason: m.stop_reason,
                message: finalize(m, started),
            },
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
