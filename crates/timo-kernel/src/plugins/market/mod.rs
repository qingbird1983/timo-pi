//! 插件市场：静态索引、筛选与本地状态（设计 08）。
//!
//! 索引只提供展示与下载来源；安装仍走签名包校验与挡位授权。

mod download;
mod fetch;
mod store;

use serde::{Deserialize, Serialize};
use timo_ext::{PluginKind, is_semver, is_valid_id};

use super::PluginError;

pub use download::{MAX_MANIFEST_BYTES, MAX_PACKAGE_BYTES, download};
pub use fetch::{RefreshOutcome, refresh, validate_index_url};
pub use store::{
    MarketMeta, MarketState, index_url, load_cached, load_state, market_dir, save_state,
    set_index_url, toggle_blocked, toggle_favorite, write_cache,
};

/// 市场索引 schema。
pub const MARKET_SCHEMA: u32 = 1;

/// 固定分类（v1）。
pub const MARKET_CATEGORIES: [&str; 9] = [
    "AGI 架构探索",
    "UI 增强",
    "用量与计费",
    "主题与外观",
    "模型与账号接入",
    "身份与通信",
    "会话与消息",
    "记忆",
    "工具与能力",
];

/// 市场页签（已安装与高级由界面直接复用插件管理器）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarketTab {
    /// 全部条目。
    #[default]
    Discover,
    /// 主题与外观。
    Theme,
    /// 本地收藏。
    Favorites,
    /// 已屏蔽。
    Blocked,
}

/// 排序方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarketSort {
    /// 索引原始顺序。
    #[default]
    Recommended,
    /// 近 30 天下载降序。
    Downloads,
    /// 星标降序。
    Stars,
}

/// 筛选条件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarketQuery {
    /// 页签。
    pub tab: MarketTab,
    /// 分类（精确匹配；Theme 页签忽略）。
    pub category: Option<String>,
    /// 搜索词（名称/描述/作者/id）。
    pub search: String,
    /// 排序。
    pub sort: MarketSort,
}

/// 市场索引。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketIndex {
    /// schema 版本。
    pub schema: u32,
    /// 索引版本标记。
    #[serde(default)]
    pub market_version: String,
    /// 索引更新时间。
    #[serde(default)]
    pub updated_at: String,
    /// 条目。
    #[serde(default)]
    pub plugins: Vec<MarketPlugin>,
}

/// 一条市场条目。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketPlugin {
    /// 插件 id（与 manifest 一致）。
    pub id: String,
    /// 展示名。
    pub name: String,
    /// 轨道。
    pub kind: PluginKind,
    /// 版本。
    pub version: String,
    /// 作者。
    pub author: Author,
    /// 描述。
    #[serde(default)]
    pub description: String,
    /// 分类（已过滤为固定分类）。
    #[serde(default)]
    pub categories: Vec<String>,
    /// 截图 URL（HTTPS，最多 4 张）。
    #[serde(default)]
    pub screenshots: Vec<String>,
    /// 展示统计。
    #[serde(default)]
    pub stats: Stats,
    /// 版本要求。
    #[serde(default)]
    pub requires: Requires,
    /// 下载来源。
    pub package: Package,
    /// 清单地址（可选）。
    #[serde(default)]
    pub manifest_url: Option<String>,
    /// 主页（可选）。
    #[serde(default)]
    pub homepage: Option<String>,
    /// 讨论/评论地址（可选，只外链）。
    #[serde(default)]
    pub discussion: Option<String>,
}

/// 作者信息。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Author {
    /// 名称。
    #[serde(default)]
    pub name: String,
    /// 主页。
    #[serde(default)]
    pub url: Option<String>,
}

/// 展示统计（宿主不统计，缺失为 0）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Stats {
    /// 近 30 天下载。
    #[serde(default)]
    pub downloads_30d: u64,
    /// 星标。
    #[serde(default)]
    pub stars: u64,
}

/// 版本要求。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Requires {
    /// Timo 版本约束（展示用）。
    #[serde(default)]
    pub timo: Option<String>,
    /// 宿主 API 主版本。
    #[serde(default)]
    pub api: Option<u32>,
}

/// 下载来源。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Package {
    /// 包地址（HTTPS，`.nshpkg`）。
    pub url: String,
    /// 包 sha256（64 hex，无前缀）。
    pub sha256: String,
}

/// 一条索引警告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketWarning {
    /// 错误码（E750/E752）。
    pub code: &'static str,
    /// 人读原因。
    pub message: String,
}

/// 解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketLoad {
    /// 校验后的索引。
    pub index: MarketIndex,
    /// 被忽略的字段/条目。
    pub warnings: Vec<MarketWarning>,
}

/// 解析并校验索引；schema 不符整份拒绝（E750）。
///
/// # Errors
/// JSON/schema 非法 → E750。
pub fn parse_index(text: &str) -> Result<MarketLoad, PluginError> {
    let raw: RawIndex = serde_json::from_str(text)
        .map_err(|error| PluginError::new("E750", format!("invalid market index: {error}")))?;
    if raw.schema != MARKET_SCHEMA {
        return Err(PluginError::new(
            "E750",
            format!("unsupported market schema {} (expected 1)", raw.schema),
        ));
    }
    let mut warnings = Vec::new();
    let mut plugins = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for raw in raw.plugins {
        let Some(plugin) = convert(raw, &mut warnings) else {
            continue;
        };
        if !seen.insert(plugin.id.clone()) {
            warnings.push(MarketWarning {
                code: "E750",
                message: format!("duplicate plugin id `{}` ignored", plugin.id),
            });
            continue;
        }
        plugins.push(plugin);
    }
    Ok(MarketLoad {
        index: MarketIndex {
            schema: MARKET_SCHEMA,
            market_version: raw.market_version,
            updated_at: raw.updated_at,
            plugins,
        },
        warnings,
    })
}

