//! AgentHandle：run 生命周期、steering/follow-up、取消（pi Agent 类同构的
//! Phase 1 子集——无扩展钩子注册面，见设计 02 §3 占位说明）。

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::error::AgentError;
use crate::events::EventBus;
use crate::queue::PendingMessageQueue;
use crate::run::{LoopConfig, SessionSink, run_agent_loop};

/// 单会话 agent 句柄：装配一次，可多次 run/续跑。
pub struct AgentHandle {
    config: Arc<LoopConfig>,
    bus: EventBus,
    steering: Arc<PendingMessageQueue>,
    follow_up: Arc<PendingMessageQueue>,
    active: tokio::sync::Mutex<()>,
    cancel_slot: std::sync::Mutex<Option<CancellationToken>>,
}

impl AgentHandle {
    /// 装配（配置在 run 之间共享；模型切换经新句柄/新 run）。
    #[must_use]
    pub fn new(config: LoopConfig) -> Self {
        Self {
            config: Arc::new(config),
            bus: EventBus::new(),
            steering: Arc::new(PendingMessageQueue::new()),
            follow_up: Arc::new(PendingMessageQueue::new()),
            active: tokio::sync::Mutex::new(()),
            cancel_slot: std::sync::Mutex::new(None),
        }
    }

    /// 事件总线（订阅在 run 前建立即可收到全程事件）。
    #[must_use]
    pub fn bus(&self) -> EventBus {
        self.bus.clone()
    }

    /// steering 队列（当前轮工具批结束后、下轮请求前注入）。
    #[must_use]
    pub fn steering(&self) -> &PendingMessageQueue {
        &self.steering
    }

    /// follow-up 队列（即将自然停止时注入）。
    #[must_use]
    pub fn follow_up(&self) -> &PendingMessageQueue {
        &self.follow_up
    }

    /// 运行一次：prompt → 循环至自然停止/取消/错误。
    ///
    /// # Errors
    /// 已有 run 进行中（[`AgentError::RunActive`]）；空 prompt 且队列空（
    /// [`AgentError::EmptyPrompt`]）。
    pub async fn run(
        &self,
        prompt: String,
        history: Vec<timo_ai::Message>,
        session: SessionSink,
    ) -> Result<Vec<timo_ai::Message>, AgentError> {
        let _guard = self.active.try_lock().map_err(|_| AgentError::RunActive)?;
        if prompt.trim().is_empty() && !self.has_queued() {
            return Err(AgentError::EmptyPrompt);
        }
        let cancel = CancellationToken::new();
        *self
            .cancel_slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cancel.clone());
        let prompt_messages = if prompt.trim().is_empty() {
            vec![]
        } else {
            vec![timo_ai::user_text(&prompt, self.config.clock.now_ms())]
        };
        let channels = crate::run::RunChannels {
            bus: &self.bus,
            steering: &self.steering,
            follow_up: &self.follow_up,
        };
        let out = run_agent_loop(
            &self.config,
            &channels,
            history,
            prompt_messages,
            session,
            cancel,
        )
        .await;
        *self
            .cancel_slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(out)
    }

    /// 中止当前 run（无 run 则无操作；pi abort 只触发信号不抛错）。
    pub fn abort(&self) {
        if let Some(c) = self
            .cancel_slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            c.cancel();
        }
    }

    /// 是否有排队消息（steering 优先语义的 peek，pi 同构）。
    #[must_use]
    pub fn has_queued(&self) -> bool {
        self.steering.has_queued() || self.follow_up.has_queued()
    }
}
