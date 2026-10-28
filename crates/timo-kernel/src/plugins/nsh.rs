//! B 轨签名包的内核侧安装账本（设计 02 §4/§5/§7）。
//!
//! 内核不链接 NSH：签名、证书、撤销与降级由桌面隔离层用 `verify_package`
//! 判定，把结论作为 [`PackageReceipt`] 交回这里做**清单一致性校验、落盘、
//! 授权与状态写入**。NSH 类型不出桌面（设计 00 §6）。

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use timo_ext::GuiManifest;

use super::state::PluginsState;
use super::{InstalledPlugin, PermissionTier, PluginError, now, write_grant};

/// 桌面隔离层校验通过后交回的安装凭据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageReceipt {
    /// NSH app id（必须等于清单 `nsh_id`）。
    pub app_id: String,
    /// NSH app version（必须等于清单 `version`）。
    pub version: String,
    /// NSH build 号（只用于诊断文本；单调性由 NSH 信任库保证）。
    pub build: u64,
    /// 包内容 sha256（小写十六进制，不带前缀）。
    pub sha256: String,
}

/// 插件根目录：`<data_dir>/plugins`。
#[must_use]
pub fn plugins_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("plugins")
}

/// 单个插件目录。
#[must_use]
pub fn plugin_dir(data_dir: &Path, id: &str) -> PathBuf {
    plugins_dir(data_dir).join(id)
}

/// NSH 资源根（`verify_package` 的 `assets_root`）。
#[must_use]
pub fn assets_root(data_dir: &Path) -> PathBuf {
    plugins_dir(data_dir).join("assets")
}

/// 生产信任根目录。
#[must_use]
pub fn trust_dir(data_dir: &Path) -> PathBuf {
    plugins_dir(data_dir).join("trust")
}

/// 开发信任根目录（仅 debug 构建或显式开启时使用）。
#[must_use]
pub fn dev_dir(data_dir: &Path) -> PathBuf {
    plugins_dir(data_dir).join("dev")
}

fn staging_root(data_dir: &Path) -> PathBuf {
    plugins_dir(data_dir).join(".staging")
}

fn trash_root(data_dir: &Path) -> PathBuf {
    plugins_dir(data_dir).join(".trash")
}

/// 安装级撤销是否命中（设计 02 §5 的第二层）。
#[must_use]
pub fn is_revoked(data_dir: &Path, id: &str) -> bool {
    PluginsState::load(data_dir)
        .revoked
        .iter()
        .any(|item| item == id)
}

/// 落盘一个已验签的 B 轨包（新增安装或同 id 更新）。
///
/// # Errors
/// 撤销命中 E711；清单与包不一致 E713；版本降低 E712；挡位越界 E722；
/// 落盘失败 E730（回滚旧目录，不留下半装状态）。
pub fn install_bundle(
    data_dir: &Path,
    manifest: &GuiManifest,
    bundle: &[u8],
    receipt: &PackageReceipt,
    tier: PermissionTier,
) -> Result<InstalledPlugin, PluginError> {
    if is_revoked(data_dir, &manifest.id) {
        return Err(PluginError::new(
            "E711",
            format!("plugin `{}` is revoked", manifest.id),
        ));
    }
    let digest = format!("{:x}", Sha256::digest(bundle));
    if !digest.eq_ignore_ascii_case(&receipt.sha256) {
        return Err(PluginError::new(
            "E711",
            format!(
                "package hash mismatch: bytes are {digest}, receipt says {}",
                receipt.sha256
            ),
        ));
    }
    let declared = manifest.nsh_id.clone().unwrap_or_default();
    if declared != receipt.app_id || manifest.version != receipt.version {
        return Err(PluginError::new(
            "E713",
            format!(
                "manifest says {}/{} but the package is {}/{}",
                declared, manifest.version, receipt.app_id, receipt.version
            ),
        ));
    }
    let root = plugin_dir(data_dir, &manifest.id);
    if let Some(previous) = installed_manifest(data_dir, &manifest.id)
        && semver_order(&manifest.version, &previous.version) == Some(std::cmp::Ordering::Less)
    {
        return Err(PluginError::new(
            "E712",
            format!(
                "downgrade refused: {} installed, package is {} (build {})",
                previous.version, manifest.version, receipt.build
            ),
        ));
    }
    if tier > manifest.tier {
        return Err(PluginError::new(
            "E722",
            format!("requested tier {tier} exceeds declared {}", manifest.tier),
        ));
    }

    let staging = staging_root(data_dir).join(format!("{}-{}", manifest.id, unique_suffix()));
    let trash = trash_root(data_dir).join(format!("{}-{}", manifest.id, unique_suffix()));
    std::fs::create_dir_all(&staging)
        .map_err(|error| PluginError::new("E730", error.to_string()))?;
    write_bundle_files(&staging, manifest, bundle)?;

    let had_previous = root.exists();
    if had_previous && let Err(error) = move_dir(&root, &trash) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(PluginError::new(
            "E730",
            format!("cannot stash the old build: {error}"),
        ));
    }
    let exchange = std::fs::rename(&staging, &root).map_err(|error| {
        PluginError::new(
            "E730",
            format!("cannot install into {}: {error}", root.display()),
        )
    });
    if let Err(error) = exchange {
        if had_previous {
            let _ = move_dir(&trash, &root);
        }
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }

    let written = write_grant(
        data_dir,
        &manifest.id,
        &manifest.source_hash,
        &manifest.capabilities,
        tier,
    )
    .and_then(|()| {
        let mut state = PluginsState::load(data_dir);
        let mut entry = state.get(&manifest.id).cloned().unwrap_or_default();
        entry.enabled = true;
        entry.tier = Some(tier);
        entry.installed_at = now();
        entry.package_sha256 = Some(receipt.sha256.clone());
        state.put(&manifest.id, entry);
        state.set_revoked(&manifest.id, false);
        state.save(data_dir)
    });
    if let Err(error) = written {
        let _ = std::fs::remove_dir_all(&root);
        if had_previous {
            let _ = move_dir(&trash, &root);
        }
        return Err(error);
    }
    let _ = std::fs::remove_dir_all(&trash);
    sweep_staging(data_dir);

    Ok(InstalledPlugin {
        id: manifest.id.clone(),
        kind: timo_ext::PluginKind::Gui,
        tier,
        version: manifest.version.clone(),
    })
}

