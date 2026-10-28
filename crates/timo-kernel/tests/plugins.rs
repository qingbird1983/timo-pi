//! 统一插件清单：两轨扫描、启停、挡位收紧与卸载。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use serde_json::json;
use timo_kernel::plugins::PluginOrigin;
use timo_kernel::plugins::{
    PermissionTier, PluginKind, PluginState, PluginsState, install_plugin, list_plugins,
    revoke_plugin, set_enabled, set_tier, uninstall_plugin,
};

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn a_extension(data_dir: &Path, id: &str, capabilities: serde_json::Value) {
    let root = data_dir.join("extensions").join(id);
    write(&root.join("source/index.ts"), "export default () => {};\n");
    write(&root.join("bundle.cjs"), "module.exports = {};\n");
    let manifest = json!({
        "schema": 1,
        "id": id,
        "entry": "source/index.ts",
        "bundle": "bundle.cjs",
        "source_hash": format!("sha256:{}", "a".repeat(64)),
        "bundler": "test",
        "capabilities": capabilities
    });
    write(
        &root.join("manifest.json"),
        &serde_json::to_string_pretty(&manifest).unwrap(),
    );
}

fn b_plugin(data_dir: &Path, id: &str, name: &str, tier: &str) {
    let root = data_dir.join("plugins").join(id);
    write(&root.join("plugin.nshpkg"), "package-bytes");
    let manifest = json!({
        "schema": 2,
        "id": id,
        "kind": "gui",
        "name": name,
        "version": "1.0.0",
        "nsh_id": format!("com.timo.{id}"),
        "entry": "source/index.tsx",
        "bundle": "plugin.nshpkg",
        "source_hash": format!("sha256:{}", "b".repeat(64)),
        "bundler": "timo-devkit/0.1",
        "api": 1,
        "tier": tier,
        "slots": [{ "kind": "panel" }],
        "capabilities": { "exec": false, "net": { "domains": [] }, "fs": { "read": [], "write": [], "delete": [] } }
    });
    write(
        &root.join("manifest.json"),
        &serde_json::to_string_pretty(&manifest).unwrap(),
    );
}

#[test]
fn lists_both_tracks_and_infers_tiers() {
    let dir = tempfile::tempdir().unwrap();
    a_extension(
        dir.path(),
        "demo",
        json!({ "fs": { "write": ["workspace/**"] } }),
    );
    b_plugin(dir.path(), "session-search", "Zeta Search", "read-only");

    let entries = list_plugins(dir.path(), dir.path());
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].id, "demo");
    assert_eq!(entries[0].kind, PluginKind::Orchestration);
    assert_eq!(entries[0].origin, PluginOrigin::Installed);
    assert_eq!(entries[0].tier, PermissionTier::Workspace);
    assert_eq!(entries[0].state, PluginState::Enabled);

    assert_eq!(entries[1].id, "session-search");
    assert_eq!(entries[1].kind, PluginKind::Gui);
    assert_eq!(entries[1].name, "Zeta Search");
    assert_eq!(entries[1].tier, PermissionTier::ReadOnly);
    assert_eq!(entries[1].declared.slots.len(), 1);
}

#[test]
fn disable_persists_and_survives_reload() {
    let dir = tempfile::tempdir().unwrap();
    a_extension(dir.path(), "demo", json!({}));
    set_enabled(dir.path(), "demo", false).unwrap();

    let entries = list_plugins(dir.path(), dir.path());
    assert_eq!(entries[0].state, PluginState::Disabled);
    assert!(!entries[0].enabled);
    let state = PluginsState::load(dir.path());
    assert_eq!(
        state.get("demo").unwrap().tier,
        Some(PermissionTier::ReadOnly)
    );
}

#[test]
fn tier_can_only_be_tightened() {
    let dir = tempfile::tempdir().unwrap();
    b_plugin(dir.path(), "session-search", "Search", "trusted");
    set_tier(dir.path(), "session-search", PermissionTier::Workspace).unwrap();
    assert_eq!(
        list_plugins(dir.path(), dir.path())[0].tier,
        PermissionTier::Workspace
    );
    let error = set_tier(dir.path(), "session-search", PermissionTier::Trusted).unwrap_err();
    assert_eq!(error.code(), "E722");
}

#[test]
fn revoke_removes_grant_and_disables() {
    let dir = tempfile::tempdir().unwrap();
    a_extension(dir.path(), "demo", json!({}));
    let grant = json!({
        "schema": 1,
        "grants": {
            "demo": {
                "sourceHash": format!("sha256:{}", "a".repeat(64)),
                "exec": false,
                "net": [],
                "fs": { "read": [], "write": [], "delete": [] },
                "grantedAt": "unix:1"
            }
        }
    });
    write(
        &dir.path().join("permissions.json"),
        &serde_json::to_string_pretty(&grant).unwrap(),
    );

    revoke_plugin(dir.path(), dir.path(), "demo").unwrap();
    let entries = list_plugins(dir.path(), dir.path());
    assert_eq!(entries[0].state, PluginState::Disabled);
    let store = timo_kernel::PermissionStore::load(&dir.path().join("permissions.json")).unwrap();
    assert!(!store.grants.contains_key("demo"));
}

#[test]
fn uninstall_removes_directory_state_and_grant() {
    let dir = tempfile::tempdir().unwrap();
    a_extension(dir.path(), "demo", json!({}));
    set_enabled(dir.path(), "demo", true).unwrap();
    uninstall_plugin(dir.path(), dir.path(), "demo").unwrap();

    assert!(!dir.path().join("extensions/demo").exists());
    assert!(list_plugins(dir.path(), dir.path()).is_empty());
    assert!(PluginsState::load(dir.path()).get("demo").is_none());
}

#[test]
fn uninstall_gui_plugin_removes_its_directory() {
    let dir = tempfile::tempdir().unwrap();
    b_plugin(dir.path(), "session-search", "Search", "read-only");
    uninstall_plugin(dir.path(), dir.path(), "session-search").unwrap();
    assert!(!dir.path().join("plugins/session-search").exists());
}

#[tokio::test]
async fn install_a_track_grants_the_chosen_tier() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("demo.ts");
    write(&source, "export default () => {};\n");

    let installed = install_plugin(dir.path(), &source, PermissionTier::ReadOnly)
        .await
        .unwrap();
    assert_eq!(installed.id, "demo");
    assert_eq!(installed.tier, PermissionTier::ReadOnly);

    let manifest = timo_kernel::plugins::PluginsState::load(dir.path());
    assert_eq!(
        manifest.get("demo").unwrap().tier,
        Some(PermissionTier::ReadOnly)
    );
    let entries = list_plugins(dir.path(), dir.path());
    assert_eq!(entries[0].id, "demo");
    assert_eq!(entries[0].state, PluginState::Enabled);
    let grants = timo_kernel::PermissionStore::load(&dir.path().join("permissions.json")).unwrap();
    assert!(grants.grants.contains_key("demo"));
}

#[tokio::test]
async fn install_rejects_tier_above_declared_and_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("demo.ts");
    write(&source, "export default () => {};\n");
    let error = install_plugin(dir.path(), &source, PermissionTier::Trusted)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "E722");
    assert!(!dir.path().join("extensions/demo").exists());
}

#[tokio::test]
async fn install_rejects_b_track_packages_for_now() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("demo.nshpkg");
    write(&source, "package-bytes");
    let error = install_plugin(dir.path(), &source, PermissionTier::ReadOnly)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "E710");
}
