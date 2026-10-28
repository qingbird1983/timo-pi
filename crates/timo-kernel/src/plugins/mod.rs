//! 统一插件清单与生命周期（设计 01/02）。
//!
//! A 轨运行期仍由 `extensions` 模块装载，本模块提供清单、状态、挡位与
//! 启停/卸载入口；B 轨签名包由桌面隔离层验签后交 [`nsh::install_bundle`]
//! 落盘，运行期 `AppView` 留在桌面。

mod list;
mod model;
mod state;
mod tier;

pub mod market;
pub mod nsh;

use std::path::Path;

use timo_config::Clock;
use timo_ext::{PermissionStore, is_valid_id};

use crate::extensions::{install_extension, remove_extension};

pub use list::list_plugins;
pub use model::{
    Declared, InstalledPlugin, PluginDiagnostic, PluginEntry, PluginError, PluginOrigin,
    PluginSource, PluginState,
};
pub use state::{PLUGINS_SCHEMA, PluginStateEntry, PluginsState, state_path};
pub use tier::{effective_capabilities, infer_tier};
pub use timo_ext::{PermissionTier, PluginKind};

/// 启用/禁用插件（保留文件与授权）。
///
/// # Errors
/// 插件不存在或状态写入失败 → [`PluginError`]；已撤销的插件不能启用 → E711。
pub fn set_enabled(data_dir: &Path, id: &str, enabled: bool) -> Result<(), PluginError> {
    ensure_valid_id(id)?;
    let entry = find(data_dir, id)?;
    let mut state = PluginsState::load(data_dir);
    if enabled && state.is_revoked(id) {
        return Err(PluginError::new(
            "E711",
            format!("plugin `{id}` is revoked; reinstall it to enable"),
        ));
    }
    let mut next = state.get(id).cloned().unwrap_or_default();
    next.enabled = enabled;
    if next.tier.is_none() {
        next.tier = Some(entry.tier);
    }
    state.put(id, next);
    state.save(data_dir)
}

/// 调整挡位：只允许收紧；放宽必须重新安装。
///
/// # Errors
/// 插件不存在、越权放宽或写盘失败 → E722/E730。
pub fn set_tier(data_dir: &Path, id: &str, tier: PermissionTier) -> Result<(), PluginError> {
    ensure_valid_id(id)?;
    let entry = find(data_dir, id)?;
    if tier > entry.tier {
        return Err(PluginError::new(
            "E722",
            format!("tier can only be tightened (current {})", entry.tier),
        ));
    }
    let mut state = PluginsState::load(data_dir);
    let mut next = state.get(id).cloned().unwrap_or_default();
    next.tier = Some(tier);
    state.put(id, next);
    state.save(data_dir)?;
    rewrite_grant(data_dir, id, &entry, tier)
}

/// 撤销授权（保留文件），并把状态改为禁用。
///
/// # Errors
/// 插件不存在或写盘失败 → [`PluginError`]。
pub fn revoke_plugin(data_dir: &Path, _work_dir: &Path, id: &str) -> Result<(), PluginError> {
    ensure_valid_id(id)?;
    let _entry = find(data_dir, id)?;
    let path = data_dir.join("permissions.json");
    let mut store = PermissionStore::load(&path).unwrap_or_default();
    if store.revoke(id) {
        store
            .save(&path)
            .map_err(|error| PluginError::new("E730", error.to_string()))?;
    }
    let mut state = PluginsState::load(data_dir);
    let mut next = state.get(id).cloned().unwrap_or_default();
    next.enabled = false;
    state.put(id, next);
    state.set_revoked(id, true);
    state.save(data_dir)
}

