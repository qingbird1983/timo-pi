//! 统一插件清单与生命周期（设计 01；P0 覆盖 A 轨安装与两轨清单）。
//!
//! A 轨运行期仍由 `extensions` 模块装载，本模块只提供清单、状态、挡位与
//! 启停/卸载入口；B 轨签名包安装与运行在 P1 接入。

mod state;
mod tier;

pub mod market;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use timo_config::Clock;
use timo_ext::{
    Capabilities, DiscoveryOptions, GuiManifest, GuiManifestError, Manifest, PermissionStore,
    SlotDecl, discover, is_valid_id,
};

use crate::extensions::{install_extension, remove_extension};

pub use timo_ext::{PermissionTier, PluginKind};

pub use state::{PLUGINS_SCHEMA, PluginStateEntry, PluginsState, state_path};
pub use tier::{effective_capabilities, infer_tier};

/// 插件来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginOrigin {
    /// 内置功能配置入口。
    BuiltInConfig,
    /// 实验性产品开关。
    Experimental,
    /// 第三方已安装插件。
    Installed,
}

/// 安装来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginSource {
    /// 内置。
    BuiltIn,
    /// 本地文件/目录。
    LocalFile,
    /// 远程链接。
    Url,
    /// 市场安装。
    Market,
}

/// 插件状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    /// 已启用。
    Enabled,
    /// 已禁用。
    Disabled,
    /// 装载/校验失败。
    Error,
    /// 有可用更新。
    UpdateAvailable,
}

/// 清单声明的能力（申请，不等于授予）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Declared {
    /// 能力声明。
    pub capabilities: Capabilities,
    /// 界面槽位（仅 B 轨）。
    pub slots: Vec<SlotDecl>,
    /// 只读查询白名单。
    pub queries: Vec<String>,
    /// 动作白名单。
    pub actions: Vec<String>,
    /// 可调用工具名。
    pub tools: Vec<String>,
    /// 宿主 API 主版本。
    pub api: Option<u32>,
}

/// 列表中的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginEntry {
    /// 命名空间 id。
    pub id: String,
    /// 轨道。
    pub kind: PluginKind,
    /// 来源分组。
    pub origin: PluginOrigin,
    /// 展示名。
    pub name: String,
    /// 版本（A 轨旧清单为空）。
    pub version: String,
    /// 是否启用。
    pub enabled: bool,
    /// 实际档位。
    pub tier: PermissionTier,
    /// 清单声明。
    pub declared: Declared,
    /// 状态。
    pub state: PluginState,
    /// 安装来源。
    pub source: PluginSource,
    /// 指纹（A 轨源码 / B 轨清单声明）。
    pub source_hash: String,
    /// 是否有配置页。
    pub configurable: bool,
    /// 最近诊断。
    pub diagnostics: Vec<PluginDiagnostic>,
    /// 插件根目录（内置项为空）。
    pub path: PathBuf,
}

/// 一条插件诊断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDiagnostic {
    /// E7xx 错误码。
    pub code: String,
    /// 人读原因。
    pub message: String,
}

/// 安装结果摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    /// 插件 id。
    pub id: String,
    /// 轨道。
    pub kind: PluginKind,
    /// 采用的挡位。
    pub tier: PermissionTier,
    /// 版本（A 轨旧清单为空）。
    pub version: String,
}

/// 插件操作错误（E7xx）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginError {
    code: &'static str,
    message: String,
}

impl PluginError {
    /// 构造错误。
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// 错误码。
    #[must_use]
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// 人读原因。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PluginError {}

impl From<GuiManifestError> for PluginError {
    fn from(error: GuiManifestError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}

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
        let state = state.get(&found.id);
        let enabled = state.is_none_or(|entry| entry.enabled);
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
        let state = state.get(&id);
        let enabled = state.is_none_or(|entry| entry.enabled);
        match manifest {
            Ok(manifest) => {
                let mut diagnostics = vec![];
                if !manifest.bundle_path(&root).is_file() {
                    diagnostics.push(PluginDiagnostic {
                        code: "E713".into(),
                        message: format!("package `{}` is missing", manifest.bundle),
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
                    state: if !diagnostics.is_empty() {
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

/// 启用/禁用插件（保留文件与授权）。
///
/// # Errors
/// 插件不存在或状态写入失败 → [`PluginError`]。
pub fn set_enabled(data_dir: &Path, id: &str, enabled: bool) -> Result<(), PluginError> {
    ensure_valid_id(id)?;
    let entry = find(data_dir, id)?;
    let mut state = PluginsState::load(data_dir);
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
            "B-track packages arrive with the NSH runtime (P1)",
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
    write_grant(data_dir, &manifest, tier)?;
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

fn write_grant(
    data_dir: &Path,
    manifest: &Manifest,
    tier: PermissionTier,
) -> Result<(), PluginError> {
    let effective = effective_capabilities(tier, &manifest.capabilities);
    let path = data_dir.join("permissions.json");
    let mut store = PermissionStore::load(&path).unwrap_or_default();
    store.grants.insert(
        manifest.id.clone(),
        timo_ext::Grant {
            source_hash: manifest.source_hash.clone(),
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

fn now() -> String {
    format!("unix:{}", (timo_config::SystemClock).now_ms())
}
