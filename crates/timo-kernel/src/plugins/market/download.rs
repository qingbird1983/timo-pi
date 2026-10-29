//! 市场包下载：HTTPS + sha256 校验后落暂存目录（设计 08 §5）。
//!
//! 只负责「下载与指纹」；签名、清单校验、挡位确认仍走安装向导（02/01 §6）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
use timo_ext::{GuiManifest, PluginKind};

use super::{MarketPlugin, validate_index_url};
use crate::plugins::PluginError;

/// 包体积上限（64 MiB）。
pub const MAX_PACKAGE_BYTES: usize = 64 * 1024 * 1024;
/// 清单体积上限（1 MiB）。
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

/// 下载 `.nshpkg` 与清单到 `<data_dir>/market/downloads/<id>-<suffix>/`。
///
/// 返回的目录已是安装向导的输入（`manifest.json` + 包文件）。
///
/// # Errors
/// 地址非法或下载失败 → E753；清单缺失/非法或与索引不一致 → E713。
pub async fn download(data_dir: &Path, plugin: &MarketPlugin) -> Result<PathBuf, PluginError> {
    if !validate_index_url(&plugin.package.url) {
        return Err(PluginError::new(
            "E753",
            format!("package url for `{}` must be HTTPS", plugin.id),
        ));
    }
    let Some(manifest_url) = plugin.manifest_url.as_deref() else {
        return Err(PluginError::new(
            "E713",
            format!("`{}` has no manifest_url", plugin.id),
        ));
    };
    if !validate_index_url(manifest_url) {
        return Err(PluginError::new(
            "E713",
            format!("manifest url for `{}` must be HTTPS", plugin.id),
        ));
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent(concat!("timo/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| PluginError::new("E753", format!("cannot build http client: {error}")))?;
    let bundle = fetch(
        &client,
        &plugin.package.url,
        MAX_PACKAGE_BYTES,
        "E753",
        "package",
    )
    .await?;
    let digest = format!("{:x}", Sha256::digest(&bundle));
    if !digest.eq_ignore_ascii_case(&plugin.package.sha256) {
        return Err(PluginError::new(
            "E753",
            format!(
                "package hash mismatch for `{}`: downloaded {digest}",
                plugin.id
            ),
        ));
    }
    let manifest_bytes = fetch(
        &client,
        manifest_url,
        MAX_MANIFEST_BYTES,
        "E713",
        "manifest",
    )
    .await?;
    let manifest_text = String::from_utf8(manifest_bytes)
        .map_err(|error| PluginError::new("E713", format!("manifest is not UTF-8: {error}")))?;
    let manifest: GuiManifest = serde_json::from_str(&manifest_text)
        .map_err(|error| PluginError::new("E713", format!("invalid manifest: {error}")))?;
    if manifest.id != plugin.id {
        return Err(PluginError::new(
            "E713",
            format!(
                "manifest id `{}` does not match index id `{}`",
                manifest.id, plugin.id
            ),
        ));
    }
    if manifest.kind != PluginKind::Gui {
        return Err(PluginError::new(
            "E713",
            format!("`{}` is not a gui manifest", plugin.id),
        ));
    }
    let root =
        super::market_dir(data_dir)
            .join("downloads")
            .join(format!("{}-{}", plugin.id, suffix()));
    let bundle_path = manifest.bundle_path(&root);
    if let Some(parent) = bundle_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| PluginError::new("E730", error.to_string()))?;
    }
    tokio::fs::write(root.join("manifest.json"), manifest_text)
        .await
        .map_err(|error| PluginError::new("E730", error.to_string()))?;
    tokio::fs::write(&bundle_path, &bundle)
        .await
        .map_err(|error| PluginError::new("E730", error.to_string()))?;
    Ok(root)
}

async fn fetch(
    client: &reqwest::Client,
    url: &str,
    limit: usize,
    code: &'static str,
    label: &'static str,
) -> Result<Vec<u8>, PluginError> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| PluginError::new(code, format!("{label} download failed: {error}")))?;
    if !response.status().is_success() {
        return Err(PluginError::new(
            code,
            format!("{label} returned {}", response.status()),
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(PluginError::new(
            code,
            format!("{label} exceeds {} bytes", limit),
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| PluginError::new(code, format!("{label} download failed: {error}")))?;
    if bytes.len() > limit {
        return Err(PluginError::new(
            code,
            format!("{label} exceeds {} bytes", limit),
        ));
    }
    Ok(bytes.to_vec())
}

fn suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos() as u64);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}{count:x}")
}
