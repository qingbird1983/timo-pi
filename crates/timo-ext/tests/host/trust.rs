//! `project_trust` 事件与 `ctx.isProjectTrusted()` 的宿主契约（设计 12 §8 批 B）。

use super::*;

/// 两个扩展依次回答 `project_trust`：第一个给出 no 的拥有决定。
#[test]
fn project_trust_first_decision_wins() {
    let dir = tempfile::tempdir().unwrap();
    let host = ExtensionHost::start(services(), Duration::from_secs(10)).unwrap();
    let no = dir.path().join("no.cjs");
    std::fs::write(
        &no,
        r#"module.exports = function (pi) {
  pi.on("project_trust", async () => ({ trusted: "no", remember: true }));
};"#,
    )
    .unwrap();
    let yes = dir.path().join("yes.cjs");
    std::fs::write(
        &yes,
        r#"module.exports = function (pi) {
  pi.on("project_trust", async () => ({ trusted: "yes" }));
};"#,
    )
    .unwrap();
    host.load("no", &no).unwrap();
    host.load("yes", &yes).unwrap();
    let value = host
        .dispatch("project_trust", json!({ "cwd": "/tmp" }))
        .unwrap();
    assert_eq!(value["trusted"], "no");
    assert_eq!(value["remember"], true);
}

/// `undecided` 不拥有决定，继续问下一个。
#[test]
fn project_trust_skips_undecided() {
    let dir = tempfile::tempdir().unwrap();
    let host = ExtensionHost::start(services(), Duration::from_secs(10)).unwrap();
    let undecided = dir.path().join("u.cjs");
    std::fs::write(
        &undecided,
        r#"module.exports = function (pi) {
  pi.on("project_trust", async () => ({ trusted: "undecided" }));
};"#,
    )
    .unwrap();
    let yes = dir.path().join("y.cjs");
    std::fs::write(
        &yes,
        r#"module.exports = function (pi) {
  pi.on("project_trust", async () => ({ trusted: "yes", remember: false }));
};"#,
    )
    .unwrap();
    host.load("u", &undecided).unwrap();
    host.load("y", &yes).unwrap();
    let value = host
        .dispatch("project_trust", json!({ "cwd": "/tmp" }))
        .unwrap();
    assert_eq!(value["trusted"], "yes");
}

/// `ctx.isProjectTrusted()` 读共享槽，装载后翻转也要立即生效。
#[test]
fn is_project_trusted_reads_shared_flag() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let flag = Arc::new(AtomicBool::new(false));
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let services: Arc<dyn HostServices> = Arc::new(
        BasicServices::new(cwd, Grants::default(), json!({}))
            .with_project_trusted(Arc::clone(&flag)),
    );
    let source = r#"
module.exports = function (pi) {
  pi.on("project_trust", async (event, ctx) => ({
    trusted: ctx.isProjectTrusted() ? "yes" : "undecided",
  }));
};
"#;
    let fixture = Fixture::with_services("trustor", source, services, Duration::from_secs(10));
    let ask = || {
        fixture
            .host
            .dispatch("project_trust", json!({ "cwd": "/tmp" }))
            .unwrap()
    };
    assert!(ask().is_null());
    flag.store(true, Ordering::Relaxed);
    assert_eq!(ask()["trusted"], "yes");
}
