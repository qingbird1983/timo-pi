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
        last_request: None,
    };
    let new_messages = kernel.commit_outcome(outcome);
    assert_eq!(new_messages.len(), 1);
    // K0：第二次 run 能从完整上下文继续，而不是只剩新消息
    assert_eq!(kernel.history().unwrap().len(), 3);
    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test]
async fn cache_stats_read_the_session_entries() {
    let data = std::env::temp_dir().join(format!("timo-kernel-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let work = data.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let sessions = Kernel::sessions_dir(Some(&data)).unwrap();
    let dir = timo_config::session_dir_for(&sessions, &work);
    let mut writer = timo_session::SessionWriter::create(
        &dir,
        &work.display().to_string(),
        None,
        Arc::new(timo_config::FixedClock::new(1)),
        Arc::new(timo_session::IdGen::new()),
    )
    .unwrap();
    let id = writer.session_id().to_string();
    for (input, read, ts) in [(20_000u64, 500u64, 0i64), (18_500, 1_500, 60_000)] {
        let mut message =
            timo_ai::AssistantMessage::pending("openai-completions", "deepseek", "m", ts);
        message.stop_reason = timo_ai::StopReason::Stop;
        message.content = vec![timo_ai::Content::Text { text: "ok".into() }];
        message.usage.input = input;
        message.usage.cache_read = read;
        message.usage.total_tokens = input + read + 1;
        message.usage.cost.input = input as f64 * 1e-6;
        message.usage.cost.cache_read = read as f64 * 1e-7;
        writer
            .append_raw(
                timo_session::kinds::MESSAGE,
                serde_json::to_value(timo_ai::Message::Assistant(message)).unwrap(),
            )
            .unwrap();
    }
    drop(writer);

    let mut kernel = Kernel::new(KernelOptions {
        data_dir: Some(data.clone()),
        work_dir: Some(work),
        model_override: Some(("anthropic".into(), "claude-sonnet-4-5".into())),
        api_key: Some("test-key".into()),
        ..Default::default()
    })
    .await
    .expect("assemble");
    kernel.resume(&id).await.expect("resume");
    let stats = kernel.cache_stats().expect("stats");
    assert_eq!(stats.miss_count, 1);
    assert_eq!(stats.missed_tokens, 18_500);
    assert!((stats.missed_cost - 0.01665).abs() < 1e-9, "{stats:?}");
    assert_eq!(stats.misses[0].idle_ms, 60_000);
    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test]
async fn no_session_writes_nothing_to_disk() {
    let data = std::env::temp_dir().join(format!("timo-kernel-nosession-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let work = data.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let mut kernel = Kernel::new(KernelOptions {
        data_dir: Some(data.clone()),
        work_dir: Some(work.clone()),
        model_override: Some(("anthropic".into(), "claude-sonnet-4-5".into())),
        api_key: Some("test-key".into()),
        no_session: true,
        ..Default::default()
    })
    .await
    .expect("assemble");

    kernel.start_session().expect("start_session is a no-op");
    assert_eq!(kernel.session_id(), None);
    let sessions = Kernel::sessions_dir(Some(&data)).unwrap();
    let entries = std::fs::read_dir(&sessions)
        .map(|dir| dir.count())
        .unwrap_or_default();
    assert_eq!(entries, 0, "--no-session 不能落任何会话文件");
    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test]
async fn tool_allowlist_enables_listed_tools_and_warns_on_strays() {
    let data = std::env::temp_dir().join(format!("timo-kernel-tools-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let work = data.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let mut kernel = Kernel::new(KernelOptions {
        data_dir: Some(data.clone()),
        work_dir: Some(work),
        model_override: Some(("anthropic".into(), "claude-sonnet-4-5".into())),
        api_key: Some("test-key".into()),
        tool_allowlist: Some(vec!["read".into(), "grep".into(), "nope".into()]),
        ..Default::default()
    })
    .await
    .expect("assemble");

    let names: Vec<String> = kernel
        .tools()
        .await
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(
        names,
        ["grep", "read"],
        "列出的都要声明（grep 被默认激活集关掉了也要恢复），未列出的不声明"
    );
    let stray = kernel
        .resource_warnings()
        .iter()
        .find(|w| w.kind == crate::WarningKind::UnknownTool)
        .expect("stray warning");
    assert_eq!(stray.message, "unknown tool `nope`");
    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test]
async fn appended_prompt_text_lands_in_the_system_prompt() {
    let data = std::env::temp_dir().join(format!("timo-kernel-append-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let work = data.join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(data.join("APPEND_SYSTEM.md"), "File rule.").unwrap();
    let mut kernel = Kernel::new(KernelOptions {
        data_dir: Some(data.clone()),
        work_dir: Some(work),
        model_override: Some(("anthropic".into(), "claude-sonnet-4-5".into())),
        api_key: Some("test-key".into()),
        append_system_prompt: vec!["Flag rule.".into(), String::new()],
        ..Default::default()
    })
    .await
    .expect("assemble");
    kernel.tools().await;
    let prompt = kernel.system_prompt().to_string();
    let file_at = prompt.find("File rule.").expect("file section present");
    let flag_at = prompt.find("Flag rule.").expect("flag text appended");
    assert!(file_at < flag_at, "命令行文本接在文件之后");
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
