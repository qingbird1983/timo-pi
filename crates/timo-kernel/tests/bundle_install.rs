//! B 轨签名包安装账本：验签回执落盘、回滚与两层撤销（设计 02 §4/§5/§7）。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use serde_json::json;
use sha2::{Digest, Sha256};
use timo_ext::GuiManifest;
use timo_kernel::plugins::PermissionTier;
use timo_kernel::plugins::nsh::{PackageReceipt, install_bundle, plugin_dir};
use timo_kernel::plugins::{
    PluginState, PluginsState, list_plugins, revoke_plugin, set_enabled, uninstall_plugin,
};

const BYTES: &[u8] = b"signed-package-bytes";

fn digest(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn manifest(dir: &Path, id: &str, version: &str, tier: &str) -> GuiManifest {
    let value = json!({
        "schema": 2,
        "id": id,
        "kind": "gui",
        "name": id,
        "version": version,
        "nsh_id": format!("com.timo.{id}"),
        "entry": "source/index.tsx",
        "bundle": "plugin.nshpkg",
        "source_hash": format!("sha256:{}", "b".repeat(64)),
        "bundler": "timo-devkit/0.1",
        "api": 1,
        "tier": tier,
        "slots": [{ "kind": "panel" }],
        "capabilities": {
            "exec": false,
            "net": { "domains": [] },
            "fs": { "read": ["docs/**"], "write": [], "delete": [] }
        }
    });
    let path = dir.join("incoming-manifest.json");
    std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    GuiManifest::load(&path).unwrap().unwrap()
}

fn receipt(item: &GuiManifest) -> PackageReceipt {
    PackageReceipt {
        app_id: item.nsh_id.clone().unwrap_or_default(),
        version: item.version.clone(),
        build: 7,
        sha256: digest(BYTES),
    }
}

#[test]
fn installs_a_verified_package_and_records_grant_and_state() {
    let dir = tempfile::tempdir().unwrap();
    let item = manifest(dir.path(), "session-search", "1.0.0", "read-only");
    let installed = install_bundle(
        dir.path(),
        &item,
        BYTES,
        &receipt(&item),
        PermissionTier::ReadOnly,
    )
    .unwrap();
    assert_eq!(installed.id, "session-search");
    assert_eq!(installed.version, "1.0.0");
    let root = plugin_dir(dir.path(), "session-search");
    assert_eq!(std::fs::read(root.join("plugin.nshpkg")).unwrap(), BYTES);
    assert!(root.join("manifest.json").is_file());
    let state = PluginsState::load(dir.path());
    let entry = state.get("session-search").unwrap();
    assert!(entry.enabled);
    assert_eq!(
        entry.package_sha256.as_deref(),
        Some(digest(BYTES).as_str())
    );
    let grant_path = dir.path().join("permissions.json");
    let store: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(grant_path).unwrap()).unwrap();
    assert_eq!(
        store["grants"]["session-search"]["sourceHash"],
        item.source_hash
    );
    let listed = list_plugins(dir.path(), dir.path());
    let row = listed
        .iter()
        .find(|row| row.id == "session-search")
        .unwrap();
    assert_eq!(row.version, "1.0.0");
    assert!(row.diagnostics.is_empty());
}

#[test]
fn refuses_a_package_whose_bytes_do_not_match_the_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let item = manifest(dir.path(), "board", "1.0.0", "read-only");
    let error = install_bundle(
        dir.path(),
        &item,
        b"tampered-one-byte",
        &receipt(&item),
        PermissionTier::ReadOnly,
    )
    .unwrap_err();
    assert_eq!(error.code(), "E711");
    assert!(!plugin_dir(dir.path(), "board").exists());
    assert!(!dir.path().join("permissions.json").exists());
}

#[test]
fn refuses_when_the_manifest_disagrees_with_the_package() {
    let dir = tempfile::tempdir().unwrap();
    let package = manifest(dir.path(), "board", "1.0.0", "read-only");
    let mut item = package.clone();
    item.nsh_id = Some("com.timo.other".into());
    let error = install_bundle(
        dir.path(),
        &item,
        BYTES,
        &receipt(&package),
        PermissionTier::ReadOnly,
    )
    .unwrap_err();
    assert_eq!(error.code(), "E713");
    assert!(!plugin_dir(dir.path(), "board").exists());
}

#[test]
fn refuses_a_downgrade_and_keeps_the_running_build() {
    let dir = tempfile::tempdir().unwrap();
    let first = manifest(dir.path(), "board", "2.1.0", "read-only");
    install_bundle(
        dir.path(),
        &first,
        BYTES,
        &receipt(&first),
        PermissionTier::ReadOnly,
    )
    .unwrap();
    let older = manifest(dir.path(), "board", "2.0.0", "read-only");
    let error = install_bundle(
        dir.path(),
        &older,
        BYTES,
        &receipt(&older),
        PermissionTier::ReadOnly,
    )
    .unwrap_err();
    assert_eq!(error.code(), "E712");
    let root = plugin_dir(dir.path(), "board");
    let text = std::fs::read_to_string(root.join("manifest.json")).unwrap();
    assert!(text.contains("2.1.0"), "{text}");
}

