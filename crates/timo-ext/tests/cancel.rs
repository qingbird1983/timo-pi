//! 扩展工具取消（接缝 #7）：per-call 标志由 QuickJS 中断处理器收敛。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use serde_json::json;
use timo_ext::{BasicServices, ErrorCode, ExtensionHost, Grants, HostServices};

const SPIN: &str = r#"
module.exports = function (pi) {
  pi.registerTool({
    name: "spin",
    label: "Spin",
    description: "",
    parameters: {},
    execute: async () => { while (true) {} },
  });
};
"#;

fn start_host(dir: &Path, id: &str) -> ExtensionHost {
    let path = dir.join("bundle.cjs");
    std::fs::write(&path, SPIN).unwrap();
    let services: Arc<dyn HostServices> = Arc::new(BasicServices::new(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        Grants::default(),
        json!({}),
    ));
    let host = ExtensionHost::start(services, Duration::from_secs(30)).unwrap();
    host.load(id, &path).unwrap();
    host
}

#[test]
fn cancelled_tool_call_stops_before_budget() {
    let dir = tempfile::tempdir().unwrap();
    let host = start_host(dir.path(), "cancel-ext");
    let flag = Arc::new(AtomicBool::new(true));
    let started = Instant::now();
    let error = host
        .call_tool_cancellable("plugin_cancel-ext_spin", json!({}), Some(flag))
        .unwrap_err();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "cancel should not wait for the 30s budget"
    );
    assert_eq!(error.code(), Some(ErrorCode::E301));
}
