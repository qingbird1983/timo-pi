//! B 轨 GUI 插件清单（schema 2；设计 02 §3）。
//!
//! 与 A 轨 [`crate::Manifest`] 分开建模：A 轨装载器只认 schema 1，B 轨只认
//! schema 2，互不迁移。清单不签名，代码完整性由 NSH 签名包保证。

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::manifest::{Capabilities, is_safe_relative, is_sha256_ref, is_valid_id};
use crate::permissions::validate_capabilities;

/// B 轨清单 schema 版本。
pub const GUI_MANIFEST_SCHEMA: u32 = 2;

/// 宿主 API 主版本（v1）。
pub const HOST_API_VERSION: u32 = 1;

/// slot 白名单（设计 03 §1）。
pub const SLOT_KINDS: [&str; 6] = [
    "panel",
    "toolCard",
    "composerTrigger",
    "composerControl",
    "entryExtra",
    "theme",
];

/// 插件形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    /// B 轨界面插件。
    Gui,
    /// A 轨编排插件。
    Orchestration,
}

/// 权限挡位（安装时一次授权；有效能力 = 声明 ∩ 挡位上限）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionTier {
    /// 只读：会话/模型快照与声明内读作用域。
    ReadOnly,
    /// 读写：额外允许声明内写/删，不允许执行与联网。
    Workspace,
    /// 执行：允许 exec 与声明白名单联网。
    Trusted,
}

impl std::fmt::Display for PermissionTier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::ReadOnly => "read-only",
            Self::Workspace => "workspace",
            Self::Trusted => "trusted",
        };
        formatter.write_str(label)
    }
}

/// 一个 slot 声明（白名单与运行时行为见设计 03）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SlotDecl {
    /// slot 类型（必须命中 [`SLOT_KINDS`]）。
    pub kind: String,
    /// 容器标题（`panel` 用）。
    pub title: Option<String>,
    /// 图标名（宿主词汇表）。
    pub icon: Option<String>,
    /// 同类型内的排序（升序）。
    pub order: Option<i32>,
}

/// B 轨清单全文（schema 2）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuiManifest {
    /// schema 版本（必须为 [`GUI_MANIFEST_SCHEMA`]）。
    pub schema: u32,
    /// 插件 id（与 A 轨同规则）。
    pub id: String,
    /// 插件形态（本阶段只接受 `gui`）。
    pub kind: PluginKind,
    /// 展示名。
    pub name: String,
    /// 语义化版本。
    pub version: String,
    /// NSH app id（`gui` 必填）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nsh_id: Option<String>,
    /// 源码入口（相对插件根）。
    pub entry: String,
    /// 签名包（相对插件根，`.nshpkg`）。
    pub bundle: String,
    /// 源码树指纹（`sha256:<64 hex>`）。
    pub source_hash: String,
    /// 打包器版本标记。
    pub bundler: String,
    /// 安装时间（可选）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// 宿主 API 主版本（`gui` 必填）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<u32>,
    /// 申请挡位。
    pub tier: PermissionTier,
    /// 声明槽位。
    #[serde(default)]
    pub slots: Vec<SlotDecl>,
    /// 只读查询白名单。
    #[serde(default)]
    pub queries: Vec<String>,
    /// 动作白名单。
    #[serde(default)]
    pub actions: Vec<String>,
    /// `actions.runTool` 可调用工具名。
    #[serde(default)]
    pub tools: Vec<String>,
    /// 声明能力（与 A 轨同结构）。
    #[serde(default)]
    pub capabilities: Capabilities,
}

/// 清单读取/校验错误（E720 字段、E721 API、E722 挡位）。
#[derive(Debug, thiserror::Error)]
pub enum GuiManifestError {
    /// 读取失败（归入 E720：清单不可用）。
    #[error("E720 cannot read {path}: {reason}")]
    Read {
        /// 出错路径。
        path: PathBuf,
        /// 系统错误文案。
        reason: String,
    },
    /// 字段值域非法。
    #[error("E720 invalid manifest {path}: {reason}")]
    Invalid {
        /// 清单路径。
        path: PathBuf,
        /// 人读原因。
        reason: String,
    },
    /// 宿主 API 主版本不兼容。
    #[error("E721 unsupported host api {found} (expected {expected})")]
    Api {
        /// 清单声明的主版本。
        found: u32,
        /// 宿主支持的主版本。
        expected: u32,
    },
    /// 申请挡位不足以承载声明能力。
    #[error("E722 tier {tier} does not allow {reason}")]
    Tier {
        /// 清单申请挡位。
        tier: PermissionTier,
        /// 越界项。
        reason: String,
    },
}

impl GuiManifestError {
    /// 错误码（E720/E721/E722）。
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Read { .. } | Self::Invalid { .. } => "E720",
            Self::Api { .. } => "E721",
            Self::Tier { .. } => "E722",
        }
    }
}

