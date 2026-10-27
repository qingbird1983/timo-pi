//! 缓存预热（phase-3.5/05 §4）：run 收尾后按 TTL 续命前缀缓存，费用划算才发。
//!
//! 任务与内核同生命周期（`Drop` 取消）；会话/模型变化由调用方 `cancel`。
//! 刷新请求原样重放最后一次上下文，`max_tokens = 1`，成功后写
//! `usage(kind=cache_warm)` entry（统计把它当新基线）。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;
use timo_agent::WarmRequest;
use timo_ai::{AuthResolver, Model, ProviderApi, StopReason, StreamOptions, Usage};
use timo_config::{CacheWarmingMode, Clock, ThinkingLevel};
use tokio_util::sync::CancellationToken;

use crate::SessionSink;

/// 经济阈值（USD）：预期节省低于它不发。
pub const SAVINGS_THRESHOLD: f64 = 0.05;
/// streaming 模式安全窗（秒）。
pub const STREAMING_WINDOW_SECS: u64 = 60 * 60;
/// idle 模式安全窗（秒）。
pub const IDLE_WINDOW_SECS: u64 = 30 * 60;

/// 预热状态（`/session` 展示用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarmingState {
    /// 不预热（附原因）。
    Inactive {
        /// 人读原因。
        reason: String,
    },
    /// 已排定。
    Scheduled {
        /// 计划刷新时刻（unix 毫秒）。
        next_warm_at_ms: i64,
    },
    /// 正在刷新。
    Refreshing,
}

/// 一次排定的参数（纯计算，便于单测）。
#[derive(Debug, Clone, PartialEq)]
pub struct WarmPlan {
    /// 距刷新时长。
    pub delay: Duration,
    /// 安全窗（从排定时刻起）。
    pub window: Duration,
    /// 预期节省（USD）。
    pub expected_savings: f64,
}

/// 排定输入。
pub struct ArmInput {
    /// 当前模型（含 TTL 与费率）。
    pub model: Model,
    /// 协议适配器。
    pub api: Arc<dyn ProviderApi>,
    /// 凭证解析器。
    pub auth: Arc<AuthResolver>,
    /// 最后一次请求快照。
    pub request: WarmRequest,
    /// 上一次真实请求的 prompt token。
    pub prompt_tokens: u64,
    /// 当前思考档位。
    pub thinking: ThinkingLevel,
    /// 会话写入口（None = 不落盘）。
    pub session: SessionSink,
    /// 时钟。
    pub clock: Arc<dyn Clock>,
}

/// 计算是否/何时预热；`Err` 是人读的停用原因。
///
/// # Errors
/// 模式关闭、TTL 未知或过短、thinking 未关、无 prompt、费率未知、
/// 预期节省低于阈值 → 返回停用原因。
pub fn plan(
    mode: CacheWarmingMode,
    model: &Model,
    thinking: ThinkingLevel,
    prompt_tokens: u64,
    request_model: &str,
) -> Result<WarmPlan, String> {
    if mode == CacheWarmingMode::Off {
        return Err("warming off".into());
    }
    if request_model != model.id {
        return Err("model changed".into());
    }
    if thinking != ThinkingLevel::Off {
        return Err("thinking is on".into());
    }
    let ttl = model
        .cache_ttl_seconds
        .ok_or_else(|| "model has no cache ttl".to_string())?;
    if ttl <= 10 {
        return Err("cache ttl too short".into());
    }
    if prompt_tokens == 0 {
        return Err("no prompt tokens".into());
    }
    let window = match mode {
        CacheWarmingMode::Streaming => STREAMING_WINDOW_SECS,
        CacheWarmingMode::Idle => IDLE_WINDOW_SECS,
        CacheWarmingMode::Off => unreachable!("checked above"),
    };
    let delay_secs = ttl.saturating_mul(9) / 10;
    let delay_secs = delay_secs.min(ttl - 10).max(1);
    if delay_secs >= window {
        return Err("cache ttl exceeds the warm window".into());
    }

    let prompt = prompt_tokens as f64;
    let per_token = 1.0 / 1_000_000.0;
    let read_rate = model.cost.cache_read * per_token;
    let write_rate = if model.cost.cache_write > 0.0 {
        model.cost.cache_write * per_token
    } else {
        model.cost.input * per_token
    };
    let output_rate = model.cost.output * per_token;
    if read_rate <= 0.0 || write_rate <= 0.0 {
        return Err("pricing unknown".into());
    }
    let warm_cost = prompt * read_rate + output_rate;
    let miss_cost = prompt * write_rate - prompt * read_rate;
    let probability = if mode == CacheWarmingMode::Streaming {
        1.0
    } else {
        0.15
    };
    let expected_savings = probability * miss_cost - warm_cost;
    if miss_cost <= 0.0 || expected_savings < SAVINGS_THRESHOLD {
        return Err(format!(
            "expected savings ${expected_savings:.4} below ${SAVINGS_THRESHOLD:.2}"
        ));
    }
    Ok(WarmPlan {
        delay: Duration::from_secs(delay_secs),
        window: Duration::from_secs(window),
        expected_savings,
    })
}

