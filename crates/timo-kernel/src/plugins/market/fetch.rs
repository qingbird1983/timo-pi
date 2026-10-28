//! 索引拉取：HTTPS + 条件请求，失败保留缓存（设计 08 §4）。

use std::path::Path;
use std::time::Duration;

use timo_config::Clock;

use super::store::{MarketMeta, load_meta, save_meta, write_cache};
use super::{MarketLoad, parse_index};
use crate::plugins::PluginError;

/// 索引体积上限（8 MiB）。
pub const MAX_INDEX_BYTES: usize = 8 * 1024 * 1024;

/// 拉取结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// 拿到并校验了新索引（已写缓存）。
    Updated(MarketLoad),
    /// 304，缓存仍有效。
    NotModified,
    /// 网络或服务不可用；缓存保持原样。
    Offline(String),
}

/// 索引地址必须是带主机的 HTTPS 地址。
#[must_use]
pub fn validate_index_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    parsed.scheme() == "https" && parsed.host_str().is_some_and(|host| !host.is_empty())
}

/// 拉取索引；成功时原子写缓存。
///
/// # Errors
/// 地址非法 → E750；索引内容非法或写盘失败 → E750/E730。
pub async fn refresh(data_dir: &Path, url: &str) -> Result<RefreshOutcome, PluginError> {
    if !validate_index_url(url) {
        return Err(PluginError::new(
            "E750",
            "index url must be an HTTPS address",
        ));
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent(concat!("timo/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| PluginError::new("E751", format!("cannot build http client: {error}")))?;
    let meta = load_meta(data_dir);
    let mut request = client.get(url).header("accept", "application/json");
    if let Some(etag) = &meta.etag {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            return Ok(RefreshOutcome::Offline(format!(
                "market index unavailable: {error}"
            )));
        }
    };
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(RefreshOutcome::NotModified);
    }
    if !response.status().is_success() {
        return Ok(RefreshOutcome::Offline(format!(
            "market index returned {}",
            response.status()
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_INDEX_BYTES as u64)
    {
        return Err(PluginError::new("E750", "market index exceeds 8 MiB"));
    }
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => {
            return Ok(RefreshOutcome::Offline(format!(
                "market index unavailable: {error}"
            )));
        }
    };
    if bytes.len() > MAX_INDEX_BYTES {
        return Err(PluginError::new("E750", "market index exceeds 8 MiB"));
    }
    let text = String::from_utf8(bytes.to_vec())
        .map_err(|error| PluginError::new("E750", format!("market index is not UTF-8: {error}")))?;
    let load = parse_index(&text)?;
    write_cache(data_dir, &load.index)?;
    save_meta(
        data_dir,
        &MarketMeta {
            etag,
            fetched_at: format!("unix:{}", (timo_config::SystemClock).now_ms()),
        },
    )?;
    Ok(RefreshOutcome::Updated(load))
}
