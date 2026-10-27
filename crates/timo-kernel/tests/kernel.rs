//! 门面集成测试：装配链与解析优先级（不触网；run 的 LLM 路径由 timo-agent
//! 集成测试以 mock 覆盖）。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use timo_config::FixedClock;
use timo_kernel::{Kernel, KernelOptions};
use timo_session::{IdGen, SessionWriter};

fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("timo-kernel-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[tokio::test]
async fn assembles_with_explicit_model_and_default_tools() {
    let data = temp_dir("data");
    let work = temp_dir("work");
    let kernel = Kernel::new(KernelOptions {
        data_dir: Some(data),
        work_dir: Some(work),
        model_override: Some(("anthropic".into(), "claude-sonnet-4-5".into())),
        api_key: Some("test-key".into()),
        ..Default::default()
    })
    .await
    .expect("assemble");
    assert_eq!(kernel.model_label(), "anthropic/claude-sonnet-4-5");
    // pi DEFAULT_TOOL_NAMES：read/bash/edit/write（powershell/grep/find 注册但 Hidden）
    // 注册表对外声明序为确定性的名字序；契约只约束集合（设计 03 §1）。
    let mut declared = kernel.declared_tools();
    declared.sort();
    assert_eq!(declared, vec!["bash", "edit", "read", "write"]);
    assert_eq!(kernel.thinking_label(), "medium"); // 设置缺省 medium
}

#[tokio::test]
async fn unknown_model_spec_errors() {
    let data = temp_dir("bad");
    let result = Kernel::new(KernelOptions {
        data_dir: Some(data),
        model_override: Some(("anthropic".into(), "no-such-model".into())),
        api_key: Some("k".into()),
        ..Default::default()
    })
    .await;
    let Err(err) = result else {
        panic!("must fail");
    };
    assert!(err.to_string().contains("model not found"));
}

#[tokio::test]
async fn a_bare_model_id_resolves_when_unique() {
    let data = temp_dir("bareid");
    let kernel = Kernel::new(KernelOptions {
        data_dir: Some(data),
        work_dir: Some(temp_dir("bareid-work")),
        model_override: Some(("".into(), "claude-sonnet-4-5".into())),
        api_key: Some("test-key".into()),
        ..Default::default()
    })
    .await
    .expect("bare id must resolve on its own");
    assert_eq!(kernel.model_label(), "anthropic/claude-sonnet-4-5");
}

#[tokio::test]
async fn an_ambiguous_bare_model_id_lists_candidates() {
    let data = temp_dir("duel");
    let model = r#"{"id":"dual","context_window":1000,"max_tokens":100,"input":["text"],"cost":{"input":1,"output":1,"cache_read":0,"cache_write":0}}"#;
    std::fs::write(
        data.join("models.json"),
        format!(
            r#"{{"providers":{{"acme":{{"base_url":"https://a.example/v1","api":"openai-completions","api_key":"x","models":[{model}]}},"other":{{"base_url":"https://b.example/v1","api":"openai-completions","api_key":"x","models":[{model}]}}}}}}"#
        ),
    )
    .unwrap();
    let result = Kernel::new(KernelOptions {
        data_dir: Some(data),
        work_dir: Some(temp_dir("duel-work")),
        model_override: Some(("".into(), "dual".into())),
        api_key: Some("k".into()),
        ..Default::default()
    })
    .await;
    let Err(err) = result else {
        panic!("an ambiguous bare id must fail");
    };
    let text = err.to_string();
    assert!(
        text.contains("ambiguous") && text.contains("acme/dual") && text.contains("other/dual"),
        "{text}"
    );
}

#[tokio::test]
async fn no_credential_errors_cleanly() {
    let data = temp_dir("nocred");
    let err = Kernel::new(KernelOptions {
        data_dir: Some(data),
        // 不给 key 且测试环境无 ANTHROPIC/OPENAI key 时：第三优先级全部不可用
        ..Default::default()
    })
    .await;
    // 若 CI 环境恰好有 key，本测试允许通过（不视为失败）
    if let Err(e) = err {
        assert!(e.to_string().contains("credential") || e.to_string().contains("model not found"));
    }
}

#[tokio::test]
async fn resume_missing_session_errors() {
    let data = temp_dir("resume");
    let mut kernel = Kernel::new(KernelOptions {
        data_dir: Some(data),
        model_override: Some(("anthropic".into(), "claude-haiku-4-5".into())),
        api_key: Some("k".into()),
        ..Default::default()
    })
    .await
    .unwrap();
    let err = kernel.resume("01NOPE").await.expect_err("missing session");
    assert!(matches!(
        err,
        timo_kernel::KernelError::Session(timo_session_shim::SessionError::InvalidFile { .. })
            | timo_kernel::KernelError::Session(timo_session_shim::SessionError::Io { .. })
            | timo_kernel::KernelError::Session(timo_session_shim::SessionError::NotFound(_))
    ));
}

#[tokio::test]
async fn mcp_is_lazy_and_failure_does_not_block() {
    use timo_mcp::ServerState;

    let data = temp_dir("mcp-lazy");
    std::fs::write(
        data.join("mcp.json"),
        r#"{"servers":{"bad":{"command":"definitely-not-a-real-command-xyz"}}}"#,
    )
    .unwrap();
    let mut kernel = Kernel::new(KernelOptions {
        data_dir: Some(data),
        model_override: Some(("anthropic".into(), "claude-haiku-4-5".into())),
        api_key: Some("k".into()),
        ..Default::default()
    })
    .await
    .expect("assemble without connecting");
    // 装配期只读配置：状态仍 Idle，未发起连接。
    assert_eq!(kernel.mcp_state("bad").await, ServerState::Idle);
    let mut events = kernel.events();
    // 首次枚举触发连接失败：不发散错误，只通告事件后照常可用。
    let tools = kernel.tools().await;
    assert_eq!(tools.len(), 4, "builtin active set survives MCP failure");
    assert!(matches!(
        kernel.mcp_state("bad").await,
        ServerState::Failed(_)
    ));
    let event = events.try_recv().expect("mcp failure reported");
    assert!(matches!(
        event,
        timo_agent::KernelEvent::Error { ref context } if context.contains("bad")
    ));
}

#[tokio::test]
async fn session_draft_history_and_delete_api() {
    let data = temp_dir("sessions-api");
    let work = temp_dir("sessions-api-work");
    let dir = Kernel::sessions_dir(Some(&data)).unwrap();
    std::fs::create_dir_all(&dir).unwrap();

    let mut kernel = Kernel::new(KernelOptions {
        data_dir: Some(data.clone()),
        work_dir: Some(work),
        model_override: Some(("anthropic".into(), "claude-sonnet-4-5".into())),
        api_key: Some("k".into()),
        ..Default::default()
    })
    .await
    .expect("assemble");

    assert!(kernel.session_id().is_none());
    assert!(kernel.history().is_none());
    kernel.new_session();
    assert!(kernel.session_id().is_none());

    // 当前会话删除后回到草稿态
    let writer = SessionWriter::create(
        &dir,
        "D:/x",
        None,
        Arc::new(FixedClock::new(1)),
        Arc::new(IdGen::new()),
    )
    .unwrap();
    let id = writer.session_id().to_string();
    drop(writer);
    kernel.resume(&id).await.expect("resume");
    assert_eq!(kernel.session_id(), Some(id.as_str()));
    assert!(kernel.history().is_some());
    kernel.delete_session(&id).expect("delete current");
    assert!(kernel.session_id().is_none());
    assert!(kernel.history().is_none());
    assert!(!dir.join(format!("{id}.jsonl")).exists());

    // 不存在的会话 / 路径穿越都被拒绝
    assert!(kernel.delete_session(&id).is_err());
    let outside = data.join("outside.jsonl");
    std::fs::write(&outside, "x").unwrap();
    assert!(kernel.delete_session("../outside").is_err());
    assert!(outside.exists());
}

/// 测试专用：直接引用 timo-session 的错误类型做匹配（避免门面重导出膨胀）。
mod timo_session_shim {
    pub use timo_session::SessionError;
}
