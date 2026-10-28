//! 事件桥测试（设计 04 §3/§6/§7）：钩子 → JS 派发 → 回读白名单。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::json;
use timo_agent::{BeforeAgentStart, LoopHooks, ToolOutcome};
use timo_ai::{Content, ProviderHooks, user_text};
use timo_ext::{BasicServices, ErrorCode, ExtHooks, ExtensionHost, Grants, HostServices};

const BRIDGE: &str = r#"
module.exports = function (pi) {
  pi.on("before_agent_start", async () => ({ systemPrompt: "ext sys" }));
  pi.on("context", async (event) => ({ messages: event.messages.slice(0, 1) }));
  pi.on("tool_call", async (event, ctx) => {
    if (event.input && event.input.block) {
      return { block: true, reason: "no" };
    }
    return { input: { ...event.input, prompt: ctx.getSystemPrompt() } };
  });
  pi.on("tool_result", async () => ({
    content: [{ type: "text", text: "patched" }],
  }));
  pi.on("before_provider_request", async (payload) => ({ ...payload, marker: "seen" }));
  pi.on("before_provider_headers", async () => ({
    headers: { authorization: null, "x-ext": "1" },
  }));
  pi.on("after_provider_response", async () => undefined);
};
"#;

fn setup(source: &str) -> (tempfile::TempDir, Arc<ExtensionHost>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bundle.cjs");
    std::fs::write(&path, source).unwrap();
    let services: Arc<dyn HostServices> = Arc::new(BasicServices::new(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        Grants::default(),
        json!({}),
    ));
    let host = Arc::new(ExtensionHost::start(services, Duration::from_secs(5)).unwrap());
    host.load("bridge", &path).unwrap();
    (dir, host)
}

fn tool_call(arguments: serde_json::Value) -> Content {
    Content::ToolCall {
        id: "call_1".into(),
        name: "read".into(),
        arguments,
        arguments_error: None,
    }
}

#[tokio::test]
async fn before_agent_start_updates_host_prompt() {
    let (_dir, host) = setup(BRIDGE);
    let hooks = ExtHooks::new(Arc::clone(&host));
    let prompt = user_text("hi", 1);
    let replaced = hooks
        .before_agent_start(BeforeAgentStart {
            prompt: std::slice::from_ref(&prompt),
            system_prompt: "base",
        })
        .await;
    assert_eq!(replaced.as_deref(), Some("ext sys"));
    assert_eq!(host.system_prompt(), "ext sys");
    assert_eq!(hooks.system_prompt().as_deref(), Some("ext sys"));
}

#[tokio::test]
async fn context_transform_uses_first_message_only() {
    let (_dir, host) = setup(BRIDGE);
    let hooks = ExtHooks::new(host);
    let messages = vec![user_text("a", 1), user_text("b", 2)];
    let out = hooks
        .transform_context(messages)
        .await
        .expect("transformed");
    assert_eq!(out.len(), 1);
}

#[tokio::test]
async fn tool_call_bridge_blocks_and_rewrites() {
    let (_dir, host) = setup(BRIDGE);
    let hooks = ExtHooks::new(Arc::clone(&host));
    let blocked = hooks
        .before_tool_call(&tool_call(json!({ "block": true })))
        .await
        .unwrap();
    assert_eq!(blocked.block.as_deref(), Some("no"));
    assert!(blocked.args.is_none());

    hooks
        .before_agent_start(BeforeAgentStart {
            prompt: &[],
            system_prompt: "base",
        })
        .await;
    let rewritten = hooks
        .before_tool_call(&tool_call(json!({ "path": "a" })))
        .await
        .unwrap();
    assert!(rewritten.block.is_none());
    let args = rewritten.args.unwrap();
    assert_eq!(args["path"], "a");
    assert_eq!(args["prompt"], "ext sys");
}

#[tokio::test]
async fn tool_result_bridge_patches_content() {
    let (_dir, host) = setup(BRIDGE);
    let hooks = ExtHooks::new(host);
    let output = timo_tools::ToolOutput::text("raw");
    let patch = hooks
        .after_tool_call(&ToolOutcome {
            tool_call_id: "call_1",
            tool_name: "read",
            output: &output,
        })
        .await
        .unwrap();
    match &patch.content.unwrap()[0] {
        Content::Text { text } => assert_eq!(text, "patched"),
        other => panic!("text expected: {other:?}"),
    }
}

#[tokio::test]
async fn provider_hooks_round_trip() {
    let (_dir, host) = setup(BRIDGE);
    let hooks = ExtHooks::new(host);
    let body = hooks
        .before_request(json!({ "model": "m" }))
        .await
        .expect("replaced body");
    assert_eq!(body["marker"], "seen");
    assert_eq!(body["model"], "m");

    let patch = hooks
        .before_headers(vec![("authorization".into(), "Bearer x".into())])
        .await
        .expect("header patch");
    assert!(
        patch
            .iter()
            .any(|(name, value)| name == "authorization" && value.is_none())
    );
    assert!(
        patch
            .iter()
            .any(|(name, value)| name == "x-ext" && value.as_deref() == Some("1"))
    );

    hooks.after_response(200, vec![]).await;
    assert!(
        !host_errors(&hooks).contains(&ErrorCode::E302),
        "unexpected handler errors"
    );
}

#[tokio::test]
async fn abort_flag_retires_dispatch() {
    let (_dir, host) = setup(BRIDGE);
    let flag = Arc::new(AtomicBool::new(true));
    let hooks = ExtHooks::new(host).with_abort_flag(flag);
    let prompt = user_text("hi", 1);
    let replaced = hooks
        .before_agent_start(BeforeAgentStart {
            prompt: std::slice::from_ref(&prompt),
            system_prompt: "base",
        })
        .await;
    assert!(replaced.is_none());
}

fn host_errors(hooks: &ExtHooks) -> Vec<ErrorCode> {
    hooks
        .host()
        .diagnostics_for("bridge")
        .into_iter()
        .map(|entry| entry.code)
        .collect()
}
