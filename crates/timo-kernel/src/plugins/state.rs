//! `plugins.json`：统一启停状态（设计 01 §3、02 §7）。
//!
//! 只存 `id → { enabled, tier, installed_at }`；清单以目录扫描为准。

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use timo_ext::PermissionTier;

use super::PluginError;

/// 状态文件 schema。
pub const PLUGINS_SCHEMA: u32 = 1;

/// 单个插件的持久状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginStateEntry {
    /// 是否启用（缺省 true）。
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// 用户选择的挡位（缺省按声明推断）。
    #[serde(default)]
    pub tier: Option<PermissionTier>,
    /// 安装时间（`unix:<ms>`）。
    #[serde(default)]
    pub installed_at: String,
    /// B 轨签名包 sha256（A 轨缺省）。
    #[serde(default)]
    pub package_sha256: Option<String>,
}

fn default_enabled() -> bool {
    true
}

impl Default for PluginStateEntry {
    fn default() -> Self {
        Self {
            enabled: true,
            tier: None,
            installed_at: String::new(),
            package_sha256: None,
        }
    }
}

/// `plugins.json` 的内存形态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginsState {
    /// 结构版本。
    #[serde(default)]
    pub schema: u32,
    /// 按 id 索引的状态。
    #[serde(default)]
    pub plugins: BTreeMap<String, PluginStateEntry>,
}

impl Default for PluginsState {
    fn default() -> Self {
        Self {
            schema: PLUGINS_SCHEMA,
            plugins: BTreeMap::new(),
        }
    }
}

impl PluginsState {
    /// 读取状态文件；不存在或损坏都回退为空（损坏下次保存时覆盖）。
    #[must_use]
    pub fn load(data_dir: &Path) -> Self {
        let path = state_path(data_dir);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        let mut state: Self = serde_json::from_str(&text).unwrap_or_default();
        state.schema = PLUGINS_SCHEMA;
        state
    }

    /// 原子写回（临时文件 + rename；Unix 0600）。
    ///
    /// # Errors
    /// 目录创建或写入失败 → E730。
    pub fn save(&self, data_dir: &Path) -> Result<(), PluginError> {
        let path = state_path(data_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                PluginError::new(
                    "E730",
                    format!("cannot create {}: {error}", parent.display()),
                )
            })?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| PluginError::new("E730", error.to_string()))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)
            .map_err(|error| PluginError::new("E730", format!("cannot write state: {error}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, &path)
            .map_err(|error| PluginError::new("E730", format!("cannot replace state: {error}")))
    }

    /// 取状态（只读）。
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&PluginStateEntry> {
        self.plugins.get(id)
    }

    /// 写入状态。
    pub fn put(&mut self, id: &str, entry: PluginStateEntry) {
        self.schema = PLUGINS_SCHEMA;
        self.plugins.insert(id.to_string(), entry);
    }

    /// 删除状态。
    pub fn remove(&mut self, id: &str) {
        self.plugins.remove(id);
    }
}

/// 状态文件路径：`<data_dir>/plugins.json`。
#[must_use]
pub fn state_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("plugins.json")
}
