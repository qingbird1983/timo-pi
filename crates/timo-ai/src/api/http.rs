//! 共享 HTTP 发送：传输层重试（仅初始请求；流开始后不重试，设计 01 §8）。

use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::errors::{AiError, ERROR_BODY_CAP};
use crate::retry;
use crate::stream::StreamOptions;

/// 一次 SSE 请求的参数包（收敛 post_sse 参数量）。
pub(crate) struct SseRequest<'a> {
    /// 完整端点 URL。
    pub url: &'a str,
    /// 请求头（含鉴权）。
    pub headers: Vec<(String, String)>,
    /// 请求体。
    pub body: &'a Value,
    /// 流选项（取消/重试上限取自此）。
    pub options: &'a StreamOptions,
    /// 供应商 id（错误信息用）。
    pub provider: &'a str,
}

/// POST JSON 并期望 SSE 响应；带传输层重试与取消。
///
/// # Errors
/// 重试穷尽/不可重试状态/服务端退避超上限/取消 → [`AiError`]。
pub(crate) async fn post_sse(
    http: &reqwest::Client,
    request: SseRequest<'_>,
) -> Result<reqwest::Response, AiError> {
    let SseRequest {
        url,
        headers,
        body,
        options,
        provider,
    } = request;
    let cancel = &options.cancel;
    let max_retries = options.max_retries;
    let max_retry_delay_ms = options.max_retry_delay_ms;
    let payload = serde_json::to_string(body).map_err(|e| AiError::Json {
        context: "request body".into(),
        detail: e.to_string(),
    })?;
    let attempts = max_retries.saturating_add(1);
    for attempt in 0..attempts {
        let mut req = http
            .post(url)
            .header("content-type", "application/json")
            .body(payload.clone());
        for (k, v) in &headers {
            req = req.header(k.as_str(), v.as_str());
        }
        let sent = tokio::select! {
            _ = cancel.cancelled() => return Err(AiError::Sse("aborted".into())),
            r = req.send() => r,
        };
        match sent {
            Ok(resp) if resp.status().is_success() => return Ok(resp),
            Ok(resp) => {
                let status = resp.status().as_u16();
                let should_retry = resp
                    .headers()
                    .get("x-should-retry")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<bool>().ok());
                if attempt + 1 >= attempts || !retry::transport_should_retry(status, should_retry) {
                    let body = resp.text().await.unwrap_or_default();
                    let body: String = body.chars().take(ERROR_BODY_CAP).collect();
                    return Err(AiError::Http {
                        status,
                        provider: provider.into(),
                        body,
                    });
                }
                let delay = retry_after_ms(resp.headers());
                match retry::transport_delay(delay, attempt, max_retry_delay_ms) {
                    Some(d) => sleep_or_cancel(d, cancel).await?,
                    None => {
                        return Err(AiError::Http {
                            status,
                            provider: provider.into(),
                            body: format!("server requested retry delay beyond cap ({delay:?})"),
                        });
                    }
                }
            }
            Err(e) => {
                if attempt + 1 >= attempts {
                    return Err(AiError::Transport(e.to_string()));
                }
                match retry::transport_delay(None, attempt, max_retry_delay_ms) {
                    Some(d) => sleep_or_cancel(d, cancel).await?,
                    None => return Err(AiError::Transport(e.to_string())),
                }
            }
        }
    }
    unreachable!("retry loop returns within attempts")
}

/// `retry-after-ms` → `retry-after`（秒或 HTTP 日期不支持时退回 None）。
fn retry_after_ms(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    if let Some(v) = headers.get("retry-after-ms").and_then(|v| v.to_str().ok()) {
        return v.parse::<u64>().ok();
    }
    headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|s| s.saturating_mul(1000))
}

async fn sleep_or_cancel(
    d: std::time::Duration,
    cancel: &CancellationToken,
) -> Result<(), AiError> {
    tokio::select! {
        _ = cancel.cancelled() => Err(AiError::Sse("aborted".into())),
        _ = tokio::time::sleep(d) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_parsing() {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("retry-after-ms", "250".parse().expect("hdr"));
        assert_eq!(retry_after_ms(&h), Some(250));
        let mut h2 = reqwest::header::HeaderMap::new();
        h2.insert("retry-after", "2".parse().expect("hdr"));
        assert_eq!(retry_after_ms(&h2), Some(2000));
    }
}
