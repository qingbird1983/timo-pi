//! 发现与 manifest 集成测试（设计 02 §2/§3.1）。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::Path;

use timo_ext::{DiscoveryOptions, ErrorCode, Scope, Status, discover};

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn options(work: &Path, data: &Path) -> DiscoveryOptions {
    DiscoveryOptions {
        work_dir: work.to_path_buf(),
        data_dir: Some(data.to_path_buf()),
        explicit: Vec::new(),
        include_project_pi: true,
    }
}

fn valid_manifest(id: &str) -> String {
    format!(
        r#"{{
            "schema": 1,
            "id": "{id}",
            "entry": "source/index.ts",
            "bundle": "bundle.mjs",
            "source_hash": "sha256:{}",
            "bundler": "rolldown-test"
        }}"#,
        "a".repeat(64)
    )
}

#[test]
fn project_scope_overrides_user_scope() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("work");
    let data = temp.path().join("data");
    write(
        &work.join(".timo/extensions/demo/index.ts"),
        "export default () => {};",
    );
    write(
        &data.join("extensions/demo/index.ts"),
        "export default () => {};",
    );

    let outcome = discover(&options(&work, &data));
    assert_eq!(outcome.extensions.len(), 1);
    let demo = &outcome.extensions[0];
    assert_eq!(demo.id, "demo");
    assert_eq!(demo.scope, Scope::Project);
    assert!(demo.entry.starts_with(work.join(".timo/extensions/demo")));
    assert!(
        outcome
            .issues
            .iter()
            .any(|issue| issue.code == ErrorCode::E204)
    );
}

#[test]
fn discovers_all_entry_forms_and_skips_noise() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("work");
    let data = temp.path().join("data");
    let root = work.join(".timo/extensions");
    write(&root.join("a.ts"), "export default () => {};");
    write(&root.join("b/index.ts"), "export default () => {};");
    write(
        &root.join("c/package.json"),
        r#"{"pi":{"extensions":["./main.ts"]}}"#,
    );
    write(&root.join("c/main.ts"), "export default () => {};");
    write(&root.join("d/d.ts"), "export default () => {};");
    fs::create_dir_all(root.join("broken")).unwrap();
    write(&root.join(".hidden.ts"), "export default () => {};");
    write(
        &root.join("node_modules/skip.ts"),
        "export default () => {};",
    );

    let outcome = discover(&options(&work, &data));
    let ids: Vec<_> = outcome
        .extensions
        .iter()
        .map(|ext| ext.id.as_str())
        .collect();
    assert_eq!(ids, vec!["a", "b", "c", "d"]);
    assert!(
        outcome
            .issues
            .iter()
            .any(|issue| issue.code == ErrorCode::E101 && issue.path.ends_with("broken"))
    );
    assert!(!ids.contains(&"hidden"));
    assert!(!ids.contains(&"skip"));
}

#[test]
fn manifest_id_wins_and_invalid_manifest_is_reported() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("work");
    let data = temp.path().join("data");
    let root = work.join(".timo/extensions");
    write(&root.join("orig/index.ts"), "export default () => {};");
    write(&root.join("orig/manifest.json"), &valid_manifest("renamed"));
    write(&root.join("bad/index.ts"), "export default () => {};");
    write(&root.join("bad/manifest.json"), r#"{"schema":2}"#);

    let outcome = discover(&options(&work, &data));
    assert_eq!(outcome.extensions.len(), 1);
    let ext = &outcome.extensions[0];
    assert_eq!(ext.id, "renamed");
    assert!(ext.manifest.is_some());
    assert!(
        outcome
            .issues
            .iter()
            .any(|issue| issue.code == ErrorCode::E205 && issue.path.ends_with("bad"))
    );
}

#[test]
fn explicit_and_project_pi_paths_respect_scope() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("work");
    let data = temp.path().join("data");
    let explicit = temp.path().join("x.ts");
    write(&explicit, "export default () => {};");
    write(
        &work.join(".pi/extensions/pi-only/index.ts"),
        "export default () => {};",
    );

    let with_pi = DiscoveryOptions {
        explicit: vec![explicit.clone()],
        ..options(&work, &data)
    };
    let outcome = discover(&with_pi);
    let ids: Vec<_> = outcome
        .extensions
        .iter()
        .map(|ext| ext.id.as_str())
        .collect();
    assert_eq!(ids, vec!["pi-only", "x"]);
    assert_eq!(
        outcome
            .extensions
            .iter()
            .find(|ext| ext.id == "x")
            .map(|ext| ext.scope),
        Some(Scope::Explicit)
    );

    let without_pi = DiscoveryOptions {
        include_project_pi: false,
        ..with_pi
    };
    let outcome = discover(&without_pi);
    let ids: Vec<_> = outcome
        .extensions
        .iter()
        .map(|ext| ext.id.as_str())
        .collect();
    assert_eq!(ids, vec!["x"]);
}

#[test]
fn diagnostics_report_can_be_built_from_discovery() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("work");
    let data = temp.path().join("data");
    write(
        &work.join(".timo/extensions/ok/index.ts"),
        "export default () => {};",
    );

    let outcome = discover(&options(&work, &data));
    let mut report = timo_ext::DiagnosticReport::new("2026-10-26T00:00:00Z", "0.1.0");
    for ext in &outcome.extensions {
        report.push(timo_ext::ExtensionDiagnostic::new(
            &ext.id,
            ext.root.display().to_string(),
            Status::Loaded,
        ));
    }
    let json = report.to_json_pretty().unwrap();
    assert!(json.contains("\"loaded\": 1"));
    assert!(report.render_human().contains("ok"));
}
