use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use timo_ai::{
    AssistantMessage, AssistantMessageStream, AssistantStreamEvent, Compat, Context, RateCard,
    StopReason,
};
use timo_config::FixedClock;

const MODEL_ID: &str = "claude-test";

fn model(ttl: Option<u64>) -> Model {
    Model {
        id: MODEL_ID.into(),
        name: "Test".into(),
        provider: "anthropic".into(),
        api: timo_ai::ApiKind::AnthropicMessages,
        base_url: "http://localhost".into(),
        reasoning: false,
        input: vec!["text".into()],
        cost: RateCard {
            input: 10.0,
            output: 50.0,
            cache_read: 0.25,
            cache_write: 12.5,
            tiers: vec![],
        },
        context_window: 200_000,
        max_tokens: 4096,
        headers: vec![],
        cache_ttl_seconds: ttl,
        compat: Compat::default(),
    }
}

fn request() -> WarmRequest {
    WarmRequest {
        model: MODEL_ID.into(),
        context: Context::default(),
    }
}

/// 记录请求次数并返回固定用量的假适配器。
struct FakeApi {
    calls: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl ProviderApi for FakeApi {
    async fn stream(
        &self,
        _model: &Model,
        _context: &Context,
        _options: StreamOptions,
    ) -> AssistantMessageStream {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let mut message = AssistantMessage::pending("anthropic-messages", "anthropic", MODEL_ID, 1);
        message.stop_reason = StopReason::Stop;
        message.usage.cache_read = 20_000;
        message.usage.total_tokens = 20_001;
        tokio::spawn(async move {
            let _ = tx
                .send(AssistantStreamEvent::Done {
                    reason: StopReason::Stop,
                    message,
                })
                .await;
        });
        AssistantMessageStream::new(rx)
    }
}

fn auth() -> Arc<AuthResolver> {
    Arc::new(AuthResolver {
        explicit: Some("test-key".into()),
        provider_ref: None,
        env_names: vec![],
    })
}

#[test]
fn plan_rejects_unusable_settings() {
    assert!(
        plan(
            CacheWarmingMode::Off,
            &model(Some(300)),
            ThinkingLevel::Off,
            1000,
            MODEL_ID
        )
        .unwrap_err()
        .contains("off")
    );
    assert!(
        plan(
            CacheWarmingMode::Streaming,
            &model(None),
            ThinkingLevel::Off,
            1000,
            MODEL_ID
        )
        .unwrap_err()
        .contains("ttl")
    );
    assert!(
        plan(
            CacheWarmingMode::Streaming,
            &model(Some(5)),
            ThinkingLevel::Off,
            1000,
            MODEL_ID
        )
        .unwrap_err()
        .contains("short")
    );
    assert!(
        plan(
            CacheWarmingMode::Streaming,
            &model(Some(300)),
            ThinkingLevel::Medium,
            1000,
            MODEL_ID
        )
        .unwrap_err()
        .contains("thinking")
    );
    assert!(
        plan(
            CacheWarmingMode::Streaming,
            &model(Some(300)),
            ThinkingLevel::Off,
            0,
            MODEL_ID
        )
        .unwrap_err()
        .contains("prompt")
    );
    assert!(
        plan(
            CacheWarmingMode::Streaming,
            &model(Some(300)),
            ThinkingLevel::Off,
            1000,
            "other"
        )
        .unwrap_err()
        .contains("model changed")
    );
}

#[test]
fn plan_delay_and_economics() {
    let streaming = plan(
        CacheWarmingMode::Streaming,
        &model(Some(300)),
        ThinkingLevel::Off,
        20_000,
        MODEL_ID,
    )
    .unwrap();
    assert_eq!(streaming.delay, Duration::from_secs(270));
    assert_eq!(streaming.window, Duration::from_secs(STREAMING_WINDOW_SECS));
    // warm=0.005+0.00005; miss=0.245 → 0.23995
    assert!((streaming.expected_savings - 0.23995).abs() < 1e-9);

    // 11 秒 TTL：刷新延迟压到 1 秒
    let short = plan(
        CacheWarmingMode::Streaming,
        &model(Some(11)),
        ThinkingLevel::Off,
        20_000,
        MODEL_ID,
    )
    .unwrap();
    assert_eq!(short.delay, Duration::from_secs(1));

    // idle 概率 0.15：2 万 prompt 不划算，4 万才过阈值
    assert!(
        plan(
            CacheWarmingMode::Idle,
            &model(Some(300)),
            ThinkingLevel::Off,
            20_000,
            MODEL_ID
        )
        .is_err()
    );
    let idle = plan(
        CacheWarmingMode::Idle,
        &model(Some(300)),
        ThinkingLevel::Off,
        40_000,
        MODEL_ID,
    )
    .unwrap();
    assert_eq!(idle.window, Duration::from_secs(IDLE_WINDOW_SECS));
}

fn sink(name: &str) -> (std::path::PathBuf, SessionSink) {
    let dir = std::env::temp_dir().join(format!("timo-warmer-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let writer = timo_session::SessionWriter::create(
        &dir,
        "D:/work",
        None,
        Arc::new(FixedClock::new(1)),
        Arc::new(timo_session::IdGen::new()),
    )
    .unwrap();
    let path = writer.path().to_path_buf();
    (path, Some(Arc::new(tokio::sync::Mutex::new(writer))))
}

fn arm(warmer: &CacheWarmer, api: Arc<dyn ProviderApi>, session: SessionSink) {
    warmer.arm(ArmInput {
        model: model(Some(11)),
        api,
        auth: auth(),
        request: request(),
        prompt_tokens: 20_000,
        thinking: ThinkingLevel::Off,
        session,
        clock: Arc::new(FixedClock::new(1_000)),
    });
}

#[tokio::test]
async fn streaming_mode_refreshes_once_and_writes_usage() {
    let (path, session) = sink("once");
    let calls = Arc::new(AtomicUsize::new(0));
    let warmer = CacheWarmer::new(CacheWarmingMode::Streaming);
    arm(
        &warmer,
        Arc::new(FakeApi {
            calls: calls.clone(),
        }),
        session,
    );
    assert!(matches!(warmer.status(), WarmingState::Scheduled { .. }));

    // 1 秒延迟 + 收尾；最多等 5 秒
    for _ in 0..50 {
        if calls.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1, "exactly one refresh");
    for _ in 0..20 {
        if matches!(warmer.status(), WarmingState::Inactive { .. }) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        warmer.status(),
        WarmingState::Inactive {
            reason: "warmed".into()
        }
    );
    let entries = timo_session::read_entries(&path).unwrap();
    let warm = entries
        .iter()
        .find(|entry| entry.kind == timo_session::kinds::USAGE)
        .expect("usage entry written");
    assert_eq!(warm.payload["kind"], "cache_warm");
    assert_eq!(warm.payload["provider"], "anthropic");
    assert_eq!(warm.payload["timestamp"], 1_000);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[tokio::test]
async fn cancel_stops_the_pending_refresh() {
    let (path, session) = sink("cancel");
    let calls = Arc::new(AtomicUsize::new(0));
    let warmer = CacheWarmer::new(CacheWarmingMode::Streaming);
    arm(
        &warmer,
        Arc::new(FakeApi {
            calls: calls.clone(),
        }),
        session,
    );
    warmer.cancel("session changed");
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        warmer.status(),
        WarmingState::Inactive {
            reason: "session changed".into()
        }
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// 请求已发出后才完成的假适配器：用于取消竞态。
struct SlowApi {
    calls: Arc<AtomicUsize>,
    gate: Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait]
impl ProviderApi for SlowApi {
    async fn stream(
        &self,
        _model: &Model,
        _context: &Context,
        _options: StreamOptions,
    ) -> AssistantMessageStream {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.gate.notified().await;
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let mut message = AssistantMessage::pending("anthropic-messages", "anthropic", MODEL_ID, 1);
        message.stop_reason = StopReason::Stop;
        message.usage.cache_read = 20_000;
        message.usage.total_tokens = 20_001;
        tokio::spawn(async move {
            let _ = tx
                .send(AssistantStreamEvent::Done {
                    reason: StopReason::Stop,
                    message,
                })
                .await;
        });
        AssistantMessageStream::new(rx)
    }
}

#[tokio::test]
async fn cancelled_in_flight_refresh_writes_no_baseline() {
    let (path, session) = sink("inflight");
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(tokio::sync::Notify::new());
    let warmer = CacheWarmer::new(CacheWarmingMode::Streaming);
    arm(
        &warmer,
        Arc::new(SlowApi {
            calls: calls.clone(),
            gate: Arc::clone(&gate),
        }),
        session,
    );
    for _ in 0..50 {
        if calls.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    warmer.cancel("new run");
    gate.notify_waiters();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let entries = timo_session::read_entries(&path).unwrap();
    assert!(
        entries
            .iter()
            .all(|entry| entry.kind != timo_session::kinds::USAGE),
        "cancelled refresh must not write a baseline"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
