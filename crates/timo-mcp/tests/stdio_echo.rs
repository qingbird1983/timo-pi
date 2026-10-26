//! A4：本地 stdio MCP server 集成测试（设计 07 §7）——工具发现、命名空间、
//! 统一管线调用。起 Python echo server 子进程；不触网。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mcp")
}

#[tokio::test]
async fn stdio_echo_server_roundtrip() {
    let dir = std::env::temp_dir().join(format!("timo-mcp-echo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("mcp.json");
    let script = fixtures().join("echo_server.py");
    let json = serde_json::json!({
        "servers": {
            "echo": {"command": "python", "args": [script.display().to_string()]}
        }
    });
    std::fs::write(&config, serde_json::to_string(&json).unwrap()).unwrap();

    let entries = timo_mcp::load_server_registry(&config, &dir.join("project.json")).unwrap();
    let runtime = timo_mcp::McpRuntime::new(entries).shared();
    assert_eq!(runtime.enabled_servers(), vec!["echo"]);

    // 首次枚举才连接（惰性）
    let defs = runtime
        .definitions("echo")
        .await
        .expect("connect + list_tools");
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].name, "mcp__echo__echo");
    assert_eq!(defs[0].parameters["properties"]["text"]["type"], "string");

    // 走统一注册表与统一管线（接缝 #4）
    let mut registry = timo_tools::ToolRegistry::new();
    registry.register_all(defs);
    let (updates, _rx) = tokio::sync::mpsc::channel(4);
    let ctx = timo_tools::ToolCallCtx {
        tool_call_id: "t1".into(),
        params: serde_json::json!({"text": "hi"}),
        cancel: tokio_util::sync::CancellationToken::new(),
        updates,
        cwd: Arc::from(dir.as_path()),
    };
    let out = registry
        .execute("mcp__echo__echo", ctx)
        .await
        .expect("call echo");
    assert_eq!(
        out.content[0],
        timo_ai::Content::Text {
            text: "echo: hi".into()
        }
    );
    assert!(!out.is_error);
}
