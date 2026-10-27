//! 缓存预热门面（phase-3.5/05 §4、§6）：排定/取消/状态。

use std::sync::Arc;

use timo_agent::WarmRequest;

use super::Kernel;
use crate::warmer::{ArmInput, WarmingState};

impl Kernel {
    /// 当前预热状态（`/session` 展示用）。
    #[must_use]
    pub fn cache_warming_status(&self) -> WarmingState {
        self.warmer.status()
    }

    /// run 收尾：按最后一次请求与用量排定刷新。
    pub(super) fn arm_warmer(&self, request: WarmRequest, messages: &[timo_ai::Message]) {
        let prompt_tokens = messages
            .iter()
            .rev()
            .find_map(|message| match message {
                timo_ai::Message::Assistant(assistant) => Some(
                    assistant.usage.input
                        + assistant.usage.cache_read
                        + assistant.usage.cache_write,
                ),
                _ => None,
            })
            .unwrap_or(0);
        self.warmer.arm(ArmInput {
            model: self.resolved.model.clone(),
            api: timo_ai::api_for(self.resolved.model.api),
            auth: Arc::clone(&self.resolved.auth),
            request,
            prompt_tokens,
            thinking: self.resolved.thinking,
            session: self.session.clone(),
            clock: Arc::clone(&self.clock),
        });
    }
}
