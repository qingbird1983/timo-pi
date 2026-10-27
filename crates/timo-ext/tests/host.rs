//! QuickJS 宿主契约（设计 01 §3、04 §2）：装载、工具调用、事件派发与预算。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use timo_ext::{BasicServices, ErrorCode, ExtensionHost, Grants, HostServices, LoadedExtension};

fn services() -> Arc<dyn HostServices> {
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Arc::new(BasicServices::new(cwd, Grants::default(), json!({})))
}

struct Fixture {
    _dir: tempfile::TempDir,
    host: ExtensionHost,
    loaded: LoadedExtension,
}

impl Fixture {
    fn new(id: &str, source: &str) -> Self {
        Self::with_budget(id, source, Duration::from_secs(30))
    }

    fn with_budget(id: &str, source: &str, budget: Duration) -> Self {
        Self::with_services(id, source, services(), budget)
    }

    fn with_services(
        id: &str,
        source: &str,
        services: Arc<dyn HostServices>,
        budget: Duration,
    ) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bundle.cjs");
        std::fs::write(&path, source).unwrap();
        let host = ExtensionHost::start(services, budget).unwrap();
        let loaded = host.load(id, &path).unwrap();
        assert_eq!(loaded.id, id);
        Self {
            _dir: dir,
            host,
            loaded,
        }
    }
}

const HELLO: &str = r#"
module.exports = function (pi) {
  pi.registerTool({
    name: "hello",
    label: "Hello",
    description: "Say hello",
    parameters: { type: "object" },
    execute: async (toolCallId, args) => ({
      content: [{ type: "text", text: "hello " + (args.name || "world") }],
      details: { call: toolCallId },
    }),
  });
  pi.registerCommand("demo", { description: "demo command", handler: async () => {} });
  pi.on("tool_call", async (event) => {
    if (event.input && event.input.blocked) {
      return { block: true, reason: "blocked by test" };
    }
    return { input: { ...event.input, seen: true } };
  });
  pi.on("before_agent_start", async () => ({ systemPrompt: "patched" }));
  pi.on("tool_result", async () => ({
    content: [{ type: "text", text: "patched result" }],
  }));
};
"#;

/// 工具回报"内部还有一次模型调用"的用量（设计 08 §4.3）。
/// 独立 bundle：不扩 HELLO，免得动它既有的一条注册数断言。
const USAGE_RESULT: &str = r#"
module.exports = function (pi) {
  pi.registerTool({
    name: "nested",
    label: "Nested",
    description: "Reports the usage of an inner model call",
    parameters: { type: "object" },
    execute: async () => ({
      content: [{ type: "text", text: "nested done" }],
      details: {},
      usage: { input: 4321, output: 128, cache_read: 900, total_tokens: 5349, cost: { total: 0.0107 } },
    }),
  });
};
"#;

#[test]
fn load_registers_prefixed_tools_commands_and_hooks() {
    let fixture = Fixture::new("demo", HELLO);
    let tools = fixture.host.tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "plugin_demo_hello");
    assert_eq!(tools[0].js_name, "hello");
    assert_eq!(tools[0].ext_id, "demo");
    assert_eq!(tools[0].description, "Say hello");
    assert_eq!(fixture.host.commands(), vec!["plugin_demo_demo"]);

    let mut hooks = fixture.loaded.hooks.clone();
    hooks.sort_unstable();
    assert_eq!(
        hooks,
        vec!["before_agent_start", "tool_call", "tool_result"]
    );
}

#[test]
fn call_tool_returns_result() {
    let fixture = Fixture::new("demo", HELLO);
    let result = fixture
        .host
        .call_tool("plugin_demo_hello", json!({ "name": "qoder" }))
        .unwrap();
    assert_eq!(result.content[0]["text"], "hello qoder");
    assert_eq!(result.details["call"], "toolcall");
    assert!(!result.is_error);
}

#[test]
fn tool_usage_crosses_the_json_boundary() {
    let fixture = Fixture::new("demo", USAGE_RESULT);
    let usage = fixture
        .host
        .call_tool("plugin_demo_nested", json!({}))
        .unwrap()
        .usage
        .expect("usage forwarded");
    assert_eq!(usage.input, 4_321);
    assert_eq!(usage.cache_read, 900);
    assert_eq!(usage.total_tokens, 5_349);
    assert!(
        (usage.cost.total - 0.0107).abs() < 1e-12,
        "cost.total must survive: {usage:?}"
    );

    let plain = Fixture::new("demo", HELLO);
    let none = plain
        .host
        .call_tool("plugin_demo_hello", json!({ "name": "x" }))
        .unwrap();
    assert!(none.usage.is_none(), "不带 usage 时不得凭空造用量");
}