/// 按页签、分类、搜索与排序筛选条目。
#[must_use]
pub fn filter(index: &MarketIndex, state: &MarketState, query: &MarketQuery) -> Vec<MarketPlugin> {
    let search = query.search.trim().to_lowercase();
    let mut out: Vec<MarketPlugin> = index
        .plugins
        .iter()
        .filter(|plugin| match query.tab {
            MarketTab::Blocked => state.blocked.iter().any(|id| id == &plugin.id),
            _ => !state.blocked.iter().any(|id| id == &plugin.id),
        })
        .filter(|plugin| match query.tab {
            MarketTab::Favorites => state.favorites.iter().any(|id| id == &plugin.id),
            MarketTab::Theme => plugin.categories.iter().any(|item| item == "主题与外观"),
            _ => true,
        })
        .filter(|plugin| {
            query
                .category
                .as_ref()
                .is_none_or(|category| plugin.categories.contains(category))
        })
        .filter(|plugin| {
            search.is_empty()
                || plugin.name.to_lowercase().contains(&search)
                || plugin.description.to_lowercase().contains(&search)
                || plugin.author.name.to_lowercase().contains(&search)
                || plugin.id.contains(&search)
        })
        .cloned()
        .collect();
    match query.sort {
        MarketSort::Recommended => {}
        MarketSort::Downloads => {
            out.sort_by_key(|plugin| std::cmp::Reverse(plugin.stats.downloads_30d));
        }
        MarketSort::Stars => out.sort_by_key(|plugin| std::cmp::Reverse(plugin.stats.stars)),
    }
    out
}

#[derive(Debug, Deserialize)]
struct RawIndex {
    schema: u32,
    #[serde(default)]
    market_version: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    plugins: Vec<RawPlugin>,
}

#[derive(Debug, Deserialize)]
struct RawPlugin {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    author: RawAuthor,
    #[serde(default)]
    description: String,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    screenshots: Vec<String>,
    #[serde(default)]
    stats: Stats,
    #[serde(default)]
    requires: Requires,
    #[serde(default)]
    package: Option<Package>,
    #[serde(default)]
    manifest_url: Option<String>,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    discussion: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawAuthor {
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: Option<String>,
}

fn convert(raw: RawPlugin, warnings: &mut Vec<MarketWarning>) -> Option<MarketPlugin> {
    let id = raw.id.clone();
    let reject = |warnings: &mut Vec<MarketWarning>, reason: String| {
        warnings.push(MarketWarning {
            code: "E750",
            message: format!("plugin `{id}` ignored: {reason}"),
        });
        None
    };
    if !is_valid_id(&raw.id) {
        return reject(warnings, "invalid id".into());
    }
    if raw.name.trim().is_empty() {
        return reject(warnings, "empty name".into());
    }
    if !is_semver(&raw.version) {
        return reject(warnings, "version is not semver".into());
    }
    let kind = match raw.kind.as_str() {
        "gui" => PluginKind::Gui,
        "orchestration" => PluginKind::Orchestration,
        other => return reject(warnings, format!("unknown kind `{other}`")),
    };
    let Some(package) = raw.package else {
        return reject(warnings, "missing package".into());
    };
    if !is_https(&package.url) || !package.url.ends_with(".nshpkg") {
        return reject(
            warnings,
            "package url must be HTTPS and end with `.nshpkg`".into(),
        );
    }
    if package.sha256.len() != 64 || !package.sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return reject(warnings, "package sha256 must be 64 hex chars".into());
    }
    let mut categories = Vec::new();
    for category in raw.categories {
        if MARKET_CATEGORIES.contains(&category.as_str()) {
            if !categories.contains(&category) {
                categories.push(category);
            }
        } else {
            warnings.push(MarketWarning {
                code: "E752",
                message: format!("plugin `{id}` has unknown category `{category}` (ignored)"),
            });
        }
    }
    let mut screenshots = Vec::new();
    for shot in raw.screenshots {
        if screenshots.len() == 4 {
            warnings.push(MarketWarning {
                code: "E750",
                message: format!("plugin `{id}` exceeds 4 screenshots (truncated)"),
            });
            break;
        }
        if is_https(&shot) {
            screenshots.push(shot);
        } else {
            warnings.push(MarketWarning {
                code: "E752",
                message: format!("plugin `{id}` has a non-HTTPS screenshot (ignored)"),
            });
        }
    }
    Some(MarketPlugin {
        id: raw.id,
        name: raw.name,
        kind,
        version: raw.version,
        author: Author {
            name: raw.author.name,
            url: raw.author.url.filter(|url| is_https(url)),
        },
        description: raw.description,
        categories,
        screenshots,
        stats: raw.stats,
        requires: raw.requires,
        package: Package {
            sha256: package.sha256.to_ascii_lowercase(),
            ..package
        },
        manifest_url: raw.manifest_url.filter(|url| is_https(url)),
        homepage: raw.homepage.filter(|url| is_https(url)),
        discussion: raw.discussion.filter(|url| is_https(url)),
    })
}

fn is_https(url: &str) -> bool {
    url.starts_with("https://") && url.len() > 8 && !url.contains(char::is_whitespace)
}