/// 卸载：停用状态 → 撤授权 → 删目录 → 删状态。
///
/// # Errors
/// 插件不存在或删除/写盘失败 → [`PluginError`]。
pub fn uninstall_plugin(data_dir: &Path, _work_dir: &Path, id: &str) -> Result<(), PluginError> {
    ensure_valid_id(id)?;
    let entry = find(data_dir, id)?;
    match entry.kind {
        PluginKind::Orchestration => remove_extension(data_dir, id)
            .map_err(|error| PluginError::new("E730", error.to_string()))?,
        PluginKind::Gui => {
            if entry.path.exists() {
                std::fs::remove_dir_all(&entry.path).map_err(|error| {
                    PluginError::new(
                        "E730",
                        format!("cannot remove {}: {error}", entry.path.display()),
                    )
                })?;
            }
            let grant_path = data_dir.join("permissions.json");
            let mut store = PermissionStore::load(&grant_path).unwrap_or_default();
            if store.revoke(id) {
                store
                    .save(&grant_path)
                    .map_err(|error| PluginError::new("E730", error.to_string()))?;
            }
        }
    }
    let mut state = PluginsState::load(data_dir);
    state.remove(id);
    state.save(data_dir)
}

/// 安装本地 A 轨扩展（单文件），按所选挡位授予能力。
///
/// # Errors
/// id 非法、打包失败、挡位超出声明或写盘失败 → E710/E722/E730。
pub async fn install_plugin(
    data_dir: &Path,
    source: &Path,
    tier: PermissionTier,
) -> Result<InstalledPlugin, PluginError> {
    if source.extension().is_some_and(|ext| ext == "nshpkg") {
        return Err(PluginError::new(
            "E710",
            "B-track packages must be verified by the desktop NSH layer, then installed \
             through plugins::nsh::install_bundle",
        ));
    }
    let id = source
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    ensure_valid_id(&id)?;
    let manifest = install_extension(data_dir, source)
        .await
        .map_err(|error| PluginError::new("E730", error.to_string()))?;
    let declared = infer_tier(&manifest.capabilities);
    if tier > declared {
        let _ = remove_extension(data_dir, &id);
        return Err(PluginError::new(
            "E722",
            format!("requested tier {tier} exceeds declared capabilities ({declared})"),
        ));
    }
    write_grant(
        data_dir,
        &id,
        &manifest.source_hash,
        &manifest.capabilities,
        tier,
    )?;
    let mut state = PluginsState::load(data_dir);
    state.put(
        &id,
        PluginStateEntry {
            enabled: true,
            tier: Some(tier),
            installed_at: now(),
            package_sha256: None,
        },
    );
    state.save(data_dir)?;
    Ok(InstalledPlugin {
        id,
        kind: PluginKind::Orchestration,
        tier,
        version: String::new(),
    })
}

fn ensure_valid_id(id: &str) -> Result<(), PluginError> {
    if is_valid_id(id) {
        Ok(())
    } else {
        Err(PluginError::new(
            "E720",
            format!("invalid plugin id `{id}`"),
        ))
    }
}

fn find(data_dir: &Path, id: &str) -> Result<PluginEntry, PluginError> {
    list_plugins(data_dir, data_dir)
        .into_iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| PluginError::new("E730", format!("plugin not installed: {id}")))
}

fn write_grant(
    data_dir: &Path,
    id: &str,
    source_hash: &str,
    capabilities: &timo_ext::Capabilities,
    tier: PermissionTier,
) -> Result<(), PluginError> {
    let effective = effective_capabilities(tier, capabilities);
    let path = data_dir.join("permissions.json");
    let mut store = PermissionStore::load(&path).unwrap_or_default();
    store.grants.insert(
        id.to_string(),
        timo_ext::Grant {
            source_hash: source_hash.to_string(),
            exec: effective.exec,
            net: effective.net.domains,
            fs: effective.fs,
            granted_at: now(),
        },
    );
    store
        .save(&path)
        .map_err(|error| PluginError::new("E730", error.to_string()))
}

fn rewrite_grant(
    data_dir: &Path,
    id: &str,
    entry: &PluginEntry,
    tier: PermissionTier,
) -> Result<(), PluginError> {
    let path = data_dir.join("permissions.json");
    let mut store = PermissionStore::load(&path).unwrap_or_default();
    let Some(grant) = store.grants.get_mut(id) else {
        return Ok(());
    };
    let effective = effective_capabilities(tier, &entry.declared.capabilities);
    grant.exec = effective.exec;
    grant.net = effective.net.domains;
    grant.fs = effective.fs;
    store
        .save(&path)
        .map_err(|error| PluginError::new("E730", error.to_string()))
}

fn now() -> String {
    format!("unix:{}", (timo_config::SystemClock).now_ms())
}
