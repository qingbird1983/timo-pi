//! AgentHandle：run 生命周期、steering/follow-up、取消（pi Agent 类同构的
//! Phase 1 子集——无扩展钩子注册面，见设计 02 §3 占位说明）。

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::error::AgentError;
use crate::events::EventBus;
use crate::queue::PendingMessageQueue;
use crate::run::{LoopConfig, RunOutcome, SessionSink, run_agent_loop};

/// 可从任意任务调用的中止句柄（薄壳在 run 进行中拦截 Ctrl+C 用）。
#[derive(Clone)]
pub struct AbortHandle {
    slot: Arc<std::sync::Mutex<Option<CancellationToken>>>,
}

impl AbortHandle {
    /// 中止当前 run（无 run 则无操作）。
    pub fn abort(&self) {
        if let Some(c) = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            c.cancel();
        }
    }
}

/// 单会话 agent 句柄：装配一次，可多次 run/续跑。
pub struct AgentHandle {
    config: Arc<LoopConfig>,
    bus: EventBus,
    steering: Arc<PendingMessageQueue>,
    follow_up: Arc<PendingMessageQueue>,
    active: tokio::sync::Mutex<()>,
    cancel_slot: Arc<std::sync::Mutex<Option<CancellationToken>>>,
}

impl AgentHandle {
    /// 装配（配置在 run 之间共享；模型切换经新句柄/新 run）。
    #[must_use]
    pub fn new(config: LoopConfig) -> Self {
        Self::with_parts(
            config,
            EventBus::new(),
            Arc::new(PendingMessageQueue::new()),
            Arc::new(PendingMessageQueue::new()),
        )
    }

    /// 用外部总线与队列装配（门面层：先订阅、后装配，保证首事件不丢）。
    #[must_use]
    pub fn with_parts(
        config: LoopConfig,
        bus: EventBus,
        steering: Arc<PendingMessageQueue>,
        follow_up: Arc<PendingMessageQueue>,
    ) -> Self {
        Self {
            config: Arc::new(config),
            bus,
            steering,
            follow_up,
            active: tokio::sync::Mutex::new(()),
            cancel_slot: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// 中止句柄（克隆后可在 run 借出期间从其他任务调用）。
    #[must_use]
    pub fn abort_handle(&self) -> AbortHandle {
        AbortHandle {
            slot: Arc::clone(&self.cancel_slot),
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
    /// 只返回本次新增消息；需要保留多轮上下文的宿主用
    /// [`AgentHandle::run_with_context`]。
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
        let outcome = self.run_with_context(prompt, history, session).await?;
        Ok(outcome.new_messages)
    }

    /// 运行一次并返回新增消息 + 完整上下文（多轮宿主保留 history 用）。
    ///
    /// # Errors
    /// 同 [`AgentHandle::run`]。
    pub async fn run_with_context(
        &self,
        prompt: String,
        history: Vec<timo_ai::Message>,
        session: SessionSink,
    ) -> Result<RunOutcome, AgentError> {
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

    /// 等待当前 run 结束（无 run 时立即返回）。
    pub async fn wait_idle(&self) {
        let _guard = self.active.lock().await;
    }

    /// 是否有排队消息（steering 优先语义的 peek，pi 同构）。
    #[must_use]
    pub fn has_queued(&self) -> bool {
        self.steering.has_queued() || self.follow_up.has_queued()
    }

    /// 对模型声明的工具名清单（门面诊断用）。
    #[must_use]
    pub fn declared_tools(&self) -> Vec<String> {
        self.config
            .tools
            .declared()
            .into_iter()
            .map(|d| d.name.clone())
            .collect()
    }
}
