//! 两层重试（设计 01 §8；对齐 pi provider-retry / retry）。

use std::time::Duration;

/// 传输层判定：状态码 + `x-should-retry` 头是否可重试（pi 同构）。
#[must_use]
pub fn transport_should_retry(status: u16, should_retry_header: Option<bool>) -> bool {
    match should_retry_header {
        Some(v) => v, // 头明确时以头为准（false → 永不）
        None => status == 408 || status == 409 || status == 429 || status >= 500,
    }
}

/// 传输层退避：`retry_after_ms`（服务端要求）优先；否则指数 `min(0.5·2^n, 8s)`
/// 乘 (1 − rand·0.25)（仅向下抖动）。返回 `None` 表示服务端要求超上限 → 立即失败。
#[must_use]
pub fn transport_delay(
    retry_after_ms: Option<u64>,
    attempt: u32,
    max_retry_delay_ms: u64,
) -> Option<Duration> {
    if let Some(ms) = retry_after_ms {
        if max_retry_delay_ms > 0 && ms > max_retry_delay_ms {
            return None;
        }
        return Some(Duration::from_millis(ms));
    }
    let base = (0.5 * 2_f64.powi(i32::try_from(attempt).unwrap_or(16)).min(16.0)).min(8.0);
    let jitter = 1.0 - rand_factor() * 0.25;
    Some(Duration::from_secs_f64(base * jitter))
}

/// 简化随机源（0..1）：传输层抖动用，非安全敏感。
fn rand_factor() -> f64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    f64::from(nanos % 1000) / 1000.0
}

/// assistant 轮次层：错误文案是否可重试（pi RETRYABLE 模式），配额/账单类排除。
#[must_use]
pub fn assistant_error_retryable(msg: &str) -> bool {
    let m = msg.to_lowercase();
    if NON_RETRYABLE.iter().any(|p| m.contains(p)) {
        return false;
    }
    RETRYABLE.iter().any(|p| m.contains(p))
}

const RETRYABLE: &[&str] = &[
    "overloaded",
    "rate limit",
    "rate_limit",
    "ratelimit",
    "429",
    "500",
    "502",
    "503",
    "504",
    "520",
    "524",
    "network",
    "connection",
    "timeout",
    "timed out",
    "socket hang up",
    "stream ended before message_stop",
];

const NON_RETRYABLE: &[&str] = &[
    "insufficient_quota",
    "billing",
    "usage limit",
    "monthly limit",
    "quota exceeded",
    "invalid_api_key",
    "permission denied",
];

/// assistant 轮次退避：`base·2^(attempt-1)`，上限 `cap`，无抖动（pi 同构）。
#[must_use]
pub fn assistant_backoff(attempt: u32, base_ms: u64, cap_ms: u64) -> Duration {
    let exp = attempt.saturating_sub(1).min(20);
    let ms = base_ms.saturating_mul(1u64 << exp).min(cap_ms);
    Duration::from_millis(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_rules_match_pi() {
        assert!(transport_should_retry(429, None));
        assert!(transport_should_retry(503, None));
        assert!(!transport_should_retry(400, None));
        assert!(!transport_should_retry(429, Some(false)));
        assert!(transport_should_retry(400, Some(true)));
    }

    #[test]
    fn server_delay_cap_fails_fast() {
        assert_eq!(transport_delay(Some(120_000), 0, 60_000), None);
        assert!(transport_delay(Some(1_000), 0, 60_000).is_some());
        assert!(transport_delay(None, 3, 60_000).unwrap() <= Duration::from_secs(8));
    }

    #[test]
    fn assistant_patterns() {
        assert!(assistant_error_retryable("Rate limit exceeded (429)"));
        assert!(assistant_error_retryable("Connection error"));
        assert!(!assistant_error_retryable(
            "insufficient_quota: billing cycle"
        ));
        assert!(!assistant_error_retryable("Invalid schema for tools"));
    }

    #[test]
    fn backoff_caps() {
        assert_eq!(
            assistant_backoff(1, 2000, 60_000),
            Duration::from_millis(2000)
        );
        assert_eq!(
            assistant_backoff(3, 2000, 60_000),
            Duration::from_millis(8000)
        );
        assert_eq!(
            assistant_backoff(10, 2000, 60_000),
            Duration::from_millis(60_000)
        );
    }
}
