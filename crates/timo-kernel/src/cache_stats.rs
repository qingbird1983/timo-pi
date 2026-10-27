//! 缓存命中统计（phase-3.5/05 §3）：从会话 entry 现算"本应命中却被重新计费"的 token。
//!
//! 输入只有三类 entry：assistant `message`（候选请求）、`session.compaction`
//! （上下文合法变化，重置基线）、`usage(kind=cache_warm)`（预热请求刷新基线）。
//! 判定与代价公式对齐 pi（噪声地板 1024）。

use timo_ai::{Message, Usage};
use timo_session::{Entry, kinds};

/// 一次判定为 miss 的请求。
#[derive(Debug, Clone, PartialEq)]
pub struct CacheMiss {
    /// 重新计费的 token。
    pub missed_tokens: u64,
    /// 重新计费金额（USD）。
    pub missed_cost: f64,
    /// 与上一条请求的间隔（毫秒）。
    pub idle_ms: u64,
    /// 两条请求的 `provider/model` 是否不同。
    pub model_changed: bool,
}

/// 会话级缓存统计。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CacheStats {
    /// 重新计费 token 合计。
    pub missed_tokens: u64,
    /// 重新计费金额合计（USD）。
    pub missed_cost: f64,
    /// miss 次数。
    pub miss_count: usize,
    /// 逐次明细（会话顺序）。
    pub misses: Vec<CacheMiss>,
}

/// 噪声地板：缓存断点对齐造成的零头不计。
pub const NOISE_FLOOR: u64 = 1024;

#[derive(Debug, Clone)]
struct Baseline {
    prompt_tokens: u64,
    model_key: String,
    timestamp: i64,
    reported_cache: bool,
}

/// 从 entry 列表现算统计；`fallback_read_rate` 是目录里的缓存读单价（USD/token）。
#[must_use]
pub fn from_entries(entries: &[Entry], fallback_read_rate: f64) -> CacheStats {
    let mut stats = CacheStats::default();
    let mut baseline: Option<Baseline> = None;
    for entry in entries {
        if entry.kind == kinds::COMPACTION {
            baseline = None;
            continue;
        }
        if entry.kind == kinds::USAGE && is_cache_warm(&entry.payload) {
            if let Some(next) = warm_baseline(entry) {
                baseline = Some(next);
            }
            continue;
        }
        if entry.kind != kinds::MESSAGE {
            continue;
        }
        let Ok(Message::Assistant(assistant)) =
            serde_json::from_value::<Message>(entry.payload.clone())
        else {
            continue;
        };
        let prompt =
            assistant.usage.input + assistant.usage.cache_read + assistant.usage.cache_write;
        if prompt == 0 {
            continue;
        }
        let model_key = format!("{}/{}", assistant.provider, assistant.model);
        let reported = assistant.usage.cache_read + assistant.usage.cache_write > 0;
        if let Some(prev) = &baseline
            && (reported || prev.reported_cache)
        {
            let missed = prev
                .prompt_tokens
                .min(prompt)
                .saturating_sub(assistant.usage.cache_read);
            if missed > NOISE_FLOOR {
                let cost = missed_cost(&assistant.usage, missed, fallback_read_rate);
                stats.missed_tokens += missed;
                stats.missed_cost += cost;
                stats.miss_count += 1;
                stats.misses.push(CacheMiss {
                    missed_tokens: missed,
                    missed_cost: cost,
                    idle_ms: u64::try_from(assistant.timestamp - prev.timestamp).unwrap_or(0),
                    model_changed: model_key != prev.model_key,
                });
            }
        }
        baseline = Some(Baseline {
            prompt_tokens: prompt,
            model_key,
            timestamp: assistant.timestamp,
            reported_cache: reported,
        });
    }
    stats
}

fn is_cache_warm(payload: &serde_json::Value) -> bool {
    payload.get("kind").and_then(serde_json::Value::as_str) == Some("cache_warm")
}

fn warm_baseline(entry: &Entry) -> Option<Baseline> {
    let usage: Usage = serde_json::from_value(entry.payload.get("usage")?.clone()).ok()?;
    let prompt = usage.input + usage.cache_read + usage.cache_write;
    if prompt == 0 {
        return None;
    }
    let provider = entry.payload.get("provider")?.as_str()?;
    let model = entry.payload.get("model")?.as_str()?;
    Some(Baseline {
        prompt_tokens: prompt,
        model_key: format!("{provider}/{model}"),
        timestamp: entry
            .payload
            .get("timestamp")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0),
        reported_cache: usage.cache_read + usage.cache_write > 0,
    })
}

