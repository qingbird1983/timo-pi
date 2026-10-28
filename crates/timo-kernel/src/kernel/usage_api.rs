//! 用量读数门面（设计 phase-3.5/03 §6）：已落盘会话从 entry 现算，
//! 草稿会话退回内存历史（压缩后仍保留压缩前的记账）。

use super::Kernel;
use crate::usage::{self, UsageSummary};

impl Kernel {
    /// 总计 + 分桶。
    #[must_use]
    pub fn usage_summary(&self) -> UsageSummary {
        usage::summarize(&self.usage_messages())
    }

    fn usage_messages(&self) -> Vec<timo_ai::Message> {
        if let Some(id) = &self.session_id
            && let Some(path) = self.find_session_path(id)
            && let Ok(entries) = timo_session::read_entries(&path)
        {
            return entries
                .iter()
                .filter(|entry| entry.kind == timo_session::kinds::MESSAGE)
                .filter_map(|entry| serde_json::from_value(entry.payload.clone()).ok())
                .collect();
        }
        self.history.clone().unwrap_or_default()
    }
}
