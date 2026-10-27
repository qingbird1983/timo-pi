//! 会话恢复语义（设计 10 §4.2）：虚拟选择保持 / 注销后退回物理 / 路由状态可读。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use timo_ai::{AssistantMessage, Message, StopReason};
use timo_config::FixedClock;
use timo_kernel::{Kernel, KernelOptions};
use timo_session::{IdGen, SessionWriter, kinds, read_entries};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("timo-route-resume-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// models.json：`aaa/base` 是默认选择，`mock/chat` 是历史响应，`router/auto` 是虚拟条目。
fn data_dir(name: &str) -> PathBuf {
    let dir = temp_dir(name);
    let model = |id: &str| {
        format!(
            r#"{{"id":"{id}","name":"{id}","context_window":1000,"max_tokens":100,"input":["text"],"cost":{{"input":1,"output":1,"cache_read":0,"cache_write":0}}}}"#
        )
    };
    std::fs::write(
        dir.join("models.json"),
        format!(
            r#"{{"providers":{{"aaa":{{"base_url":"https://other.example/v1","api":"openai-completions","api_key":"x","models":[{}]}},"mock":{{"base_url":"https://mock.example/v1","api":"openai-completions","api_key":"x","models":[{},{}]}},"router":{{"api":"pi-virtual","models":[{{"id":"auto","name":"Auto"}}]}}}}}}"#,
            model("base"),
            model("chat"),
            model("small")
        ),
    )
    .unwrap();
    dir
}

fn options(data: &Path, work: &Path) -> KernelOptions {
    KernelOptions {
        data_dir: Some(data.to_path_buf()),
        work_dir: Some(work.to_path_buf()),
        ..Default::default()
    }
}

/// 造一个已落盘的会话：虚拟选择 + 一条物理响应 + 一条路由状态。
fn seed_session(data: &Path, work: &Path) -> String {
    let dir = timo_config::session_dir_for(&data.join("sessions"), work);
    let mut writer = SessionWriter::create(
        &dir,
        &work.display().to_string(),
        Some("router/auto"),
        Arc::new(FixedClock::new(1_000)),
        Arc::new(IdGen::new()),
    )
    .unwrap();
    writer
        .append_raw(
            kinds::MODEL_CHANGE,
            json!({ "provider": "router", "model_id": "auto" }),
        )
        .unwrap();
    let mut message = AssistantMessage::pending("openai-completions", "mock", "chat", 2);
    message.stop_reason = StopReason::Stop;
    message.content = vec![timo_ai::Content::Text {
        text: "answer".into(),
    }];
    writer
        .append_raw(
            kinds::MESSAGE,
            serde_json::to_value(Message::Assistant(message)).unwrap(),
        )
        .unwrap();
    writer
        .append_raw(
            kinds::VIRTUAL_MODEL_STATE,
            json!({
                "provider": "router",
                "model_id": "auto",
                "state": { "phase": "build" },
            }),
        )
        .unwrap();
    writer.session_id().to_string()
}

#[tokio::test]
async fn resuming_without_the_extension_falls_back_to_the_last_physical_model() {
    let data = data_dir("fallback");
    let work = temp_dir("fallback-work");
    let id = seed_session(&data, &work);
    let mut kernel = Kernel::new(options(&data, &work)).await.expect("kernel");
    assert_eq!(kernel.model_label(), "aaa/base", "默认选择先于恢复");
    kernel.resume(&id).await.expect("resume");
    assert_eq!(kernel.model_label(), "mock/chat");
    assert_eq!(kernel.route_state(), None, "物理选择不携带路由状态");
}

#[tokio::test]
async fn an_explicit_virtual_selection_wins_and_restores_its_state() {
    let data = data_dir("explicit");
    let work = temp_dir("explicit-work");
    let id = seed_session(&data, &work);
    let mut kernel = Kernel::new(KernelOptions {
        model_override: Some(("router".into(), "auto".into())),
        ..options(&data, &work)
    })
    .await
    .expect("kernel");
    kernel.resume(&id).await.expect("resume");
    assert_eq!(kernel.model_label(), "router/auto");
    assert_eq!(kernel.route_state(), Some(json!({ "phase": "build" })));
}

#[tokio::test]
async fn a_new_session_records_a_virtual_selection() {
    let data = data_dir("record");
    let work = temp_dir("record-work");
    let mut kernel = Kernel::new(KernelOptions {
        model_override: Some(("router".into(), "auto".into())),
        ..options(&data, &work)
    })
    .await
    .expect("kernel");
    kernel.start_session().expect("session");
    let id = kernel.session_id().expect("id").to_string();
    let path =
        timo_config::session_dir_for(&data.join("sessions"), &work).join(format!("{id}.jsonl"));
    let entries = read_entries(&path).expect("entries");
    let change = entries
        .iter()
        .find(|entry| entry.kind == kinds::MODEL_CHANGE)
        .expect("model.change entry");
    assert_eq!(change.payload["provider"], "router");
    assert_eq!(change.payload["model_id"], "auto");
}
