//! ProviderHooks 集成测试（设计 04 §2.2）：请求体与请求头改写点。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};
use timo_ai::{
    AiError, ApiKind, ByteStream, Catalog, Context, ProviderHooks, SseRequest, SseTransport,
    StreamOptions, api_with_transport, user_text,
};

#[derive(Default)]
struct CaptureTransport {
    body: Mutex<Option<Value>>,
    headers: Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl SseTransport for CaptureTransport {
    async fn post_sse(&self, request: SseRequest<'_>) -> Result<ByteStream, AiError> {
        *self.body.lock().unwrap() = Some(request.body.clone());
        *self.headers.lock().unwrap() = request.headers.clone();
        Ok(Box::pin(futures::stream::empty::<Result<Vec<u8>, String>>()))
    }
}

struct Hooks;

#[async_trait]
impl ProviderHooks for Hooks {
    async fn before_request(&self, mut payload: Value) -> Option<Value> {
        if let Some(object) = payload.as_object_mut() {
            object.insert("marker".into(), json!("seen"));
        }
        Some(payload)
    }

    async fn before_headers(
        &self,
        headers: Vec<(String, String)>,
    ) -> Option<Vec<(String, Option<String>)>> {
        let mut patch = vec![
            (
                "authorization".to_string(),
                Some("Bearer patched".to_string()),
            ),
            ("x-secret".to_string(), None),
        ];
        if !headers.iter().any(|(name, _)| name == "x-secret") {
            patch.remove(1);
        }
        Some(patch)
    }
}

fn model() -> timo_ai::Model {
    Catalog::builtin()
        .get("openai", "gpt-4")
        .expect("catalog model")
        .clone()
}

fn context() -> Context {
    Context {
        system_prompt: Some("sys".into()),
        messages: vec![user_text("hi", 0)],
        tools: vec![],
        thinking_level: None,
    }
}

#[tokio::test]
async fn hooks_rewrite_body_and_headers() {
    let transport = Arc::new(CaptureTransport::default());
    let transport_ref: Arc<dyn SseTransport> = transport.clone();
    let api = api_with_transport(ApiKind::OpenaiCompletions, transport_ref);
    let options = StreamOptions {
        api_key: Some("real".into()),
        headers: vec![("x-secret".to_string(), "drop-me".to_string())],
        provider_hooks: Some(Arc::new(Hooks)),
        ..StreamOptions::default()
    };
    let mut stream = api.stream(&model(), &context(), options).await;
    while stream.next_event().await.is_some() {}

    let body = transport.body.lock().unwrap().clone().expect("body");
    assert_eq!(body["marker"], "seen");
    let headers = transport.headers.lock().unwrap().clone();
    assert!(
        headers
            .iter()
            .any(|(name, value)| name == "authorization" && value == "Bearer patched")
    );
    assert!(!headers.iter().any(|(name, _)| name == "x-secret"));
}
