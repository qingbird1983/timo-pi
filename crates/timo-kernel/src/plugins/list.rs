//! 两轨目录扫描与清单聚合（设计 01 §3、§5）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use timo_ext::{
    DiscoveryOptions, GuiManifest, GuiManifestError, Manifest, PermissionTier, PluginKind, discover,
};

use super::model::{
    Declared, PluginDiagnostic, PluginEntry, PluginOrigin, PluginSource, PluginState,
};
use super::state::PluginsState;
use super::tier::infer_tier;

/// 列出全部已安装插件（A 轨项目/用户扩展 + B 轨插件目录）。
///
/// 同名 id 冲突时 A 轨优先。
#[must_use]
pub fn list_plugins(data_dir: &Path, work_dir: &Path) -> Vec<PluginEntry> {
    let state = PluginsState::load(data_dir);
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    let outcome = discover(&DiscoveryOptions {
        work_dir: work_dir.to_path_buf(),
        data_dir: Some(data_dir.to_path_buf()),
        explicit: vec![],
        include_project_pi: true,
    });
    for found in outcome.extensions {
        let revoked = state.is_revoked(&found.id);
        let state = state.get(&found.id);
        let enabled = !revoked && state.is_none_or(|entry| entry.enabled);
        let manifest = found.manifest;
        let declared = declared_from_a(manifest.as_ref());
        let inferred = state
            .and_then(|entry| entry.tier)
            .unwrap_or_else(|| infer_tier(&declared.capabilities));
        let missing = manifest.is_none();
        let mut diagnostics = vec![];
        if missing {
            diagnostics.push(PluginDiagnostic {
                code: "E205".into(),
                message: "manifest.json is missing; the extension will not load".into(),
            });
        }
        if revoked {
            diagnostics.push(PluginDiagnostic {
                code: "E711".into(),
                message: "the plugin is revoked and cannot be enabled".into(),
            });
        }
        seen.insert(found.id.clone());
        entries.push(PluginEntry {
            id: found.id.clone(),
            kind: PluginKind::Orchestration,
            origin: PluginOrigin::Installed,
            name: found.id,
            version: String::new(),
            enabled,
            tier: inferred,
            declared,
            state: if missing {
                PluginState::Error
            } else if enabled {
                PluginState::Enabled
            } else {
                PluginState::Disabled
            },
            source: PluginSource::LocalFile,
            source_hash: manifest.map_or_else(String::new, |item| item.source_hash),
            configurable: false,
            diagnostics,
            path: found.root,
        });
    }
    for (id, root, manifest) in scan_gui(data_dir) {
        if seen.contains(&id) {
            continue;
        }
        let revoked = state.is_revoked(&id);
        let state = state.get(&id);
        let enabled = !revoked && state.is_none_or(|entry| entry.enabled);
        match manifest {
            Ok(manifest) => {
                let mut diagnostics = vec![];
                let mut broken = false;
                if !manifest.bundle_path(&root).is_file() {
                    broken = true;
                    diagnostics.push(PluginDiagnostic {
                        code: "E713".into(),
                        message: format!("package `{}` is missing", manifest.bundle),
                    });
                }
                if revoked {
                    // 撤销 = 拒载但记为「已禁用 + E711」，不当成装载故障（设计 02 §6）。
                    diagnostics.push(PluginDiagnostic {
                        code: "E711".into(),
                        message: "the plugin is revoked and cannot be enabled".into(),
                    });
                }
                let declared_tier = manifest.tier;
                let tier = state
                    .and_then(|entry| entry.tier)
                    .map_or(declared_tier, |chosen| chosen.min(declared_tier));
                entries.push(PluginEntry {
                    id: manifest.id.clone(),
                    kind: PluginKind::Gui,
                    origin: PluginOrigin::Installed,
                    name: manifest.name.clone(),
                    version: manifest.version.clone(),
                    enabled,
                    tier,
                    declared: declared_from_b(&manifest),
                    state: if broken {
                        PluginState::Error
                    } else if enabled {
                        PluginState::Enabled
                    } else {
                        PluginState::Disabled
                    },
                    source: PluginSource::LocalFile,
                    source_hash: manifest.source_hash.clone(),
                    configurable: false,
                    diagnostics,
                    path: root,
                });
            }
            Err(error) => {
                entries.push(PluginEntry {
                    id: id.clone(),
                    kind: PluginKind::Gui,
                    origin: PluginOrigin::Installed,
                    name: id.clone(),
                    version: String::new(),
                    enabled,
                    tier: state
                        .and_then(|entry| entry.tier)
                        .unwrap_or(PermissionTier::ReadOnly),
                    declared: Declared::default(),
                    state: PluginState::Error,
                    source: PluginSource::LocalFile,
                    source_hash: String::new(),
                    configurable: false,
                    diagnostics: vec![PluginDiagnostic {
                        code: error.code().to_string(),
                        message: error.to_string(),
                    }],
                    path: root,
                });
            }
        }
    }
    entries.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    entries
}

fn declared_from_a(manifest: Option<&Manifest>) -> Declared {
    manifest.map_or_else(Declared::default, |manifest| Declared {
        capabilities: manifest.capabilities.clone(),
        ..Declared::default()
    })
}

fn declared_from_b(manifest: &GuiManifest) -> Declared {
    Declared {
        capabilities: manifest.capabilities.clone(),
        slots: manifest.slots.clone(),
        queries: manifest.queries.clone(),
        actions: manifest.actions.clone(),
        tools: manifest.tools.clone(),
        api: manifest.api,
    }
}

/// 扫描 `<data_dir>/plugins/<id>/manifest.json`。
fn scan_gui(data_dir: &Path) -> Vec<(String, PathBuf, Result<GuiManifest, GuiManifestError>)> {
    let root = data_dir.join("plugins");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return vec![];
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with('.') || matches!(name, "assets" | "trust" | "dev") {
            continue;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest_path = path.join("manifest.json");
        if !manifest_path.is_file() {
            continue;
        }
        let manifest = match GuiManifest::load(&manifest_path) {
            Ok(Some(manifest)) => Ok(manifest),
            Ok(None) => Err(GuiManifestError::Read {
                path: manifest_path.clone(),
                reason: "manifest disappeared during scan".into(),
            }),
            Err(error) => Err(error),
        };
        let id = manifest
            .as_ref()
            .map_or_else(|_| name.to_string(), |item| item.id.clone());
        out.push((id, path, manifest));
    }
    out
}
