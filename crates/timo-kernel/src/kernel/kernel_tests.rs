//! 内核装配与上下文写回测试（从 kernel.rs 拆出，保持单文件 ≤500 行）。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

#[tokio::test]
async fn commit_outcome_keeps_the_full_context() {
    let data = std::env::temp_dir().join(format!("timo-kernel-k0-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let work = data.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let mut kernel = Kernel::new(KernelOptions {
        data_dir: Some(data.clone()),
        work_dir: Some(work),
        model_override: Some(("anthropic".into(), "claude-sonnet-4-5".into())),
        api_key: Some("test-key".into()),
        ..Default::default()
    })
    .await
    .expect("assemble");

    let outcome = timo_agent::RunOutcome {
        new_messages: vec![timo_ai::user_text("second", 2)],
        context: vec![
            timo_ai::user_text("first", 1),
            timo_ai::user_text("first reply", 1),
            timo_ai::user_text("second", 2),
        ],
    };
    let new_messages = kernel.commit_outcome(outcome);
    assert_eq!(new_messages.len(), 1);
    // K0：第二次 run 能从完整上下文继续，而不是只剩新消息
    assert_eq!(kernel.history().unwrap().len(), 3);
    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test]
async fn manual_compact_needs_a_session_and_content() {
    let data = std::env::temp_dir().join(format!("timo-kernel-compact-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let work = data.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let mut kernel = Kernel::new(KernelOptions {
        data_dir: Some(data.clone()),
        work_dir: Some(work),
        model_override: Some(("anthropic".into(), "claude-sonnet-4-5".into())),
        api_key: Some("test-key".into()),
        ..Default::default()
    })
    .await
    .expect("assemble");

    // 草稿会话：没有可写入的 entry，直接拒绝
    let error = kernel.compact(None).await.unwrap_err();
    assert!(matches!(error, KernelError::Unavailable(_)));
    assert_eq!(error.to_string(), "no saved session to compact");

    // 已保存但为空：切点不成立 → Nothing（不发起摘要请求）
    kernel.open_new_session().unwrap();
    let error = kernel.compact(None).await.unwrap_err();
    assert!(matches!(
        error,
        KernelError::Compaction(timo_agent::compaction::CompactionError::Nothing)
    ));
    assert_eq!(
        error.to_string(),
        "compaction: nothing to compact (session too small)"
    );
    let _ = std::fs::remove_dir_all(&data);
}
