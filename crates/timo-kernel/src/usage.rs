//! 用量与成本聚合（设计 phase-3.5/03）：从会话消息现算，不新增落盘格式。

use std::cmp::Ordering;
use std::collections::BTreeMap;

use timo_ai::{Message, Usage};

/// 工具与摘要的固定桶名（与 pi 同名，作为跨实现对照锚点）。
pub const TOOLS_BUCKET: &str = "Tools/summaries";

/// 会话级用量总计。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageTotals {
    /// 输入 token（不含缓存）。
    pub input: u64,
    /// 输出 token（含推理）。
    pub output: u64,
    /// 缓存读 token。
    pub cache_read: u64,
    /// 缓存写 token。
    pub cache_write: u64,
    /// 费用（USD）。
    pub cost: f64,
}

impl UsageTotals {
    /// 四项 token 之和。
    #[must_use]
    pub fn tokens(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
}

/// 按模型（或工具/摘要）分桶的用量。
#[derive(Debug, Clone, PartialEq)]
pub struct UsageBucket {
    /// `provider/model` 或 [`TOOLS_BUCKET`]。
    pub key: String,
    /// 四项 token 之和。
    pub tokens: u64,
    /// 费用（USD）。
    pub cost: f64,
}

/// 总计 + 分桶（桶按成本降序，同价按 key 升序）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageSummary {
    /// 全量总计。
    pub totals: UsageTotals,
    /// 分桶。
    pub buckets: Vec<UsageBucket>,
}

/// 从消息历史聚合用量；成本取消息内的 `usage.cost.total`（适配器按费率写入）。
pub(crate) fn summarize(messages: &[Message]) -> UsageSummary {
    let mut totals = UsageTotals::default();
    let mut buckets: BTreeMap<String, UsageBucket> = BTreeMap::new();

    for message in messages {
        match message {
            Message::Assistant(assistant) => {
                let usage = &assistant.usage;
                add_totals(&mut totals, usage);
                let model = assistant
                    .response_model
                    .as_deref()
                    .unwrap_or(&assistant.model);
                let key = format!("{}/{}", assistant.provider, model);
                add_bucket(&mut buckets, key, usage);
            }
            Message::ToolResult(result) => {
                if let Some(usage) = &result.usage {
                    add_totals(&mut totals, usage);
                    add_bucket(&mut buckets, TOOLS_BUCKET.to_string(), usage);
                }
            }
            Message::User(_) | Message::System(_) => {}
        }
    }

    let mut buckets: Vec<UsageBucket> = buckets
        .into_values()
        .filter(|bucket| bucket.cost > 0.0 || bucket.tokens > 0)
        .collect();
    buckets.sort_by(|left, right| {
        right
            .cost
            .partial_cmp(&left.cost)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.key.cmp(&right.key))
    });

    UsageSummary { totals, buckets }
}

fn add_totals(totals: &mut UsageTotals, usage: &Usage) {
    totals.input += usage.input;
    totals.output += usage.output;
    totals.cache_read += usage.cache_read;
    totals.cache_write += usage.cache_write;
    totals.cost += usage.cost.total;
}

fn add_bucket(buckets: &mut BTreeMap<String, UsageBucket>, key: String, usage: &Usage) {
    let bucket = buckets.entry(key.clone()).or_insert_with(|| UsageBucket {
        key,
        tokens: 0,
        cost: 0.0,
    });
    bucket.tokens += usage.input + usage.output + usage.cache_read + usage.cache_write;
    bucket.cost += usage.cost.total;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use timo_ai::{AssistantMessage, Cost, ToolResultMessage};

    fn assistant(provider: &str, model: &str, usage: Usage) -> Message {
        let mut message = AssistantMessage::pending("openai-completions", provider, model, 0);
        message.usage = usage;
        Message::Assistant(message)
    }

    fn usage(input: u64, output: u64, cost: f64) -> Usage {
        Usage {
            input,
            output,
            total_tokens: input + output,
            cost: Cost {
                total: cost,
                ..Cost::default()
            },
            ..Usage::default()
        }
    }

    fn tool_result(usage: Usage) -> Message {
        Message::ToolResult(ToolResultMessage {
            tool_call_id: "c1".into(),
            tool_name: "read".into(),
            content: vec![],
            is_error: false,
            usage: Some(usage),
            duration_ms: Some(1),
            timestamp: 0,
        })
    }

    #[test]
    fn totals_and_buckets_follow_models() {
        let summary = summarize(&[
            assistant("deepseek", "deepseek-chat", usage(100, 50, 0.002)),
            assistant("anthropic", "claude-sonnet-4-5", usage(200, 80, 0.010)),
        ]);
        assert_eq!(summary.totals.input, 300);
        assert_eq!(summary.totals.output, 130);
        assert_eq!(summary.totals.tokens(), 430);
        assert!((summary.totals.cost - 0.012).abs() < 1e-9);
        let keys: Vec<&str> = summary.buckets.iter().map(|b| b.key.as_str()).collect();
        assert_eq!(
            keys,
            ["anthropic/claude-sonnet-4-5", "deepseek/deepseek-chat"]
        );
    }

    #[test]
    fn tool_usage_goes_to_the_tools_bucket() {
        let summary = summarize(&[tool_result(usage(10, 0, 0.0))]);
        assert_eq!(summary.totals.input, 10);
        assert_eq!(summary.buckets.len(), 1);
        assert_eq!(summary.buckets[0].key, TOOLS_BUCKET);
        assert_eq!(summary.buckets[0].tokens, 10);
    }

    #[test]
    fn zero_usage_messages_produce_no_bucket() {
        let summary = summarize(&[assistant("p", "m", Usage::default())]);
        assert_eq!(summary.totals, UsageTotals::default());
        assert!(summary.buckets.is_empty());
    }

    #[test]
    fn response_model_wins_as_bucket_key() {
        let mut message = AssistantMessage::pending("openai-completions", "p", "requested", 0);
        message.response_model = Some("actual".into());
        message.usage = usage(1, 1, 0.0);
        let summary = summarize(&[Message::Assistant(message)]);
        assert_eq!(summary.buckets[0].key, "p/actual");
    }

    #[test]
    fn equal_cost_is_sorted_by_key() {
        let summary = summarize(&[
            assistant("b", "x", usage(1, 0, 0.001)),
            assistant("a", "x", usage(1, 0, 0.001)),
        ]);
        let keys: Vec<&str> = summary.buckets.iter().map(|b| b.key.as_str()).collect();
        assert_eq!(keys, ["a/x", "b/x"]);
    }

    #[test]
    fn empty_history_is_zero() {
        let summary = summarize(&[]);
        assert_eq!(summary.totals, UsageTotals::default());
        assert!(summary.buckets.is_empty());
    }
}
