//! 扩展 CLI 辅助（`timo ext list/add/remove/trust`）。

use std::path::{Path, PathBuf};

use timo_ext::{
    DiscoveryOptions, Manifest, PackOptions, PermissionStore, Scope, discover, pack,
    validate_capabilities,
};

use super::{now, permissions_path};

/// 已发现扩展摘要（`timo ext list`）。
#[derive(Debug, Clone)]
pub struct ExtensionSummary {
    /// 扩展 id。
    pub id: String,
    /// 根目录。
    pub path: PathBuf,
    /// 发现来源。
    pub scope: Scope,
    /// 已安装 manifest（未安装为 None）。
    pub manifest: Option<Manifest>,
}

/// 发现扩展（不打包、不执行）。
#[must_use]
pub fn summarize(
    data_dir: &Path,
    work_dir: &Path,
    explicit: Vec<PathBuf>,
) -> Vec<ExtensionSummary> {
    discover(&DiscoveryOptions {
        work_dir: work_dir.to_path_buf(),
        data_dir: Some(data_dir.to_path_buf()),
        explicit,
        include_project_pi: true,
        include_project: true,
    })
    .extensions
    .into_iter()
    .map(|found| ExtensionSummary {
        id: found.id,
        path: found.root,
        scope: found.scope,
        manifest: found.manifest,
    })
    .collect()
}

/// 安装扩展：复制源码到用户目录并打包（`timo ext add`）。
///
/// # Errors
/// 源文件不可读、id 非法或打包失败 → [`crate::KernelError`]。
pub async fn install_extension(data_dir: &Path, source: &Path) -> crate::Result<Manifest> {
    let id = source
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !timo_ext::is_valid_id(&id) {
        return Err(crate::KernelError::Extension(format!(
            "invalid extension id `{id}` (rename the file to lowercase letters/digits/-/_)"
        )));
    }
    let root = data_dir.join("extensions").join(&id);
    let dir = root.join("source");
    std::fs::create_dir_all(&dir).map_err(|error| {
        crate::KernelError::Extension(format!("cannot create {}: {error}", dir.display()))
    })?;
    let name = source
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("index.ts"));
    let bytes = std::fs::read(source).map_err(|error| {
        crate::KernelError::Extension(format!("cannot read {}: {error}", source.display()))
    })?;
    std::fs::write(dir.join(&name), bytes)
        .map_err(|error| crate::KernelError::Extension(format!("cannot write source: {error}")))?;
    let out = match pack(PackOptions {
        root: root.clone(),
        entry: dir.join(&name),
        id: id.clone(),
        out_dir: root.clone(),
    })
    .await
    {
        Ok(out) => out,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&root);
            return Err(crate::KernelError::Extension(error.to_string()));
        }
    };
    let mut manifest = out.manifest;
    if out.proposed.exec || !out.proposed.net.domains.is_empty() || !out.proposed.fs.read.is_empty()
    {
        manifest.capabilities.exec |= out.proposed.exec;
        for domain in out.proposed.net.domains {
            if !manifest.capabilities.net.domains.contains(&domain) {
                manifest.capabilities.net.domains.push(domain);
            }
        }
        for scope in out.proposed.fs.read {
            if !manifest.capabilities.fs.read.contains(&scope) {
                manifest.capabilities.fs.read.push(scope);
            }
        }
        // 用户级资源（`<data_dir>/agents` 等）不在 cwd 之下，`**` 覆盖不到，
        // 装机时补一条数据目录自身的绝对作用域（09 §6 真机：E403 读不到代理定义）。
        let data_scope = timo_ext::dir_scope(data_dir);
        if !manifest.capabilities.fs.read.contains(&data_scope) {
            manifest.capabilities.fs.read.push(data_scope);
        }
        manifest
            .validate(&root.join("manifest.json"))
            .map_err(|error| crate::KernelError::Extension(error.to_string()))?;
        let text = serde_json::to_string_pretty(&manifest)
            .map_err(|error| crate::KernelError::Extension(error.to_string()))?;
        std::fs::write(root.join("manifest.json"), text).map_err(|error| {
            crate::KernelError::Extension(format!("cannot write manifest: {error}"))
        })?;
    }
    Ok(manifest)
}

/// 删除扩展及其授权（`timo ext remove`）。
///
/// # Errors
/// 目录删除或授权文件写入失败 → [`crate::KernelError`]。
pub fn remove_extension(data_dir: &Path, id: &str) -> crate::Result<()> {
    let root = data_dir.join("extensions").join(id);
    if root.exists() {
        std::fs::remove_dir_all(&root).map_err(|error| {
            crate::KernelError::Extension(format!("cannot remove {}: {error}", root.display()))
        })?;
    }
    let path = permissions_path(data_dir);
    let mut store = PermissionStore::load(&path).unwrap_or_default();
    if store.revoke(id) {
        store
            .save(&path)
            .map_err(|error| crate::KernelError::Extension(error.to_string()))?;
    }
    Ok(())
}

/// 授予/撤销扩展声明的全部能力（`timo ext trust`）。
///
/// # Errors
/// manifest 缺失/非法、授权写入失败 → [`crate::KernelError`]。
pub fn trust_extension(data_dir: &Path, id: &str, revoke: bool) -> crate::Result<()> {
    let path = permissions_path(data_dir);
    let mut store = PermissionStore::load(&path).unwrap_or_default();
    if revoke {
        store.revoke(id);
        store
            .save(&path)
            .map_err(|error| crate::KernelError::Extension(error.to_string()))?;
        return Ok(());
    }
    let found = summarize(
        data_dir,
        &std::env::current_dir().unwrap_or_default(),
        vec![],
    )
    .into_iter()
    .find(|extension| extension.id == id)
    .ok_or_else(|| crate::KernelError::Extension(format!("extension not found: {id}")))?;
    let manifest = found
        .manifest
        .ok_or_else(|| crate::KernelError::Extension(format!("manifest missing for {id}")))?;
    validate_capabilities(id, &manifest.capabilities)
        .map_err(|error| crate::KernelError::Extension(error.to_string()))?;
    store.grant(&manifest, now());
    store
        .save(&path)
        .map_err(|error| crate::KernelError::Extension(error.to_string()))?;
    Ok(())
}
