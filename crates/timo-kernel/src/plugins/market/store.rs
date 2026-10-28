//! 市场本地存储：索引缓存、收藏/屏蔽状态与索引地址配置（设计 08 §4、§6）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{MarketIndex, MarketLoad, parse_index};
use crate::plugins::PluginError;

/// 市场目录（`<data_dir>/market`）。
#[must_use]
pub fn market_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("market")
}

/// 本地收藏/屏蔽状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketState {
    /// 结构版本。
    #[serde(default)]
    pub schema: u32,
    /// 收藏 id。
    #[serde(default)]
    pub favorites: Vec<String>,
    /// 屏蔽 id。
    #[serde(default)]
    pub blocked: Vec<String>,
    /// 曾安装过的 id（市场显示「已安装」用）。
    #[serde(default)]
    pub installed_seen: Vec<String>,
}

impl Default for MarketState {
    fn default() -> Self {
        Self {
            schema: 1,
            favorites: vec![],
            blocked: vec![],
            installed_seen: vec![],
        }
    }
}

/// 索引缓存元数据。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketMeta {
    /// HTTP ETag（用于条件请求）。
    #[serde(default)]
    pub etag: Option<String>,
    /// 最近一次成功拉取时间（`unix:<ms>`）。
    #[serde(default)]
    pub fetched_at: String,
}

/// 读取缓存索引；无缓存返回 `None`，损坏返回 E750。
///
/// # Errors
/// 缓存文件读失败或索引非法 → [`PluginError`]。
pub fn load_cached(data_dir: &Path) -> Result<Option<MarketLoad>, PluginError> {
    let path = market_dir(data_dir).join("index.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(PluginError::new(
                "E751",
                format!("cannot read market cache: {error}"),
            ));
        }
    };
    parse_index(&text).map(Some)
}

/// 原子写入缓存索引。
///
/// # Errors
/// 序列化或写盘失败 → E730。
pub fn write_cache(data_dir: &Path, index: &MarketIndex) -> Result<(), PluginError> {
    let text = serde_json::to_string_pretty(index)
        .map_err(|error| PluginError::new("E730", error.to_string()))?;
    write_atomic(&market_dir(data_dir).join("index.json"), &text)
}

/// 读取缓存元数据（缺失为空）。
#[must_use]
pub fn load_meta(data_dir: &Path) -> MarketMeta {
    std::fs::read_to_string(market_dir(data_dir).join("meta.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// 写入缓存元数据。
///
/// # Errors
/// 写盘失败 → E730。
pub fn save_meta(data_dir: &Path, meta: &MarketMeta) -> Result<(), PluginError> {
    let text = serde_json::to_string_pretty(meta)
        .map_err(|error| PluginError::new("E730", error.to_string()))?;
    write_atomic(&market_dir(data_dir).join("meta.json"), &text)
}

/// 读取本地状态（缺失或损坏都回退为空）。
#[must_use]
pub fn load_state(data_dir: &Path) -> MarketState {
    std::fs::read_to_string(market_dir(data_dir).join("state.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// 原子写入本地状态。
///
/// # Errors
/// 写盘失败 → E730。
pub fn save_state(data_dir: &Path, state: &MarketState) -> Result<(), PluginError> {
    let text = serde_json::to_string_pretty(state)
        .map_err(|error| PluginError::new("E730", error.to_string()))?;
    write_atomic(&market_dir(data_dir).join("state.json"), &text)
}

/// 切换收藏；返回切换后是否已收藏。
///
/// # Errors
/// 写盘失败 → E730。
pub fn toggle_favorite(data_dir: &Path, id: &str) -> Result<bool, PluginError> {
    let mut state = load_state(data_dir);
    let added = toggle(&mut state.favorites, id);
    state.schema = 1;
    save_state(data_dir, &state)?;
    Ok(added)
}

/// 切换屏蔽；返回切换后是否已屏蔽。
///
/// # Errors
/// 写盘失败 → E730。
pub fn toggle_blocked(data_dir: &Path, id: &str) -> Result<bool, PluginError> {
    let mut state = load_state(data_dir);
    let added = toggle(&mut state.blocked, id);
    state.schema = 1;
    save_state(data_dir, &state)?;
    Ok(added)
}

fn toggle(list: &mut Vec<String>, id: &str) -> bool {
    if let Some(index) = list.iter().position(|item| item == id) {
        list.remove(index);
        false
    } else {
        list.push(id.to_string());
        true
    }
}

/// 读取设置中的市场索引地址（`market.index_url`）。
#[must_use]
pub fn index_url(data_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(data_dir.join("settings.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("market")?
        .get("index_url")?
        .as_str()
        .map(str::to_string)
}

/// 写入/清除市场索引地址（保留 settings.json 其他字段）。
///
/// # Errors
/// 地址非 HTTPS 或写盘失败 → E750/E730。
pub fn set_index_url(data_dir: &Path, url: Option<&str>) -> Result<(), PluginError> {
    if let Some(url) = url
        && !super::validate_index_url(url)
    {
        return Err(PluginError::new(
            "E750",
            "index url must be an HTTPS address",
        ));
    }
    let path = data_dir.join("settings.json");
    let mut root: serde_json::Value = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| PluginError::new("E730", format!("invalid settings.json: {error}")))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(error) => {
            return Err(PluginError::new(
                "E730",
                format!("cannot read settings.json: {error}"),
            ));
        }
    };
    let object = root
        .as_object_mut()
        .ok_or_else(|| PluginError::new("E730", "settings.json must be an object"))?;
    match url {
        Some(url) => {
            let market = object
                .entry("market")
                .or_insert_with(|| serde_json::json!({}));
            let market = market
                .as_object_mut()
                .ok_or_else(|| PluginError::new("E730", "settings.market must be an object"))?;
            market.insert("index_url".into(), serde_json::json!(url));
        }
        None => {
            if let Some(market) = object
                .get_mut("market")
                .and_then(|item| item.as_object_mut())
            {
                market.remove("index_url");
            }
        }
    }
    let text = serde_json::to_string_pretty(&root)
        .map_err(|error| PluginError::new("E730", error.to_string()))?;
    write_atomic(&path, &text)
}

fn write_atomic(path: &Path, text: &str) -> Result<(), PluginError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            PluginError::new(
                "E730",
                format!("cannot create {}: {error}", parent.display()),
            )
        })?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|error| {
        PluginError::new("E730", format!("cannot write {}: {error}", tmp.display()))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path).map_err(|error| {
        PluginError::new(
            "E730",
            format!("cannot replace {}: {error}", path.display()),
        )
    })
}
