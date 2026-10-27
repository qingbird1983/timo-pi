//! 用量读数门面（设计 phase-3.5/03 §6）：从当前历史现算。

use super::Kernel;
use crate::usage::{self, UsageBucket, UsageSummary, UsageTotals};

impl Kernel {
    /// 总计 + 分桶。
    #[must_use]
    pub fn usage_summary(&self) -> UsageSummary {
        usage::summarize(self.history.as_deref().unwrap_or(&[]))
    }

    /// 仅总计。
    #[must_use]
    pub fn usage_totals(&self) -> UsageTotals {
        self.usage_summary().totals
    }

    /// 仅分桶（成本降序，同价按 key 升序）。
    #[must_use]
    pub fn usage_breakdown(&self) -> Vec<UsageBucket> {
        self.usage_summary().buckets
    }
}
