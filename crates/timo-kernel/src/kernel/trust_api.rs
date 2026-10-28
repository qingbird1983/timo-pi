//! 项目信任的只读视图与 `/trust` 保存（设计 phase-3.5/12 §3.5 / §5）。

use std::path::Path;

use crate::prompt::{ResourceWarning, WarningKind};
use crate::trust::{ProjectTrust, TrustChoice, TrustStore};

/// 信任结论对应的资源告警（装配期与 `/reload` 都从这里生成，避免两处口径）。
pub(crate) fn warnings(
    trust: &ProjectTrust,
    data_dir: &Path,
    work_dir: &Path,
) -> Vec<ResourceWarning> {
    let mut out = Vec::new();
    if let Some(message) = &trust.notice {
        out.push(ResourceWarning {
            kind: WarningKind::TrustStore,
            message: message.clone(),
            path: data_dir.join("trust.json"),
        });
    }
    if !trust.trusted && !trust.skipped.is_empty() {
        out.push(ResourceWarning {
            kind: WarningKind::UntrustedProject,
            message: format!(
                "project not trusted ({} resource(s) skipped; run with --approve or /trust)",
                trust.skipped.len()
            ),
            path: work_dir.to_path_buf(),
        });
    }
    out
}

impl super::Kernel {
    /// 本次装配的信任结论（CLI/桌面展示用）。
    #[must_use]
    pub fn project_trust(&self) -> &ProjectTrust {
        &self.project_trust
    }

    /// 扩展 `pi.isProjectTrusted()` 的真值（批 B 接 shim）。
    #[must_use]
    pub fn is_project_trusted(&self) -> bool {
        self.project_trust.trusted
    }

    /// `/trust`：保存一次选择（`Session` 不落盘）。
    ///
    /// # Errors
    /// 存储读不懂或写入失败时返回说明。
    pub fn save_project_trust(&self, choice: TrustChoice) -> Result<(), String> {
        let store = TrustStore::new(&self.data_dir);
        crate::trust::save_choice(&store, &self.work_dir, choice)
    }

    /// `/trust` 菜单的上下文：重新扫描项目资源，并带上最近保存的决策。
    #[must_use]
    pub fn trust_request(&self) -> crate::trust::TrustRequest {
        let store = TrustStore::new(&self.data_dir);
        crate::trust::TrustRequest {
            cwd: self.work_dir.clone(),
            resources: crate::trust::gate::scan(&self.work_dir),
            saved: store.nearest(&self.work_dir).ok().flatten(),
        }
    }
}
