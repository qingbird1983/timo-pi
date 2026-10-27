//! 扩展装载集成测试（设计 02 §3、03 §4）：发现、manifest、宿主与报告。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use serde_json::json;
use timo_ext::{ErrorCode, compute_source_hash};
use timo_kernel::{ExtensionOptions, load_extensions};

const BUNDLE: &str = r#"
module.exports = function (pi) {
  pi.registerTool({
    name: "hello",
    label: "Hello",
    description: "test tool",
    parameters: {},
    execute: async () => ({ content: [{ type: "text", text: "ok" }], details: {} }),
  });
  pi.registerCommand("ping", { handler: async () => {} });
  pi.on("tool_call", async () => undefined);
};
"#;

fn install(dir: &Path, id: &str, extra: serde_json::Value) {
    let root = dir.join("extensions").join(id);
    let source = root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("index.ts"), "export default function () {}\n").unwrap();
    std::fs::write(root.join("bundle.cjs"), BUNDLE).unwrap();
    let hash = compute_source_hash(&root).unwrap();
    let mut manifest = json!({
        "schema": 1,
        "id": id,
        "entry": "source/index.ts",
        "bundle": "bundle.cjs",
        "source_hash": hash,
        "bundler": "test",
        "created_at": "2026-10-26T12:00:00Z",
        "capabilities": { "exec": false, "net": { "domains": [] }, "fs": { "read": [], "write": [], "delete": [] } },
    });
    if let Some(object) = extra.as_object() {
        for (key, value) in object {
            manifest[key] = value.clone();
        }
    }
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

fn options(dir: &Path) -> ExtensionOptions {
    ExtensionOptions {
        data_dir: dir.to_path_buf(),
        work_dir: dir.to_path_buf(),
        explicit: vec![],
        interactive: false,
        include_project_pi: false,
        abort: Arc::new(AtomicBool::new(false)),
        running: Arc::new(AtomicBool::new(false)),
        flags: Default::default(),
        models: Vec::new(),
    }
}

#[tokio::test]
async fn loads_installed_extension_with_tools_commands_and_hooks() {
    let dir = tempfile::tempdir().unwrap();
    install(dir.path(), "hello", json!({}));
    let runtime = load_extensions(options(dir.path())).await;
    assert_eq!(runtime.report.summary().loaded, 1);
    assert_eq!(runtime.report.summary().failed, 0);
    let tools = runtime.host.as_ref().unwrap().tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "plugin_hello_hello");
    assert_eq!(runtime.commands, vec!["plugin_hello_ping"]);
    runtime
        .host
        .as_ref()
        .unwrap()
        .run_command("plugin_hello_ping", json!([]))
        .unwrap();
}

#[tokio::test]
async fn whole_tree_write_is_reported_as_e403() {
    let dir = tempfile::tempdir().unwrap();
    install(
        dir.path(),
        "wide",
        json!({ "capabilities": {
            "exec": false,
            "net": { "domains": [] },
            "fs": { "read": [], "write": ["**"], "delete": [] }
        }}),
    );
    let runtime = load_extensions(options(dir.path())).await;
    let entry = runtime
        .report
        .extensions
        .iter()
        .find(|ext| ext.id == "wide")
        .expect("wide entry");
    assert!(
        entry
            .errors
            .iter()
            .any(|error| error.code == ErrorCode::E403)
    );
    assert_eq!(runtime.report.summary().failed, 1);
}