/// 重新计费金额：`missed × max(0, 实付单价 − 缓存读单价)`。
fn missed_cost(usage: &Usage, missed: u64, fallback_read_rate: f64) -> f64 {
    let paid_units = usage.input + usage.cache_write;
    let paid_rate = if paid_units > 0 {
        (usage.cost.input + usage.cost.cache_write) / paid_units as f64
    } else {
        fallback_read_rate
    };
    let read_rate = if usage.cache_read > 0 && usage.cost.cache_read > 0.0 {
        usage.cost.cache_read / usage.cache_read as f64
    } else {
        fallback_read_rate
    };
    missed as f64 * (paid_rate - read_rate).max(0.0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use serde_json::json;
    use timo_ai::{AssistantMessage, Cost, StopReason};

    fn assistant(input: u64, read: u64, write: u64, ts: i64, model: &str) -> AssistantMessage {
        let mut message = AssistantMessage::pending("anthropic-messages", "anthropic", model, ts);
        message.stop_reason = StopReason::Stop;
        message.usage = Usage {
            input,
            output: 1,
            cache_read: read,
            cache_write: write,
            total_tokens: input + 1 + read + write,
            cost: Cost {
                input: input as f64 * 10.0 / 1_000_000.0,
                output: 50.0 / 1_000_000.0,
                cache_read: read as f64 / 1_000_000.0,
                cache_write: write as f64 * 12.5 / 1_000_000.0,
                total: 0.0,
            },
            ..Usage::default()
        };
        message
    }

    fn message_entry(id: usize, message: AssistantMessage) -> Entry {
        Entry {
            v: 1,
            id: format!("M{id}"),
            ts: "2026-01-01T00:00:00.000Z".into(),
            parent_id: None,
            kind: kinds::MESSAGE.into(),
            payload: serde_json::to_value(Message::Assistant(message)).unwrap(),
        }
    }

    fn warm_entry(ts: i64) -> Entry {
        Entry {
            v: 1,
            id: "W".into(),
            ts: "2026-01-01T00:00:00.000Z".into(),
            parent_id: None,
            kind: kinds::USAGE.into(),
            payload: json!({
                "kind": "cache_warm",
                "provider": "anthropic",
                "model": "claude-opus-4-6",
                "usage": {"input": 20000, "output": 1, "cache_read": 0, "cache_write": 0, "total_tokens": 20001},
                "timestamp": ts,
            }),
        }
    }

    #[test]
    fn first_request_and_unreported_cache_do_not_count() {
        let entries = vec![message_entry(1, assistant(20000, 0, 0, 0, "m"))];
        assert_eq!(from_entries(&entries, 1e-6), CacheStats::default());

        let entries = vec![
            message_entry(1, assistant(20000, 0, 0, 0, "m")),
            message_entry(2, assistant(19000, 0, 0, 1000, "m")),
        ];
        assert_eq!(from_entries(&entries, 1e-6), CacheStats::default());
    }

    #[test]
    fn noise_floor_ignores_small_gaps() {
        let entries = vec![
            message_entry(1, assistant(20000, 500, 0, 0, "m")),
            message_entry(2, assistant(1000, 19000, 0, 1000, "m")),
        ];
        assert_eq!(from_entries(&entries, 1e-6), CacheStats::default());
    }

    #[test]
    fn real_miss_counts_tokens_and_cost() {
        let entries = vec![
            message_entry(1, assistant(20000, 500, 0, 0, "m")),
            message_entry(2, assistant(18500, 1500, 0, 7_000, "m")),
        ];
        let stats = from_entries(&entries, 1e-6);
        assert_eq!(stats.miss_count, 1);
        assert_eq!(stats.missed_tokens, 18500);
        let miss = &stats.misses[0];
        assert_eq!(miss.idle_ms, 7_000);
        assert!(!miss.model_changed);
        // paid=(0.185+0)/18500=1e-5; read=1.5e-6; 18500×9e-6=0.1665
        assert!((miss.missed_cost - 0.1665).abs() < 1e-9, "{miss:?}");
    }

    #[test]
    fn compaction_resets_the_baseline() {
        let mut entries = vec![
            message_entry(1, assistant(20000, 500, 0, 0, "m")),
            Entry {
                v: 1,
                id: "C".into(),
                ts: "t".into(),
                parent_id: None,
                kind: kinds::COMPACTION.into(),
                payload: json!({"summary": "s", "keep_recent_messages": 0}),
            },
            message_entry(2, assistant(20000, 0, 0, 1000, "m")),
        ];
        // 压缩后的下一轮不再把摘要变化误判成 miss
        assert_eq!(from_entries(&entries, 1e-6), CacheStats::default());
        entries.push(message_entry(3, assistant(19000, 1500, 0, 2000, "m")));
        // 再下一轮才是真实断档：基线在压缩后重新建立
        let stats = from_entries(&entries, 1e-6);
        assert_eq!(stats.miss_count, 1);
        assert_eq!(stats.missed_tokens, 18500);
    }

    #[test]
    fn cache_warm_entry_refreshes_the_baseline() {
        let entries = vec![
            message_entry(1, assistant(20000, 500, 0, 0, "m")),
            warm_entry(1_000),
            message_entry(2, assistant(4000, 15000, 0, 2_000, "m")),
        ];
        let stats = from_entries(&entries, 1e-6);
        assert_eq!(stats.miss_count, 1);
        assert_eq!(stats.missed_tokens, 4000);
        assert_eq!(stats.misses[0].idle_ms, 1_000);
    }

    #[test]
    fn model_switch_is_reported_but_still_counted() {
        let entries = vec![
            message_entry(1, assistant(20000, 500, 0, 0, "a")),
            message_entry(2, assistant(18500, 1500, 0, 5, "b")),
        ];
        let stats = from_entries(&entries, 1e-6);
        assert_eq!(stats.miss_count, 1);
        assert!(stats.misses[0].model_changed);
    }
}
