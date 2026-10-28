#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use timo_kernel::TrustScopeChoice;

fn request() -> TrustRequest {
    TrustRequest {
        cwd: PathBuf::from("/work/demo"),
        resources: vec![],
        saved: None,
    }
}

/// 桥接把询问转成事件，并把 UI 的选择原样回传。
#[tokio::test]
async fn trust_broker_round_trips_choice() {
    let (events, mut rx) = unbounded_channel();
    let broker = TrustBroker { events };
    let asking = tokio::spawn(async move { broker.ask(request()).await });
    match rx.recv().await.unwrap() {
        BridgeEvent::Trust { respond, .. } => {
            let _ = respond.send(Some(TrustChoice {
                trusted: true,
                scope: TrustScopeChoice::Directory,
            }));
        }
        other => panic!("expected trust request, got {other:?}"),
    }
    let choice = asking.await.unwrap().expect("choice");
    assert!(choice.trusted);
    assert_eq!(choice.scope, TrustScopeChoice::Directory);
}

/// UI 通道断开（窗口关闭）→ `None`，由内核按"不信任（本次）"处理。
#[tokio::test]
async fn trust_broker_returns_none_when_window_gone() {
    let (events, rx) = unbounded_channel();
    drop(rx);
    let broker = TrustBroker { events };
    assert!(broker.ask(request()).await.is_none());
}

/// 真实内核：桥接先收到信任询问，回答后先发结论再发 `Ready`（设计 12 §3.4/§3.5）。
#[tokio::test]
async fn spawn_asks_trust_then_reports_before_ready() {
    use crate::settings::ReviewPrefs;
    use std::time::Duration;

    let base = std::env::temp_dir().join(format!(
        "timo-desktop-trust-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(base.join("work/.timo")).unwrap();
    std::fs::write(base.join("work/.timo/SYSTEM.md"), "secret").unwrap();

    let mut bridge = spawn(
        KernelOptions {
            data_dir: Some(base.join("data")),
            work_dir: Some(base.join("work")),
            model_override: Some(("anthropic".into(), "claude-sonnet-4-5".into())),
            api_key: Some("test-key".into()),
            ..Default::default()
        },
        ReviewPrefs::default(),
    );
    async fn next(rx: &mut UnboundedReceiver<BridgeEvent>) -> BridgeEvent {
        tokio::time::timeout(Duration::from_secs(15), rx.recv())
            .await
            .expect("event timed out")
            .expect("bridge closed")
    }
    match next(&mut bridge.events).await {
        BridgeEvent::Trust { request, respond } => {
            assert!(request.cwd.ends_with("work"), "{:?}", request.cwd);
            let _ = respond.send(Some(TrustChoice {
                trusted: false,
                scope: TrustScopeChoice::Session,
            }));
        }
        other => panic!("expected trust prompt first: {other:?}"),
    }
    let report = loop {
        match next(&mut bridge.events).await {
            BridgeEvent::TrustReport(report) => break report,
            BridgeEvent::Ready { .. } => panic!("Ready arrived before TrustReport"),
            _ => {}
        }
    };
    assert!(!report.trusted);
    assert_eq!(report.skipped, 1);
    loop {
        if matches!(next(&mut bridge.events).await, BridgeEvent::Ready { .. }) {
            break;
        }
    }
    drop(bridge);
    let _ = std::fs::remove_dir_all(&base);
}
