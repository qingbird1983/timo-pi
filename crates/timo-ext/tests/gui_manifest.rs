//! B 轨清单（schema 2）的字段、挡位与 API 校验。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use timo_ext::GuiManifest;

fn sample() -> Value {
    json!({
        "schema": 2,
        "id": "session-search",
        "kind": "gui",
        "name": "会话搜索",
        "version": "1.0.0",
        "nsh_id": "com.timo.session-search",
        "entry": "source/index.tsx",
        "bundle": "plugin.nshpkg",
        "source_hash": format!("sha256:{}", "a".repeat(64)),
        "bundler": "timo-devkit/0.1",
        "api": 1,
        "tier": "read-only",
        "slots": [{ "kind": "panel", "title": "会话搜索", "icon": "search", "order": 30 }],
        "queries": ["sessions.list"],
        "actions": ["notify"],
        "capabilities": { "exec": false, "net": { "domains": [] }, "fs": { "read": [], "write": [], "delete": [] } }
    })
}

fn write(dir: &std::path::Path, value: &Value) -> std::path::PathBuf {
    let path = dir.join("manifest.json");
    std::fs::write(&path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    path
}

#[test]
fn accepts_a_full_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &sample());
    let manifest = GuiManifest::load(&path).unwrap().unwrap();
    assert_eq!(manifest.id, "session-search");
    assert_eq!(manifest.slots.len(), 1);
    assert_eq!(manifest.api, Some(1));
    assert_eq!(
        manifest.bundle_path(dir.path()),
        dir.path().join("plugin.nshpkg")
    );
}

#[test]
fn missing_file_is_none() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        GuiManifest::load(&dir.path().join("manifest.json"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn rejects_a_track_schema() {
    let dir = tempfile::tempdir().unwrap();
    let mut value = sample();
    value["schema"] = json!(1);
    let error = GuiManifest::load(&write(dir.path(), &value)).unwrap_err();
    assert_eq!(error.code(), "E720");
    assert!(error.to_string().contains("unsupported schema"), "{error}");
}

#[test]
fn rejects_unknown_slot_kind() {
    let dir = tempfile::tempdir().unwrap();
    let mut value = sample();
    value["slots"] = json!([{ "kind": "webview" }]);
    let error = GuiManifest::load(&write(dir.path(), &value)).unwrap_err();
    assert_eq!(error.code(), "E720");
    assert!(error.to_string().contains("unknown slot kind"), "{error}");
}

#[test]
fn requires_gui_fields() {
    let dir = tempfile::tempdir().unwrap();
    for field in ["nsh_id", "api", "slots"] {
        let mut value = sample();
        value.as_object_mut().unwrap().remove(field);
        let error = GuiManifest::load(&write(dir.path(), &value)).unwrap_err();
        assert_eq!(error.code(), "E720", "{field}: {error}");
    }
}

#[test]
fn rejects_non_package_bundle() {
    let dir = tempfile::tempdir().unwrap();
    let mut value = sample();
    value["bundle"] = json!("plugin.cjs");
    let error = GuiManifest::load(&write(dir.path(), &value)).unwrap_err();
    assert_eq!(error.code(), "E720");
}

#[test]
fn read_only_tier_rejects_exec_and_network() {
    let dir = tempfile::tempdir().unwrap();
    let mut value = sample();
    value["capabilities"]["exec"] = json!(true);
    let error = GuiManifest::load(&write(dir.path(), &value)).unwrap_err();
    assert_eq!(error.code(), "E722");
    assert!(error.to_string().contains("exec"), "{error}");

    let mut value = sample();
    value["capabilities"]["net"]["domains"] = json!(["api.example.com"]);
    let error = GuiManifest::load(&write(dir.path(), &value)).unwrap_err();
    assert_eq!(error.code(), "E722");
}

#[test]
fn workspace_tier_rejects_exec() {
    let dir = tempfile::tempdir().unwrap();
    let mut value = sample();
    value["tier"] = json!("workspace");
    value["capabilities"]["fs"]["write"] = json!(["workspace/**"]);
    let manifest = GuiManifest::load(&write(dir.path(), &value)).unwrap();
    assert!(manifest.is_some());

    value["capabilities"]["exec"] = json!(true);
    let error = GuiManifest::load(&write(dir.path(), &value)).unwrap_err();
    assert_eq!(error.code(), "E722");
}

#[test]
fn trusted_tier_accepts_exec_and_domains() {
    let dir = tempfile::tempdir().unwrap();
    let mut value = sample();
    value["tier"] = json!("trusted");
    value["capabilities"]["exec"] = json!(true);
    value["capabilities"]["net"]["domains"] = json!(["api.example.com"]);
    assert!(
        GuiManifest::load(&write(dir.path(), &value))
            .unwrap()
            .is_some()
    );
}

#[test]
fn rejects_newer_host_api() {
    let dir = tempfile::tempdir().unwrap();
    let mut value = sample();
    value["api"] = json!(2);
    let error = GuiManifest::load(&write(dir.path(), &value)).unwrap_err();
    assert_eq!(error.code(), "E721");
}

#[test]
fn rejects_bad_version_and_hash() {
    let dir = tempfile::tempdir().unwrap();
    let mut value = sample();
    value["version"] = json!("1.0");
    assert_eq!(
        GuiManifest::load(&write(dir.path(), &value))
            .unwrap_err()
            .code(),
        "E720"
    );

    let mut value = sample();
    value["source_hash"] = json!("sha256:xyz");
    assert_eq!(
        GuiManifest::load(&write(dir.path(), &value))
            .unwrap_err()
            .code(),
        "E720"
    );
}

#[test]
fn nsh_id_rules() {
    assert!(timo_ext::is_valid_nsh_id("com.timo.session-search"));
    assert!(!timo_ext::is_valid_nsh_id(""));
    assert!(!timo_ext::is_valid_nsh_id(".timo"));
    assert!(!timo_ext::is_valid_nsh_id("com timo"));
}