/// 清扫超过 24h 的 `.staging` / `.trash`（设计 05 §7）。
///
/// 年龄取「暂存条目里最早的一次写入」：每次安装都用新的后缀建目录，所以最早
/// 写入就是它的创建时间；崩溃留下的半装目录据此过期。
pub fn sweep_staging(data_dir: &Path) {
    let cutoff = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(std::time::Duration::ZERO)
        .saturating_sub(std::time::Duration::from_secs(24 * 60 * 60));
    for root in [staging_root(data_dir), trash_root(data_dir)] {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let stale = oldest_write(&entry.path())
                .ok()
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .is_none_or(|elapsed| elapsed < cutoff);
            if !stale {
                continue;
            }
            if entry.path().is_dir() {
                let _ = std::fs::remove_dir_all(entry.path());
            } else {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// 条目本身与其内容里最早的写入时间（有界遍历，只用于暂存清理）。
fn oldest_write(path: &Path) -> std::io::Result<std::time::SystemTime> {
    let mut oldest = std::fs::metadata(path)?.modified()?;
    let mut stack = vec![path.to_path_buf()];
    let mut visited = 0_usize;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > 256 {
                return Ok(oldest);
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if let Ok(modified) = metadata.modified() {
                oldest = std::cmp::min(oldest, modified);
            }
            if metadata.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    Ok(oldest)
}

fn installed_manifest(data_dir: &Path, id: &str) -> Option<GuiManifest> {
    let Ok(text) = std::fs::read_to_string(plugin_dir(data_dir, id).join("manifest.json")) else {
        return None;
    };
    serde_json::from_str::<GuiManifest>(&text).ok()
}

fn write_bundle_files(
    staging: &Path,
    manifest: &GuiManifest,
    bundle: &[u8],
) -> Result<(), PluginError> {
    let manifest_path = staging.join("manifest.json");
    let text = serde_json::to_string_pretty(manifest)
        .map_err(|error| PluginError::new("E730", error.to_string()))?;
    std::fs::write(&manifest_path, text)
        .map_err(|error| PluginError::new("E730", format!("cannot write manifest: {error}")))?;
    let bundle_path = manifest.bundle_path(staging);
    if let Some(parent) = bundle_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| PluginError::new("E730", error.to_string()))?;
    }
    std::fs::write(&bundle_path, bundle)
        .map_err(|error| PluginError::new("E730", format!("cannot write bundle: {error}")))
}

fn move_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(from, to)
}

fn unique_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos() as u64);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id() as u64;
    format!("{pid:x}{nanos:x}{count:x}")
}

/// `1.2.3` 三段比较；无法解析时返回 `None`（交给 NSH 判定）。
fn semver_order(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    let parse = |value: &str| -> Option<[u64; 3]> {
        let head = value.split(['-', '+']).next().unwrap_or(value);
        let mut parts = head.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().unwrap_or("0").parse().ok()?;
        let patch = parts.next().unwrap_or("0").parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some([major, minor, patch])
    };
    Some(parse(left)?.cmp(&parse(right)?))
}
