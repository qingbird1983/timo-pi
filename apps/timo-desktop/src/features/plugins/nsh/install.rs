//! 待装目录的验签入口（设计 02 §4：验签在桌面，落盘在内核）。
//!
//! 桌面只看得到普通 Rust 数据：清单、包字节与一张 [`PackageReceipt`]。

use std::path::{Path, PathBuf};

use nsh_platform_gpui::verify_package;
use timo_kernel::plugins::nsh::{PackageReceipt, assets_root, dev_dir, trust_dir};
use timo_kernel::plugins::{GuiManifest, PermissionTier};

use super::open_trust;

/// 一个已通过签名校验、等待用户确认挡位的安装候选。
#[derive(Debug)]
pub(crate) struct InstallCandidate {
    /// 待装目录（清单与包所在处）。
    pub(crate) dir: PathBuf,
    /// 清单全文。
    pub(crate) manifest: GuiManifest,
    /// 包字节（确认挡位后交给内核）。
    pub(crate) bytes: Vec<u8>,
    /// NSH 验签回执。
    pub(crate) receipt: PackageReceipt,
}

impl InstallCandidate {
    /// 安装向导允许的最高挡位 = 清单申请档位（只收不放，K3）。
    #[must_use]
    pub(crate) fn max_tier(&self) -> PermissionTier {
        self.manifest.tier
    }

    /// 确认挡位后交给内核落盘。
    ///
    /// # Errors
    /// 内核安装账本给出的 E7xx 文本。
    pub(crate) fn commit(&self, data_dir: &Path, tier: PermissionTier) -> Result<String, String> {
        timo_kernel::plugins::nsh::install_bundle(
            data_dir,
            &self.manifest,
            &self.bytes,
            &self.receipt,
            tier,
        )
        .map(|installed| installed.id)
        .map_err(|error| error.to_string())
    }
}

/// 读取 `<dir>/manifest.json` 与其中的签名包，并用信任根验签。
///
/// 生产信任根在 `<data_dir>/plugins/trust`；调试构建额外接受
/// `<data_dir>/plugins/dev` 里的开发根（设计 02 §5）。
///
/// # Errors
/// 清单缺失/非法、包文件读不出、无信任根或签名校验失败，均返回人读文本。
pub(crate) fn verify_dir(data_dir: &Path, dir: &Path) -> Result<InstallCandidate, String> {
    let manifest_path = dir.join("manifest.json");
    let manifest = GuiManifest::load(&manifest_path)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| t!("plugins.no_manifest").to_string())?;
    let bundle_path = manifest.bundle_path(dir);
    if !bundle_path.is_file() {
        return Err(t!("plugins.no_bundle", file = manifest.bundle.clone()).to_string());
    }
    let bytes = std::fs::read(&bundle_path).map_err(|error| error.to_string())?;
    let mut dirs = vec![trust_dir(data_dir)];
    if cfg!(debug_assertions) {
        dirs.push(dev_dir(data_dir));
    }
    let trust = open_trust(&dirs)?;
    let assets = assets_root(data_dir);
    let loaded = verify_package(&trust, &bytes, &assets)
        .map_err(|error| t!("plugins.verify_failed", error = error).to_string())?;
    Ok(InstallCandidate {
        dir: dir.to_path_buf(),
        manifest,
        receipt: PackageReceipt {
            app_id: loaded.app.id.clone(),
            version: loaded.app.version.clone(),
            build: loaded.app.build,
            sha256: loaded.receipt.sha256.clone(),
        },
        bytes,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use timo_kernel::plugins::nsh::plugin_dir;

    const MANIFEST: &str = r#"{
  "schema": 2,
  "id": "poc-panel",
  "kind": "gui",
  "name": "PoC Panel",
  "version": "1.0.0",
  "nsh_id": "com.timo.poc",
  "entry": "source/index.tsx",
  "bundle": "plugin.nshpkg",
  "source_hash": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
  "bundler": "scripts/build-nsh-poc",
  "api": 1,
  "tier": "read-only",
  "slots": [{ "kind": "panel", "title": "PoC" }],
  "capabilities": { "exec": false, "net": { "domains": [] }, "fs": { "read": [], "write": [], "delete": [] } }
}"#;

    /// 官方 CLI 产出的包与开发根；没跑过 `scripts/build-nsh-poc` 就跳过（设计 03 §8）。
    fn fixtures() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
        let root = crate::features::plugins::nsh::poc_dir()?;
        let trust = root.join("trust").join("root.pub");
        trust.is_file().then(|| (root.join("packages"), trust))
    }

    /// 把包摆成一个「已解包的插件目录」（清单 + plugin.nshpkg）。
    fn staged(data: &Path, package: &Path) -> std::path::PathBuf {
        let stage = data.join("incoming");
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::write(stage.join("manifest.json"), MANIFEST).unwrap();
        std::fs::copy(package, stage.join("plugin.nshpkg")).unwrap();
        stage
    }

    #[test]
    fn a_signed_package_installs_through_the_kernel_ledger() {
        let Some((packages, trust)) = fixtures() else {
            eprintln!("skip: 先跑 scripts/build-nsh-poc 生成签名包");
            return;
        };
        let data = tempfile::tempdir().unwrap();
        let dev = data.path().join("plugins").join("dev");
        std::fs::create_dir_all(&dev).unwrap();
        std::fs::copy(&trust, dev.join("root.pub")).unwrap();
        let stage = staged(data.path(), &packages.join("poc-v1.nshpkg"));

        let candidate = verify_dir(data.path(), &stage).unwrap();
        assert_eq!(candidate.receipt.app_id, "com.timo.poc");
        assert_eq!(candidate.receipt.version, "1.0.0");
        assert_eq!(candidate.receipt.build, 1);
        assert_eq!(candidate.max_tier(), PermissionTier::ReadOnly);
        let id = candidate
            .commit(data.path(), PermissionTier::ReadOnly)
            .unwrap();
        assert_eq!(id, "poc-panel");
        assert!(
            plugin_dir(data.path(), "poc-panel")
                .join("plugin.nshpkg")
                .is_file()
        );
    }

    #[test]
    fn a_tampered_package_never_reaches_the_ledger() {
        let Some((packages, trust)) = fixtures() else {
            eprintln!("skip: 先跑 scripts/build-nsh-poc 生成签名包");
            return;
        };
        let data = tempfile::tempdir().unwrap();
        let dev = data.path().join("plugins").join("dev");
        std::fs::create_dir_all(&dev).unwrap();
        std::fs::copy(&trust, dev.join("root.pub")).unwrap();
        let stage = staged(data.path(), &packages.join("poc-tampered.nshpkg"));

        // 文案随语言切换，断言只看「有没有错误」与「有没有落盘」。
        let error = verify_dir(data.path(), &stage).unwrap_err();
        assert!(!error.is_empty(), "the tampered package must be refused");
        assert!(!plugin_dir(data.path(), "poc-panel").exists());
    }
}