impl GuiManifest {
    /// 读取并校验清单；文件不存在返回 `None`。
    ///
    /// # Errors
    /// 读取失败或字段非法 → [`GuiManifestError`]。
    pub fn load(path: &Path) -> Result<Option<Self>, GuiManifestError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(GuiManifestError::Read {
                    path: path.to_path_buf(),
                    reason: error.to_string(),
                });
            }
        };
        let manifest: Self =
            serde_json::from_str(&text).map_err(|error| GuiManifestError::Invalid {
                path: path.to_path_buf(),
                reason: error.to_string(),
            })?;
        manifest.validate(path)?;
        Ok(Some(manifest))
    }

    /// 校验顺序：字段值域（E720）→ 挡位上限（E722）→ API 版本（E721）。
    ///
    /// # Errors
    /// 任一校验失败 → [`GuiManifestError`]。
    pub fn validate(&self, path: &Path) -> Result<(), GuiManifestError> {
        let invalid = |reason: String| GuiManifestError::Invalid {
            path: path.to_path_buf(),
            reason,
        };
        if self.schema != GUI_MANIFEST_SCHEMA {
            return Err(invalid(format!(
                "unsupported schema {} (expected {GUI_MANIFEST_SCHEMA})",
                self.schema
            )));
        }
        if !is_valid_id(&self.id) {
            return Err(invalid(format!("invalid id `{}`", self.id)));
        }
        if self.kind != PluginKind::Gui {
            return Err(invalid("schema 2 only accepts kind=gui in v1".to_string()));
        }
        if self.name.trim().is_empty() {
            return Err(invalid("name must not be empty".to_string()));
        }
        if !is_semver(&self.version) {
            return Err(invalid(format!(
                "version must be semver-like `1.2.3`: `{}`",
                self.version
            )));
        }
        if !is_safe_relative(&self.entry) {
            return Err(invalid(format!(
                "entry must be a safe relative path: `{}`",
                self.entry
            )));
        }
        if !is_safe_relative(&self.bundle) || !self.bundle.ends_with(".nshpkg") {
            return Err(invalid(format!(
                "bundle must be a safe relative `.nshpkg` path: `{}`",
                self.bundle
            )));
        }
        if !is_sha256_ref(&self.source_hash) {
            return Err(invalid(format!(
                "source_hash must look like `sha256:<64 hex>`: `{}`",
                self.source_hash
            )));
        }
        if self.bundler.trim().is_empty() {
            return Err(invalid("bundler must not be empty".to_string()));
        }
        let nsh_id = self.nsh_id.as_deref().unwrap_or_default();
        if !is_valid_nsh_id(nsh_id) {
            return Err(invalid(format!("invalid nsh_id `{nsh_id}`")));
        }
        if self.api.is_none() {
            return Err(invalid("api is required for gui plugins".to_string()));
        }
        if self.slots.is_empty() {
            return Err(invalid("slots must not be empty".to_string()));
        }
        for slot in &self.slots {
            if !SLOT_KINDS.contains(&slot.kind.as_str()) {
                return Err(invalid(format!("unknown slot kind `{}`", slot.kind)));
            }
            for (label, value) in [("title", &slot.title), ("icon", &slot.icon)] {
                if value.as_ref().is_some_and(|text| text.chars().count() > 64) {
                    return Err(invalid(format!("slot {label} exceeds 64 chars")));
                }
            }
        }
        for (label, values) in [
            ("queries", &self.queries),
            ("actions", &self.actions),
            ("tools", &self.tools),
        ] {
            if values.iter().any(|value| value.trim().is_empty()) {
                return Err(invalid(format!("{label} entries must not be empty")));
            }
        }
        validate_capabilities(&self.id, &self.capabilities)
            .map_err(|error| invalid(format!("capabilities: {error}")))?;
        self.check_tier()?;
        let api = self.api.unwrap_or(HOST_API_VERSION);
        if api != HOST_API_VERSION {
            return Err(GuiManifestError::Api {
                found: api,
                expected: HOST_API_VERSION,
            });
        }
        Ok(())
    }

    /// 挡位上限：声明能力必须落在申请挡位内。
    fn check_tier(&self) -> Result<(), GuiManifestError> {
        let denied = |reason: &str| GuiManifestError::Tier {
            tier: self.tier,
            reason: reason.to_string(),
        };
        if self.tier == PermissionTier::ReadOnly {
            if self.capabilities.exec {
                return Err(denied("exec"));
            }
            if !self.capabilities.net.domains.is_empty() {
                return Err(denied("network access"));
            }
            if !self.capabilities.fs.write.is_empty() || !self.capabilities.fs.delete.is_empty() {
                return Err(denied("fs.write/fs.delete"));
            }
        }
        if self.tier == PermissionTier::Workspace {
            if self.capabilities.exec {
                return Err(denied("exec"));
            }
            if !self.capabilities.net.domains.is_empty() {
                return Err(denied("network access"));
            }
        }
        Ok(())
    }

    /// 包路径（清单所在目录 + `bundle`）。
    #[must_use]
    pub fn bundle_path(&self, root: &Path) -> PathBuf {
        root.join(&self.bundle)
    }
}

/// NSH app id：`com.timo.session-search` 形态，仅字母数字与 `. _ -`。
#[must_use]
pub fn is_valid_nsh_id(value: &str) -> bool {
    if value.is_empty() || value.len() > 128 {
        return false;
    }
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

/// 语义化版本（`MAJOR.MINOR.PATCH`，可选 `-pre` 后缀）。
fn is_semver(value: &str) -> bool {
    let core = value.split_once('-').map_or(value, |(core, _)| core);
    let mut parts = core.split('.');
    (0..3).all(|_| {
        parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
    }) && parts.next().is_none()
}