#[test]
fn tool_call_handler_can_merge_input_and_block() {
    let fixture = Fixture::new("demo", HELLO);
    let merged = fixture
        .host
        .dispatch(
            "tool_call",
            json!({ "toolName": "read", "input": { "path": "a" } }),
        )
        .unwrap();
    assert_eq!(merged["input"]["path"], "a");
    assert_eq!(merged["input"]["seen"], true);

    let blocked = fixture
        .host
        .dispatch(
            "tool_call",
            json!({ "toolName": "read", "input": { "blocked": true } }),
        )
        .unwrap();
    assert_eq!(blocked["block"], true);
    assert_eq!(blocked["reason"], "blocked by test");
}

#[test]
fn event_merges_follow_pi_semantics() {
    let fixture = Fixture::new("demo", HELLO);
    let start = fixture
        .host
        .dispatch("before_agent_start", json!({ "prompt": "hi" }))
        .unwrap();
    assert_eq!(start["systemPrompt"], "patched");

    let result = fixture
        .host
        .dispatch(
            "tool_result",
            json!({ "content": [{ "type": "text", "text": "raw" }] }),
        )
        .unwrap();
    assert_eq!(result["content"][0]["text"], "patched result");

    let none = fixture
        .host
        .dispatch("agent_end", json!({ "ok": true }))
        .unwrap();
    assert!(none.is_null());
}

#[test]
fn handler_exceeding_budget_reports_e301() {
    let source = r#"
module.exports = function (pi) {
  pi.on("tool_call", () => new Promise(() => {}));
};
"#;
    let fixture = Fixture::with_budget("slow", source, Duration::from_millis(50));
    let error = fixture
        .host
        .dispatch("tool_call", json!({ "input": {} }))
        .unwrap_err();
    assert_eq!(error.code(), Some(ErrorCode::E301));
}

#[test]
fn context_is_reusable_after_budget_interrupt() {
    let source = r#"
let first = true;
module.exports = function (pi) {
  pi.on("tool_call", async () => {
    if (first) {
      first = false;
      while (true) {}
    }
    return { input: { ok: true } };
  });
};
"#;
    let fixture = Fixture::with_budget("spin", source, Duration::from_millis(100));
    let error = fixture
        .host
        .dispatch("tool_call", json!({ "input": {} }))
        .unwrap_err();
    assert_eq!(error.code(), Some(ErrorCode::E301));
    let next = fixture
        .host
        .dispatch("tool_call", json!({ "input": {} }))
        .expect("context stays usable after interrupt");
    assert_eq!(next["input"]["ok"], true);
}

#[test]
fn rejecting_factory_reports_e202() {
    let source = "module.exports = function () { throw new Error('boom'); };";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bundle.cjs");
    std::fs::write(&path, source).unwrap();
    let host = ExtensionHost::start(services(), Duration::from_secs(5)).unwrap();
    let error = host.load("broken", &path).unwrap_err();
    assert_eq!(error.code(), Some(ErrorCode::E202));
    assert!(error.to_string().contains("boom"));
}

#[test]
fn non_function_export_reports_e203() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bundle.cjs");
    std::fs::write(&path, "module.exports = 42;").unwrap();
    let host = ExtensionHost::start(services(), Duration::from_secs(5)).unwrap();
    let error = host.load("bad-shape", &path).unwrap_err();
    assert_eq!(error.code(), Some(ErrorCode::E203));
}

#[test]
fn exec_without_grant_is_rejected() {
    let source = r#"
module.exports = function (pi) {
  pi.registerTool({
    name: "run",
    label: "Run",
    description: "",
    parameters: {},
    execute: async () => {
      const out = await pi.exec("echo", ["hi"]);
      return { content: [{ type: "text", text: out.stdout }] };
    },
  });
};
"#;
    let fixture = Fixture::new("exec-ext", source);
    let error = fixture
        .host
        .call_tool("plugin_exec-ext_run", json!({}))
        .unwrap_err();
    assert_eq!(error.code(), Some(ErrorCode::E303));
    assert!(error.to_string().contains("E401"));
}

#[test]
fn unknown_tool_is_e303() {
    let fixture = Fixture::new("demo", HELLO);
    let error = fixture
        .host
        .call_tool("plugin_demo_missing", json!({}))
        .unwrap_err();
    assert_eq!(error.code(), Some(ErrorCode::E303));
}

#[test]
fn js_config_is_visible_to_handlers() {
    let source = r#"
module.exports = function (pi) {
  pi.on("before_agent_start", async (_event, ctx) => ({
    systemPrompt: ctx.cwd + "|" + ctx.mode + "|" + ctx.isProjectTrusted(),
  }));
};
"#;
    let fixture = Fixture::new("ctx-ext", source);
    let value = fixture
        .host
        .dispatch("before_agent_start", json!({}))
        .unwrap();
    let prompt = value["systemPrompt"].as_str().unwrap();
    assert!(prompt.ends_with("|print|true"), "prompt was {prompt}");
    assert!(!prompt.starts_with('|'));
}