/// 预热器：持有当前任务与状态。
pub struct CacheWarmer {
    mode: CacheWarmingMode,
    state: Arc<Mutex<WarmingState>>,
    cancel: Mutex<Option<CancellationToken>>,
}

impl CacheWarmer {
    /// 新建（未排定）。
    #[must_use]
    pub fn new(mode: CacheWarmingMode) -> Self {
        Self {
            mode,
            state: Arc::new(Mutex::new(WarmingState::Inactive {
                reason: "not scheduled".into(),
            })),
            cancel: Mutex::new(None),
        }
    }

    /// 当前状态快照。
    #[must_use]
    pub fn status(&self) -> WarmingState {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or(WarmingState::Inactive {
                reason: "status poisoned".into(),
            })
    }

    /// 取消当前排定（会话切换/新 run 开始时调用）。
    pub fn cancel(&self, reason: &str) {
        if let Ok(mut slot) = self.cancel.lock()
            && let Some(token) = slot.take()
        {
            token.cancel();
        }
        self.set_state(WarmingState::Inactive {
            reason: reason.into(),
        });
    }

    /// 按最后一次请求排定刷新；不划算时只更新状态。
    pub fn arm(&self, input: ArmInput) {
        self.cancel("rescheduling");
        let plan = match plan(
            self.mode,
            &input.model,
            input.thinking,
            input.prompt_tokens,
            &input.request.model,
        ) {
            Ok(plan) => plan,
            Err(reason) => {
                self.set_state(WarmingState::Inactive { reason });
                return;
            }
        };
        let next = input.clock.now_ms() + i64::try_from(plan.delay.as_millis()).unwrap_or(0);
        self.set_state(WarmingState::Scheduled {
            next_warm_at_ms: next,
        });
        let token = CancellationToken::new();
        if let Ok(mut slot) = self.cancel.lock() {
            *slot = Some(token.clone());
        }
        let state = Arc::clone(&self.state);
        let mode = self.mode;
        tokio::spawn(async move {
            let started = Instant::now();
            loop {
                tokio::select! {
                    _ = token.cancelled() => return,
                    _ = tokio::time::sleep(plan.delay) => {}
                }
                if token.is_cancelled() {
                    return;
                }
                if started.elapsed() > plan.window {
                    set(
                        &state,
                        WarmingState::Inactive {
                            reason: "missed the refresh window".into(),
                        },
                    );
                    return;
                }
                if let Ok(mut current) = state.lock() {
                    *current = WarmingState::Refreshing;
                }
                match warm_once(&input, &token).await {
                    Ok(usage) => {
                        let prompt = usage.input + usage.cache_read + usage.cache_write;
                        if mode != CacheWarmingMode::Idle || prompt == 0 {
                            set(
                                &state,
                                WarmingState::Inactive {
                                    reason: "warmed".into(),
                                },
                            );
                            return;
                        }
                        let next = input.clock.now_ms()
                            + i64::try_from(plan.delay.as_millis()).unwrap_or(0);
                        set(
                            &state,
                            WarmingState::Scheduled {
                                next_warm_at_ms: next,
                            },
                        );
                    }
                    Err(reason) => {
                        set(&state, WarmingState::Inactive { reason });
                        return;
                    }
                }
            }
        });
    }

    fn set_state(&self, state: WarmingState) {
        set(&self.state, state);
    }
}

impl Drop for CacheWarmer {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.cancel.lock()
            && let Some(token) = slot.take()
        {
            token.cancel();
        }
    }
}

fn set(state: &Arc<Mutex<WarmingState>>, value: WarmingState) {
    if let Ok(mut current) = state.lock() {
        *current = value;
    }
}

/// 发一次 `max_tokens = 1` 的重放请求；成功写 `usage(cache_warm)` entry。
async fn warm_once(input: &ArmInput, token: &CancellationToken) -> Result<Usage, String> {
    if token.is_cancelled() {
        return Err("cancelled".into());
    }
    let api_key = input
        .auth
        .resolve()
        .await
        .map_err(|error| format!("auth: {error}"))?;
    if token.is_cancelled() {
        return Err("cancelled".into());
    }
    let options = StreamOptions {
        api_key: Some(api_key),
        cancel: token.clone(),
        max_tokens: Some(1),
        max_retries: 0,
        ..StreamOptions::default()
    };
    let message = input
        .api
        .stream(&input.model, &input.request.context, options)
        .await
        .result()
        .await;
    if matches!(message.stop_reason, StopReason::Error | StopReason::Aborted) {
        return Err(message
            .error_message
            .clone()
            .unwrap_or_else(|| format!("refresh ended with {:?}", message.stop_reason)));
    }
    if let Some(session) = &input.session {
        let payload = json!({
            "kind": "cache_warm",
            "provider": input.model.provider,
            "model": input.model.id,
            "usage": message.usage,
            "timestamp": input.clock.now_ms(),
        });
        let mut writer = session.lock().await;
        let _ = writer.append_raw(timo_session::kinds::USAGE, payload);
    }
    Ok(message.usage)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[path = "warmer_tests.rs"]
mod tests;
