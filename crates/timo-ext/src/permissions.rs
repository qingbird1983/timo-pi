//! 权限双闸（设计 03）：manifest 声明校验、授权存储与域名匹配。
//!
//! 强制点仍在宿主函数；本模块只回答"声明是否合法"与"该 id 当前被授予什么"。

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::diag::ErrorCode;
use crate::error::ExtError;
use crate::manifest::{Capabilities, FsCapability, Manifest, NetCapability};

/// `permissions.json` 的结构版本。
pub const PERMISSIONS_SCHEMA: u32 = 1;

/// 一次授权记录（绑定源码指纹）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Grant {
    /// 授权时的源码指纹。
    pub source_hash: String,
    /// 子进程能力。
    #[serde(default)]
    pub exec: bool,
    /// 允许的域名。
    #[serde(default)]
    pub net: Vec<String>,
    /// 文件能力（本版仅声明面）。
    #[serde(default)]
    pub fs: FsCapability,
    /// 授权时间（调用方注入）。
    #[serde(default)]
    pub granted_at: String,
}

impl Grant {
    /// 转成宿主使用的能力集合。
    #[must_use]
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            exec: self.exec,
            net: NetCapability {
                domains: self.net.clone(),
            },
            fs: self.fs.clone(),
        }
    }
}

/// 授权状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantState {
    /// 已授权且源码未变。
    Granted,
    /// 从未授权。
    Missing,
    /// 源码已变化，旧授权作废（E406）。
    Stale,
}

/// `permissions.json` 的内存形态。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionStore {
    /// 结构版本。
    #[serde(default)]
    pub schema: u32,
    /// 按扩展 id 索引的授权。
    #[serde(default)]
    pub grants: BTreeMap<String, Grant>,
}

impl PermissionStore {
    /// 读取授权文件；文件不存在视为空。
    ///
    /// # Errors
    /// 读取/解析失败 → [`ExtError::Read`]。
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                schema: PERMISSIONS_SCHEMA,
                grants: BTreeMap::new(),
            });
        }
        let text = std::fs::read_to_string(path).map_err(|error| ExtError::Read {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
        let mut store: Self = serde_json::from_str(&text).map_err(|error| ExtError::Read {
            path: path.to_path_buf(),
            reason: format!("invalid permissions json: {error}"),
        })?;
        store.schema = PERMISSIONS_SCHEMA;
        Ok(store)
    }

    /// 原子写回（临时文件 + rename）；Unix 下 0600。
    ///
    /// # Errors
    /// 目录/写入失败 → [`ExtError::Read`]。
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| ExtError::Read {
                path: parent.to_path_buf(),
                reason: error.to_string(),
            })?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|error| ExtError::Read {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|error| ExtError::Read {
            path: tmp.clone(),
            reason: error.to_string(),
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, path).map_err(|error| ExtError::Read {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })
    }

    /// 授予 manifest 声明的全部能力（只收不放）。
    pub fn grant(&mut self, manifest: &Manifest, at: impl Into<String>) -> Grant {
        let declared = &manifest.capabilities;
        let grant = Grant {
            source_hash: manifest.source_hash.clone(),
            exec: declared.exec,
            net: declared.net.domains.clone(),
            fs: declared.fs.clone(),
            granted_at: at.into(),
        };
        self.schema = PERMISSIONS_SCHEMA;
        self.grants.insert(manifest.id.clone(), grant.clone());
        grant
    }

    /// 撤销某扩展的全部授权。
    pub fn revoke(&mut self, id: &str) -> bool {
        self.grants.remove(id).is_some()
    }

    /// 当前生效能力；源码指纹不一致时返回全拒并标记 `Stale`。
    #[must_use]
    pub fn effective(&self, id: &str, source_hash: &str) -> (Capabilities, GrantState) {
        match self.grants.get(id) {
            None => (Capabilities::default(), GrantState::Missing),
            Some(grant) if grant.source_hash != source_hash => {
                (Capabilities::default(), GrantState::Stale)
            }
            Some(grant) => (grant.capabilities(), GrantState::Granted),
        }
    }
}

