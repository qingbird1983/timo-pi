//! 虚拟模型注册与路由（设计 10 §4.2）：JS 路由函数、目录快照与注销。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use timo_agent::{LoopHooks, RouteReason, RouteRequest};
use timo_ai::Model;
use timo_config::ThinkingLevel;
use timo_ext::{BasicServices, ErrorCode, ExtHooks, ExtensionHost, Grants, HostServices};

fn model(id: &str) -> Value {
    serde_json::to_value(Model::descriptor(
        "mock",
        id,
        timo_ai::ApiKind::AnthropicMessages,
    ))
    .expect("model json")
}

fn services(models: Vec<Value>) -> Arc<dyn HostServices> {
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Arc::new(BasicServices::new(cwd, Grants::default(), json!({})).with_models(models))
}

struct Fixture {
    _dir: tempfile::TempDir,
    host: Arc<ExtensionHost>,
}

impl Fixture {
    fn new(source: &str, models: Vec<Value>) -> Self {
        Self::try_new(source, models).expect("extension loads")
    }

    fn try_new(source: &str, models: Vec<Value>) -> Result<Self, String> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bundle.cjs");
        std::fs::write(&path, source).unwrap();
        let host = ExtensionHost::start(services(models), Duration::from_secs(30))
            .map_err(|error| error.to_string())?;
        host.load("router", &path)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            _dir: dir,
            host: Arc::new(host),
        })
    }
}

const ROUTER: &str = r#"
module.exports = function (pi) {
  pi.registerVirtualModel({
    provider: "mock",
    id: "auto",
    name: "Auto",
    thinkingLevels: ["low", "high"],
    route(request, ctx) {
      const small = ctx.modelRegistry.find("mock", "small");
      const large = ctx.modelRegistry.find("mock", "large");
      const foreign = ctx.modelRegistry.find("nope", "missing");
      return {
        model: request.reason === "user" ? small : large,
        thinkingLevel: request.state && request.state.phase === "build" ? "high" : "medium",
        state: {
          phase: request.reason,
          missing: foreign === undefined,
          previous: request.previous ? request.previous.model.id : null,
          failed: request.failed ? request.failed.model.id : null,
        },
      };
    },
  });
  pi.registerCommand("off", {
    description: "unregister",
    handler: async () => {
      pi.unregisterVirtualModel("mock", "auto");
    },
  });
};
"#;

fn payload(reason: &str, state: Option<Value>) -> Value {
    let mut body = json!({
        "model": json!({ "provider": "mock", "id": "auto", "name": "Auto" }),
        "thinkingLevel": "low",
        "reason": reason,
        "messages": [],
    });
    if let Some(state) = state {
        body["state"] = state;
    }
    body
}

#[test]
fn a_registered_route_receives_the_request_and_returns_a_catalog_model() {
    let fixture = Fixture::new(ROUTER, vec![model("small"), model("large")]);
    let entries = fixture.host.virtual_models();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        (entries[0].provider.as_str(), entries[0].id.as_str()),
        ("mock", "auto")
    );
    assert_eq!(entries[0].name, "Auto");
    assert_eq!(entries[0].thinking_levels, vec!["low", "high"]);

    let first = fixture
        .host
        .route_virtual("mock", "auto", payload("user", None))
        .expect("route");
    assert_eq!(first["model"]["id"], "small");
    assert_eq!(first["model"]["provider"], "mock");
    assert_eq!(first["thinkingLevel"], "medium");
    assert_eq!(
        first["state"],
        json!({ "phase": "user", "missing": true, "previous": null, "failed": null })
    );

    let second = fixture
        .host
        .route_virtual(
            "mock",
            "auto",
            payload("continuation", Some(json!({ "phase": "build" }))),
        )
        .expect("route");
    assert_eq!(second["model"]["id"], "large");
    assert_eq!(second["thinkingLevel"], "high");
}

#[test]
fn routes_are_scoped_to_registered_models_and_unregister_removes_them() {
    let fixture = Fixture::new(ROUTER, vec![model("small"), model("large")]);
    let error = fixture
        .host
        .route_virtual("mock", "missing", payload("user", None))
        .expect_err("unregistered model");
    assert!(error.to_string().contains("not registered"), "{error}");

    fixture
        .host
        .run_command("plugin_router_off", json!([]))
        .expect("command");
    assert!(fixture.host.virtual_models().is_empty());
    assert!(
        fixture
            .host
            .route_virtual("mock", "auto", payload("user", None))
            .is_err(),
        "unregistered route must not run"
    );
}

#[test]
fn a_route_exception_surfaces_as_an_error() {
    let source = r#"
module.exports = function (pi) {
  pi.registerVirtualModel({
    provider: "mock",
    id: "auto",
    name: "Auto",
    route() {
      throw new Error("router exploded");
    },
  });
};
"#;
    let fixture = Fixture::new(source, vec![model("small")]);
    let error = fixture
        .host
        .route_virtual("mock", "auto", payload("user", None))
        .expect_err("route throws");
    assert!(error.to_string().contains("router exploded"), "{error}");
    assert_eq!(error.code(), Some(ErrorCode::E303));
}

#[test]
fn registering_an_id_owned_by_a_physical_model_fails_the_load() {
    let source = r#"
module.exports = function (pi) {
  pi.registerVirtualModel({
    provider: "mock", id: "small", name: "Shadow",
    route() { return { model: null, thinkingLevel: "off" }; },
  });
};
"#;
    let error = Fixture::try_new(source, vec![model("small")])
        .err()
        .expect("conflict");
    assert!(error.contains("conflicts with a physical model"), "{error}");
    assert!(error.contains("E202"), "{error}");
}

#[tokio::test]
async fn the_loop_bridge_maps_requests_and_choices() {
    let fixture = Fixture::new(ROUTER, vec![model("small"), model("large")]);
    let hooks = ExtHooks::new(Arc::clone(&fixture.host));
    let selection = Model::virtual_entry("mock", "auto", "Auto");
    let physical = Model::descriptor("mock", "small", timo_ai::ApiKind::AnthropicMessages);
    let messages = vec![timo_ai::user_text("hi", 0)];

    let choice = hooks
        .route_model(RouteRequest {
            model: &selection,
            thinking: ThinkingLevel::Low,
            reason: RouteReason::Continuation,
            previous: Some((&physical, ThinkingLevel::Medium)),
            failed: Some((&physical, ThinkingLevel::Low)),
            state: Some(&json!({ "phase": "build" })),
            messages: &messages,
        })
        .await
        .expect("route")
        .expect("registered");
    assert_eq!(choice.model.id, "large");
    assert_eq!(choice.thinking, ThinkingLevel::High);
    assert_eq!(
        choice.state,
        Some(json!({
            "phase": "continuation",
            "missing": true,
            "previous": "small",
            "failed": "small",
        }))
    );

    // 物理选中不参与路由（ExtHooks 直接放行）。
    let routed = hooks
        .route_model(RouteRequest {
            model: &physical,
            thinking: ThinkingLevel::Low,
            reason: RouteReason::User,
            previous: None,
            failed: None,
            state: None,
            messages: &messages,
        })
        .await
        .expect("no route");
    assert!(routed.is_none());
}