#[test]
fn replaces_the_directory_on_a_newer_build() {
    let dir = tempfile::tempdir().unwrap();
    let first = manifest(dir.path(), "board", "1.0.0", "read-only");
    install_bundle(
        dir.path(),
        &first,
        BYTES,
        &receipt(&first),
        PermissionTier::ReadOnly,
    )
    .unwrap();
    let newer = manifest(dir.path(), "board", "1.1.0", "read-only");
    install_bundle(
        dir.path(),
        &newer,
        BYTES,
        &receipt(&newer),
        PermissionTier::ReadOnly,
    )
    .unwrap();
    let text =
        std::fs::read_to_string(plugin_dir(dir.path(), "board").join("manifest.json")).unwrap();
    assert!(text.contains("1.1.0"), "{text}");
    assert!(
        std::fs::read_dir(dir.path().join("plugins"))
            .unwrap()
            .flatten()
            .all(|entry| entry.file_name() != ".trash"
                || std::fs::read_dir(entry.path())
                    .map_or(true, |mut items| items.next().is_none()))
    );
}

#[test]
fn restores_the_previous_build_when_the_state_write_fails() {
    let dir = tempfile::tempdir().unwrap();
    let first = manifest(dir.path(), "board", "1.0.0", "read-only");
    install_bundle(
        dir.path(),
        &first,
        BYTES,
        &receipt(&first),
        PermissionTier::ReadOnly,
    )
    .unwrap();
    // plugins.json.tmp 是目录 ⇒ 状态原子写必然失败（E730）。
    std::fs::create_dir(dir.path().join("plugins.json.tmp")).unwrap();
    let newer = manifest(dir.path(), "board", "1.2.0", "read-only");
    let error = install_bundle(
        dir.path(),
        &newer,
        BYTES,
        &receipt(&newer),
        PermissionTier::ReadOnly,
    )
    .unwrap_err();
    assert_eq!(error.code(), "E730");
    let text =
        std::fs::read_to_string(plugin_dir(dir.path(), "board").join("manifest.json")).unwrap();
    assert!(text.contains("1.0.0"), "old build must survive: {text}");
}

#[test]
fn refuses_a_tier_above_what_the_manifest_declares() {
    let dir = tempfile::tempdir().unwrap();
    let item = manifest(dir.path(), "board", "1.0.0", "read-only");
    let error = install_bundle(
        dir.path(),
        &item,
        BYTES,
        &receipt(&item),
        PermissionTier::Trusted,
    )
    .unwrap_err();
    assert_eq!(error.code(), "E722");
    assert!(!plugin_dir(dir.path(), "board").exists());
    assert!(!PluginsState::load(dir.path()).plugins.contains_key("board"));
}

#[test]
fn a_revoked_plugin_refuses_reinstall_and_enable() {
    let dir = tempfile::tempdir().unwrap();
    let item = manifest(dir.path(), "board", "1.0.0", "read-only");
    install_bundle(
        dir.path(),
        &item,
        BYTES,
        &receipt(&item),
        PermissionTier::ReadOnly,
    )
    .unwrap();
    revoke_plugin(dir.path(), dir.path(), "board").unwrap();
    let row = list_plugins(dir.path(), dir.path())
        .into_iter()
        .find(|row| row.id == "board")
        .unwrap();
    assert_eq!(row.state, PluginState::Disabled);
    assert!(!row.enabled);
    assert_eq!(row.diagnostics[0].code, "E711");
    let error = install_bundle(
        dir.path(),
        &item,
        BYTES,
        &receipt(&item),
        PermissionTier::ReadOnly,
    )
    .unwrap_err();
    assert_eq!(error.code(), "E711");
    let error = set_enabled(dir.path(), "board", true).unwrap_err();
    assert_eq!(error.code(), "E711");
    uninstall_plugin(dir.path(), dir.path(), "board").unwrap();
    assert!(!PluginsState::load(dir.path()).is_revoked("board"));
}

#[test]
fn sweeping_uses_the_oldest_write_inside_a_staging_entry() {
    let dir = tempfile::tempdir().unwrap();
    let stale = dir.path().join("plugins").join(".staging").join("board-1");
    std::fs::create_dir_all(&stale).unwrap();
    let leftover = stale.join("leftover");
    std::fs::write(&leftover, b"x").unwrap();
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(60 * 60 * 25);
    let handle = std::fs::File::options()
        .write(true)
        .open(&leftover)
        .unwrap();
    handle.set_modified(long_ago).unwrap();
    drop(handle);

    let fresh = dir.path().join("plugins").join(".staging").join("board-2");
    std::fs::create_dir_all(&fresh).unwrap();
    std::fs::write(fresh.join("partial"), b"y").unwrap();

    timo_kernel::plugins::nsh::sweep_staging(dir.path());
    assert!(!stale.exists(), "a day-old half install must be swept");
    assert!(fresh.exists(), "a fresh staging entry must survive");
}