/// 校验 manifest 的能力声明（域名/作用域/全树/硬拒）。
///
/// # Errors
/// 域名或作用域非法 → E110；全树写删 → E403；硬拒路径 → E404。
pub fn validate_capabilities(id: &str, capabilities: &Capabilities) -> Result<()> {
    let invalid = |field: &str, reason: String| ExtError::InvalidManifest {
        path: std::path::PathBuf::from(format!("extensions/{id}/manifest.json")),
        reason: format!("{field}: {reason}"),
    };
    for domain in &capabilities.net.domains {
        if !is_valid_domain(domain) {
            return Err(invalid(
                "capabilities.net.domains",
                format!("invalid domain `{domain}`"),
            ));
        }
    }
    for (label, scopes) in [
        ("read", &capabilities.fs.read),
        ("write", &capabilities.fs.write),
        ("delete", &capabilities.fs.delete),
    ] {
        for scope in scopes {
            if is_whole_tree(scope) && label != "read" {
                return Err(ExtError::Permission {
                    code: ErrorCode::E403,
                    message: format!("fs.{label} must not cover the whole tree: `{scope}`"),
                });
            }
            if is_hard_denied(scope) {
                return Err(ExtError::Permission {
                    code: ErrorCode::E404,
                    message: format!("fs.{label} targets a protected path: `{scope}`"),
                });
            }
        }
    }
    Ok(())
}

/// 域名声明：主机名 + 可选端口；仅允许最左 `*.`。
#[must_use]
pub fn is_valid_domain(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value == "*" {
        return false;
    }
    let (host, port) = match value.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => (host, Some(port)),
        Some(_) => return false,
        None => (value, None),
    };
    if let Some(port) = port {
        match port.parse::<u16>() {
            Ok(port) if port > 0 => {}
            _ => return false,
        }
    }
    let name = host.strip_prefix("*.").unwrap_or(host);
    if name.contains('*') || name.starts_with('.') || name.ends_with('.') || name.is_empty() {
        return false;
    }
    if host.starts_with("*.") && !name.contains('.') {
        return false;
    }
    name.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && !label.starts_with('-')
            && !label.ends_with('-')
    })
}

/// 域名匹配：最左通配匹配一级及以上子域，不匹配 apex 本身。
#[must_use]
pub fn domain_allowed(patterns: &[String], host: &str, port: u16) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    patterns.iter().any(|pattern| {
        let pattern = pattern.trim();
        let (name, allowed_port) = match pattern.rsplit_once(':') {
            Some((name, port)) if !name.contains(':') => (name, port.parse::<u16>().ok()),
            _ => (pattern, None),
        };
        if let Some(allowed_port) = allowed_port
            && allowed_port != port
        {
            return false;
        }
        let name = name.to_ascii_lowercase();
        if let Some(suffix) = name.strip_prefix("*.") {
            host.len() > suffix.len()
                && host.ends_with(suffix)
                && host.as_bytes()[host.len() - suffix.len() - 1] == b'.'
        } else {
            host == name
        }
    })
}

fn is_whole_tree(scope: &str) -> bool {
    let normalized = scope.trim().replace('\\', "/");
    let normalized = normalized.trim_end_matches('/');
    if normalized.is_empty()
        || normalized == "~"
        || normalized == "**"
        || normalized == "/**"
        || normalized == "~/**"
    {
        return true;
    }
    if normalized.len() >= 2 && normalized.as_bytes()[1] == b':' {
        let rest = normalized[2..].trim_start_matches('/');
        return rest.is_empty() || rest == "**";
    }
    false
}

fn is_hard_denied(scope: &str) -> bool {
    let normalized = scope.replace('\\', "/").to_ascii_lowercase();
    normalized.split('/').any(|segment| {
        segment.starts_with(".env")
            || segment == ".ssh"
            || segment == ".git"
            || segment == "id_rsa"
            || segment == "id_ed25519"
            || segment.contains("credential")
    })
}