#[test]
fn agent_dir_reaches_the_extension_through_config() {
    let source = r#"
module.exports = function (pi) {
  pi.registerTool({
    name: "where",
    label: "Where",
    description: "reports the agent dir handed over by the host",
    parameters: { type: "object" },
    execute: async () => ({
      content: [{ type: "text", text: pi.getAgentDir() || "none" }],
      details: {},
    }),
  });
};
"#;
    let blank = Fixture::new("where", source);
    let missing = blank
        .host
        .call_tool("plugin_where_where", json!({}))
        .unwrap()
        .content[0]["text"]
        .clone();
    assert_eq!(missing, "none", "未设 agentDir 时不得凭空给路径");

    let dir = tempfile::tempdir().unwrap();
    let services: Arc<dyn HostServices> = Arc::new(
        BasicServices::new(dir.path(), Grants::default(), json!({}))
            .with_agent_dir(dir.path().join("agents-root")),
    );
    let fixture = Fixture::with_services("where", source, services, Duration::from_secs(30));
    let text = fixture
        .host
        .call_tool("plugin_where_where", json!({}))
        .unwrap()
        .content[0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        text.ends_with("agents-root"),
        "agentDir must reach JS: {text}"
    );
}

#[test]
fn diagnostics_from_shim_are_collected() {
    let source = r#"
module.exports = function (pi) {
  pi.registerProvider("x", {});
};
"#;
    let fixture = Fixture::new("diag-ext", source);
    let entries = fixture.host.diagnostics();
    assert!(
        entries.iter().any(
            |entry| entry.code == ErrorCode::E202 && entry.message.contains("registerProvider")
        ),
        "diagnostics were {entries:?}"
    );
}

const FS_SURFACE: &str = r#"
module.exports = function (pi) {
  pi.registerTool({
    name: "readok",
    label: "read ok",
    description: "read granted scope",
    parameters: { type: "object" },
    execute: async () => {
      const text = await pi.readFile("agents/scout.md");
      const items = await pi.listDir("agents");
      const runs = await pi.execParallel([
        { command: __CMD__, args: __ARGS_A__ },
        { command: __CMD__, args: __ARGS_B__ },
      ]);
      return {
        content: [{ type: "text", text: text + "|" + items.map((i) => i.name).join(",") + "|" + runs.map((r) => r.code).join(",") + "|" + runs.map((r) => r.stdout.trim()).join(",") }],
        details: {},
      };
    },
  });
  pi.registerTool({
    name: "readdenied",
    label: "read denied",
    description: "read outside scope",
    parameters: { type: "object" },
    execute: async () => {
      try {
        await pi.readFile("private.txt");
        return { content: [{ type: "text", text: "allowed" }], details: {} };
      } catch (error) {
        return { content: [{ type: "text", text: String(error.code) + "|" + String(error.message).slice(0, 4) }], details: {} };
      }
    },
  });
};
"#;

#[test]
fn file_and_parallel_apis_reach_extensions_with_codes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("agents")).unwrap();
    std::fs::write(dir.path().join("agents/scout.md"), "scout body").unwrap();
    std::fs::write(dir.path().join("private.txt"), "secret").unwrap();
    let shell = if cfg!(windows) { "cmd" } else { "sh" };
    let switch = if cfg!(windows) { "/C" } else { "-c" };
    let source = FS_SURFACE
        .replace("__CMD__", &format!(r#""{shell}""#))
        .replace("__ARGS_A__", &format!(r#"["{switch}", "echo A"]"#))
        .replace("__ARGS_B__", &format!(r#"["{switch}", "echo B"]"#));
    let services: Arc<dyn HostServices> = Arc::new(BasicServices::new(
        dir.path(),
        Grants {
            exec: true,
            net_domains: vec![],
            fs_read: vec!["./agents".to_string()],
        },
        serde_json::Value::Null,
    ));
    let fixture = Fixture::with_services("fs", &source, services, Duration::from_secs(30));
    let ok = fixture
        .host
        .call_tool("plugin_fs_readok", json!({}))
        .unwrap();
    let text = ok.content[0]["text"].as_str().unwrap().to_string();
    assert!(
        text.starts_with("scout body|scout.md|0,0|A,B"),
        "got {text}"
    );
    let denied = fixture
        .host
        .call_tool("plugin_fs_readdenied", json!({}))
        .unwrap();
    let text = denied.content[0]["text"].as_str().unwrap();
    assert_eq!(text, "E403|read", "denial must carry the code: {text}");
    let codes: Vec<ErrorCode> = fixture
        .host
        .diagnostics_for("fs")
        .iter()
        .map(|entry| entry.code)
        .collect();
    assert!(codes.contains(&ErrorCode::E403), "{codes:?}");
}
