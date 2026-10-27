//! 单次请求与压缩步骤（设计 02 §4.c/d、10 §4.2）。
//!
//! 从 `run/mod.rs` 拆出以守住单文件行数预算（标准 08 §4）。

use timo_ai::{AssistantMessage, StopReason};

use crate::events::{CompactionReason, KernelEvent};
use crate::hooks::RouteReason;

use super::Ctx;

impl Ctx<'_> {
    /// 请求前按阈值压缩上下文（phase-3.5/04）；失败只发事件，不中断。
    ///
    /// 阈值按本轮请求模型（已路由的物理模型）的窗口判定；摘要请求按
    /// `Direct` 单独路由（设计 10 §4.2）。
    async fn maybe_compact(&mut self) {
        let Some(settings) = self.config.compaction.clone() else {
            return;
        };
        if !settings.enabled || self.cancel.is_cancelled() {
            return;
        }
        if self.model.is_virtual() {
            return; // 未路由的虚拟条目没有窗口口径（请求会被适配器拒绝）
        }
        let tokens = crate::compaction::context_tokens(&self.messages);
        if !crate::compaction::should_compact(tokens, self.model.context_window, &settings) {
            return;
        }
        let (summary_model, summary_thinking) =
            match self.route_choice(RouteReason::Direct, None).await {
                Ok(Some((model, thinking, _))) => (model, thinking),
                Ok(None) => (self.model.clone(), self.thinking_level),
                Err(error) => {
                    self.bus.emit(KernelEvent::Error {
                        context: format!("compaction route: {error}"),
                    });
                    return;
                }
            };
        if summary_model.is_virtual() {
            return;
        }
        let api = self.api_for(&summary_model);
        let auth = self.auth_for(&summary_model);
        let input = crate::compaction::CompactInput {
            messages: &self.messages,
            model: &summary_model,
            api: &api,
            auth: &auth,
            retry: &self.config.retry,
            thinking: summary_thinking,
            settings: &settings,
            cancel: &self.cancel,
            clock: &self.config.clock,
            instructions: None,
        };
        match crate::compaction::compact(input).await {
            Ok(outcome) => {
                if let Some(sink) = &self.session {
                    let mut writer = sink.lock().await;
                    let _ = writer.append_raw(
                        timo_session::kinds::COMPACTION,
                        outcome.entry_payload(CompactionReason::Threshold),
                    );
                }
                self.messages = outcome.messages;
                self.bus.emit(KernelEvent::Compacted {
                    reason: CompactionReason::Threshold,
                    tokens_before: outcome.tokens_before,
                    tokens_after: outcome.tokens_after,
                });
            }
            Err(crate::compaction::CompactionError::Nothing) => {}
            Err(error) => self.bus.emit(KernelEvent::Error {
                context: format!("compaction: {error}"),
            }),
        }
    }

    /// 一次模型请求（含 assistant 轮次重试；设计 02 §4.c/d）。
    ///
    /// 每次请求先路由（`User`/`Continuation`，重试为 `Retry`）；路由结果只在
    /// 本次请求生效，选中模型在 run 内保持不变（设计 10 §4.2）。
    pub(super) async fn request(&mut self) -> AssistantMessage {
        let mut attempt: u32 = 0;
        let mut failed: Option<(timo_ai::Model, timo_config::ThinkingLevel)> = None;
        loop {
            let reason = if failed.is_some() {
                RouteReason::Retry
            } else {
                self.turn_reason()
            };
            let failed_ref = failed.as_ref().map(|(model, level)| (model, *level));
            match self.route_choice(reason, failed_ref).await {
                Ok(Some((model, thinking, state))) => {
                    self.model = model;
                    self.thinking_level = thinking;
                    if let Some(state) = state
                        && self.route_state.as_ref() != Some(&state)
                    {
                        // 先落会话再发请求（蓝本同序）。`Direct` 不经过这里：
                        // 压缩摘要只取路由结果，不落状态（设计 10 §4.2）。
                        if self.selection.is_virtual() {
                            self.write_virtual_state(&state).await;
                        }
                        self.store_route_state(state);
                    }
                }
                Ok(None) => {
                    self.model = self.selection.clone();
                    self.thinking_level = self.selected_thinking;
                }
                Err(error) => {
                    // 路由失败：消息仍记选中的（虚拟）模型名（T8）。
                    self.model = self.selection.clone();
                    self.thinking_level = self.selected_thinking;
                    return self.error_terminal(&error, false);
                }
            }
            if attempt == 0 {
                self.maybe_compact().await;
            }
            self.bus.emit(KernelEvent::ProviderRequest {
                provider: self.model.provider.clone(),
                model: self.model.id.clone(),
            });
            let mut message = self.request_once().await;
            // 记录本次请求实际使用的档位（路由的 previous 依赖它）。
            if message.thinking_level.is_none() {
                message.thinking_level = Some(self.thinking_level);
            }
            self.bus.emit(KernelEvent::ProviderResponse {
                provider: self.model.provider.clone(),
                model: self.model.id.clone(),
                usage: message.usage.clone(),
            });
            let retryable = message.stop_reason == StopReason::Error
                && self.config.retry.enabled
                && attempt < self.config.retry.max_retries
                && message
                    .error_message
                    .as_deref()
                    .map(timo_ai::retry::assistant_error_retryable)
                    .unwrap_or(false);
            if !retryable {
                return message;
            }
            failed = Some((self.model.clone(), self.thinking_level));
            attempt += 1;
            let delay = timo_ai::retry::assistant_backoff(
                attempt,
                self.config.retry.base_delay_ms,
                self.config.retry.max_retry_delay_ms,
            );
            tokio::select! {
                _ = self.cancel.cancelled() => return message,
                _ = tokio::time::sleep(delay) => {}
            }
        }
    }

    /// 把新状态写进共享槽（下一次 run 直接读到；设计 10 §4.2）。
    fn store_route_state(&mut self, state: serde_json::Value) {
        self.route_state = Some(state.clone());
        if let Ok(mut slot) = self.config.route_state.lock() {
            *slot = Some(state);
        }
    }

    /// 追加 `session.virtual_model_state`（只增不改）。
    async fn write_virtual_state(&self, state: &serde_json::Value) {
        let Some(sink) = &self.session else {
            return;
        };
        let payload = serde_json::json!({
            "provider": self.selection.provider,
            "model_id": self.selection.id,
            "state": state,
        });
        let mut writer = sink.lock().await;
        let _ = writer.append_raw(timo_session::kinds::VIRTUAL_MODEL_STATE, payload);
    }
}
