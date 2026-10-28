//! 权限挡位：产品层预设与能力映射（设计 01 §8）。
//!
//! 挡位只收不放：有效能力 = 清单声明 ∩ 挡位上限。A 轨旧扩展显示挡位但
//! 运行期授权仍走 `permissions.json`；本模块不改变 A 轨装载语义。

use timo_ext::{Capabilities, PermissionTier};

/// 由声明能力推断 A 轨旧扩展的挡位（无状态文件时使用）。
#[must_use]
pub fn infer_tier(capabilities: &Capabilities) -> PermissionTier {
    if capabilities.exec || !capabilities.net.domains.is_empty() {
        return PermissionTier::Trusted;
    }
    if !capabilities.fs.write.is_empty() || !capabilities.fs.delete.is_empty() {
        return PermissionTier::Workspace;
    }
    PermissionTier::ReadOnly
}

/// 挡位上限内的有效能力。
#[must_use]
pub fn effective_capabilities(tier: PermissionTier, declared: &Capabilities) -> Capabilities {
    match tier {
        PermissionTier::ReadOnly => Capabilities {
            exec: false,
            net: timo_ext::NetCapability::default(),
            fs: timo_ext::FsCapability {
                read: declared.fs.read.clone(),
                write: vec![],
                delete: vec![],
            },
        },
        PermissionTier::Workspace => Capabilities {
            exec: false,
            net: timo_ext::NetCapability::default(),
            fs: declared.fs.clone(),
        },
        PermissionTier::Trusted => declared.clone(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use timo_ext::{FsCapability, NetCapability};

    fn declared() -> Capabilities {
        Capabilities {
            exec: true,
            net: NetCapability {
                domains: vec!["api.example.com".into()],
            },
            fs: FsCapability {
                read: vec!["workspace/**".into()],
                write: vec!["workspace/**".into()],
                delete: vec!["workspace/tmp/**".into()],
            },
        }
    }

    #[test]
    fn read_only_keeps_reads_only() {
        let effective = effective_capabilities(PermissionTier::ReadOnly, &declared());
        assert!(!effective.exec);
        assert!(effective.net.domains.is_empty());
        assert_eq!(effective.fs.read, vec!["workspace/**"]);
        assert!(effective.fs.write.is_empty());
        assert!(effective.fs.delete.is_empty());
    }

    #[test]
    fn workspace_drops_exec_and_network() {
        let effective = effective_capabilities(PermissionTier::Workspace, &declared());
        assert!(!effective.exec);
        assert!(effective.net.domains.is_empty());
        assert_eq!(effective.fs.write, vec!["workspace/**"]);
    }

    #[test]
    fn trusted_keeps_everything_declared() {
        let effective = effective_capabilities(PermissionTier::Trusted, &declared());
        assert_eq!(effective, declared());
    }

    #[test]
    fn infers_tier_from_capabilities() {
        assert_eq!(
            infer_tier(&Capabilities::default()),
            PermissionTier::ReadOnly
        );
        let mut read_only = Capabilities::default();
        read_only.fs.read.push("workspace/**".into());
        assert_eq!(infer_tier(&read_only), PermissionTier::ReadOnly);

        let mut workspace = Capabilities::default();
        workspace.fs.write.push("workspace/**".into());
        assert_eq!(infer_tier(&workspace), PermissionTier::Workspace);
        assert_eq!(infer_tier(&declared()), PermissionTier::Trusted);
    }
}
