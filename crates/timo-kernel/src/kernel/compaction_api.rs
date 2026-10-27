//! 手动压缩与设置快照（phase-3.5/04 §9）。

use tokio_util::sync::CancellationToken;

use timo_agent::compaction::{CompactInput, CompactionOutcome};
use timo_agent::{CompactionReason, KernelEvent};

use super::Kernel;
use crate::Result;
use crate::error::KernelError;

impl Kernel {
    /// 当前压缩设置（只读快照）。
    #[must_use]
    pub fn compaction_settings(&self) -> &timo_config::CompactionSettings {
        &self.compaction
    }

    /// 手动压缩当前会话（`/compact`）；草稿会话报错。
    ///
    /// # Errors
    /// 无会话/无可摘要内容/摘要请求失败 → [`KernelError`]。
    pub async fn compact(&mut self, instructions: Option<&str>) -> Result<CompactionOutcome> {
        if self.session.is_none() {
            return Err(KernelError::Unavailable(
                "no saved session to compact".into(),
            ));
        }
        let messages = self.history.clone().unwrap_or_default();
        let api = timo_ai::api_for(self.resolved.model.api);
        let cancel = CancellationToken::new();
        let outcome = timo_agent::compaction::compact(CompactInput {
            messages: &messages,
            model: &self.resolved.model,
            api: &api,
            auth: &self.resolved.auth,
            retry: &self.retry,
            thinking: self.resolved.thinking,
            settings: &self.compaction,
            cancel: &cancel,
            clock: &self.clock,
            instructions,
        })
        .await
        .map_err(KernelError::from)?;

        if let Some(session) = &self.session {
            let mut writer = session.lock().await;
            writer.append_raw(
                timo_session::kinds::COMPACTION,
                outcome.entry_payload(CompactionReason::Manual),
            )?;
        }
        self.history = Some(outcome.messages.clone());
        self.bus.emit(KernelEvent::Compacted {
            reason: CompactionReason::Manual,
            tokens_before: outcome.tokens_before,
            tokens_after: outcome.tokens_after,
        });
        Ok(outcome)
    }
}
