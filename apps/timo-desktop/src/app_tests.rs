#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::DesktopApp;
use crate::features::chat;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions,
    px, size,
};
use std::sync::Arc;
use timo_kernel::{
    AssistantMessage, AssistantStreamEvent, Content, KernelEvent, Message, UserMessage,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::bridge::{BridgeCommand, BridgeEvent};

#[allow(clippy::type_complexity)]
fn open(
    cx: &mut TestAppContext,
) -> (
    AnyWindowHandle,
    Entity<DesktopApp>,
    UnboundedSender<BridgeEvent>,
    UnboundedReceiver<BridgeCommand>,
) {
    let (commands, command_rx) = unbounded_channel::<BridgeCommand>();
    let (events_tx, events) = unbounded_channel();
    let (handle, app) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1280.), px(800.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| DesktopApp::with_bridge(commands, events, window, cx)),
        )
        .expect("open test window")
    });
    (handle, app, events_tx, command_rx)
}

fn assistant_text(chat: &chat::ChatState) -> String {
    chat.rows
        .iter()
        .filter_map(|row| match &row.kind {
            chat::RowKind::Assistant(row) => Some(row.text.clone()),
            chat::RowKind::User(_) => None,
        })
        .collect()
}

#[gpui_kit::test]
fn bridge_events_drive_chat_and_sessions(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    crate::i18n::init();
    let (handle, app, events_tx, _commands) = open(cx);

    events_tx
        .send(BridgeEvent::Ready {
            model: "deepseek-chat".into(),
            sessions: vec![],
        })
        .unwrap();
    cx.run_until_parked();
    assert!(app.read_with(cx, |app, _| app.ready));

    events_tx.send(BridgeEvent::Running).unwrap();
    events_tx
        .send(BridgeEvent::Kernel(KernelEvent::AgentStarted))
        .unwrap();
    events_tx
        .send(BridgeEvent::Kernel(KernelEvent::TurnStarted))
        .unwrap();
    let mut partial = AssistantMessage::pending("openai", "deepseek", "m", 0);
    partial.content = vec![Content::Text {
        text: "hello from kernel".into(),
    }];
    events_tx
        .send(BridgeEvent::Kernel(KernelEvent::StreamEvent {
            event: Arc::new(AssistantStreamEvent::Start { partial }),
        }))
        .unwrap();
    cx.run_until_parked();
    app.read_with(cx, |app, cx| {
        let chat = app.chat.read(cx);
        assert_eq!(chat.rows.len(), 1);
        assert_eq!(assistant_text(chat), "hello from kernel");
        assert!(chat.running);
    });

    events_tx.send(BridgeEvent::Finished).unwrap();
    events_tx
        .send(BridgeEvent::Kernel(KernelEvent::AgentFinished {
            messages: Arc::new(vec![]),
        }))
        .unwrap();
    cx.run_until_parked();
    assert!(!app.read_with(cx, |app, cx| app.chat.read(cx).running));

    events_tx
        .send(BridgeEvent::Opened {
            id: Some("s1".into()),
            history: vec![Message::User(UserMessage {
                content: vec![Content::Text {
                    text: "restored".into(),
                }],
                timestamp: 0,
            })],
        })
        .unwrap();
    cx.run_until_parked();
    app.read_with(cx, |app, cx| {
        assert_eq!(app.session_id.as_deref(), Some("s1"));
        assert_eq!(app.chat.read(cx).rows.len(), 1);
        assert!(matches!(
            app.chat.read(cx).rows[0].kind,
            chat::RowKind::User(ref text) if text == "restored"
        ));
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();
}

fn file_review(
    call_id: &str,
    path: &str,
    before: Option<&str>,
    after: &str,
) -> timo_kernel::ToolReviewRequest {
    timo_kernel::ToolReviewRequest {
        tool_call_id: call_id.into(),
        tool_name: "edit".into(),
        kind: timo_kernel::ReviewKind::FileChange {
            path: std::path::PathBuf::from(format!("/work/{path}")),
            display_path: path.into(),
            before: before.map(str::to_string),
            after: after.into(),
            binary: false,
        },
    }
}

fn seed_tool_call(events_tx: &UnboundedSender<BridgeEvent>, call_id: &str, tool: &str) {
    events_tx
        .send(BridgeEvent::Kernel(KernelEvent::TurnStarted))
        .unwrap();
    events_tx
        .send(BridgeEvent::Kernel(KernelEvent::ToolCallRequested {
            call: Arc::new(Content::ToolCall {
                id: call_id.into(),
                name: tool.into(),
                arguments: serde_json::json!({ "path": "a.txt" }),
                arguments_error: None,
            }),
        }))
        .unwrap();
}

#[gpui_kit::test]
fn review_accept_returns_allow_and_queues_next(cx: &mut TestAppContext) {
    use crate::features::chat::TraceStatus;
    use timo_kernel::ReviewDecision;
    cx.update(gpui_kit::init);
    crate::i18n::init();
    let (handle, app, events_tx, _commands) = open(cx);
    events_tx
        .send(BridgeEvent::Ready {
            model: "deepseek/deepseek-chat".into(),
            sessions: vec![],
        })
        .unwrap();
    seed_tool_call(&events_tx, "c1", "edit");
    let (respond, decision) = tokio::sync::oneshot::channel();
    events_tx
        .send(BridgeEvent::Review {
            request: file_review("c1", "a.txt", Some("a\n"), "b\n"),
            respond,
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    // sheet 的滑入动画按真实时间推进，测试里等它结束再点击。
    std::thread::sleep(std::time::Duration::from_millis(250));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("review-accept").is_some());
        window.click("review-accept", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(decision.blocking_recv().unwrap(), ReviewDecision::Allow);
    app.read_with(cx, |app, cx| {
        let chat = app.chat.read(cx);
        let row = chat
            .rows
            .iter()
            .find_map(|row| match &row.kind {
                chat::RowKind::Assistant(row) => Some(row),
                _ => None,
            })
            .unwrap();
        assert_eq!(row.trace[0].status, TraceStatus::Queued);
    });
}

#[gpui_kit::test]
fn review_reject_carries_reason_and_blocks(cx: &mut TestAppContext) {
    use crate::features::chat::TraceStatus;
    use timo_kernel::ReviewDecision;
    cx.update(gpui_kit::init);
    crate::i18n::init();
    let (handle, app, events_tx, _commands) = open(cx);
    seed_tool_call(&events_tx, "c2", "edit");
    let (respond, decision) = tokio::sync::oneshot::channel();
    events_tx
        .send(BridgeEvent::Review {
            request: file_review("c2", "a.txt", None, "new\n"),
            respond,
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    // sheet 的滑入动画按真实时间推进，测试里等它结束再点击。
    std::thread::sleep(std::time::Duration::from_millis(250));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("review-reject", cx);
        window.render_frame(cx);
    })
    .unwrap();
    match decision.blocking_recv().unwrap() {
        ReviewDecision::Reject { reason } => assert!(!reason.trim().is_empty()),
        ReviewDecision::Allow => panic!("expected reject"),
    }
    app.read_with(cx, |app, cx| {
        let chat = app.chat.read(cx);
        let row = chat
            .rows
            .iter()
            .find_map(|row| match &row.kind {
                chat::RowKind::Assistant(row) => Some(row),
                _ => None,
            })
            .unwrap();
        assert_eq!(row.trace[0].status, TraceStatus::Blocked);
    });
}

#[gpui_kit::test]
fn escape_dismisses_review_as_reject(cx: &mut TestAppContext) {
    use timo_kernel::ReviewDecision;
    cx.update(gpui_kit::init);
    crate::i18n::init();
    let (handle, _app, events_tx, _commands) = open(cx);
    seed_tool_call(&events_tx, "c3", "edit");
    let (respond, decision) = tokio::sync::oneshot::channel();
    events_tx
        .send(BridgeEvent::Review {
            request: file_review("c3", "a.txt", None, "new\n"),
            respond,
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    // sheet 的滑入动画按真实时间推进，测试里等它结束再点击。
    std::thread::sleep(std::time::Duration::from_millis(250));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    match decision.blocking_recv().unwrap() {
        ReviewDecision::Reject { .. } => {}
        ReviewDecision::Allow => panic!("escape must reject"),
    }
}

#[gpui_kit::test]
fn palette_toggles_with_shortcut(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    crate::i18n::init();
    let (handle, app, events_tx, _commands) = open(cx);
    events_tx
        .send(BridgeEvent::Ready {
            model: "deepseek/deepseek-chat".into(),
            sessions: vec![],
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        app.update(cx, |app, cx| app.focus_composer(window, cx));
        window.press("ctrl-k", cx);
        window.render_frame(cx);
        assert!(app.read(cx).palette_open);
        assert!(window.try_find("palette-panel").is_some());
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(window.try_find("palette-panel").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn palette_clears_query_on_reopen(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    crate::i18n::init();
    let (handle, app, events_tx, _commands) = open(cx);
    events_tx
        .send(BridgeEvent::Ready {
            model: "deepseek/deepseek-chat".into(),
            sessions: vec![],
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        app.update(cx, |app, cx| app.focus_composer(window, cx));
        window.press("ctrl-k", cx);
        window.render_frame(cx);
        let palette = app.read(cx).palette.clone();
        palette.update(cx, |state, cx| state.set_query("设置", window, cx));
        window.render_frame(cx);
        window.press("escape", cx);
        window.render_frame(cx);
        app.update(cx, |app, cx| app.focus_composer(window, cx));
        window.press("ctrl-k", cx);
        window.render_frame(cx);
        assert!(app.read(cx).palette_open);
        let query = app.read(cx).palette.read(cx).query(cx);
        assert_eq!(query.as_ref(), "");
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_shortcut_opens_and_escape_closes(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    crate::i18n::init();
    let (handle, app, events_tx, _commands) = open(cx);
    events_tx
        .send(BridgeEvent::Ready {
            model: "deepseek/deepseek-chat".into(),
            sessions: vec![],
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        app.update(cx, |app, cx| app.focus_composer(window, cx));
        window.press("ctrl-,", cx);
        window.render_frame(cx);
        assert!(app.read(cx).settings_open);
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(!app.read(cx).settings_open);
    })
    .unwrap();
}

#[gpui_kit::test]
fn review_shortcut_accepts(cx: &mut TestAppContext) {
    use timo_kernel::ReviewDecision;
    cx.update(gpui_kit::init);
    crate::i18n::init();
    let (handle, _app, events_tx, _commands) = open(cx);
    seed_tool_call(&events_tx, "c4", "edit");
    let (respond, decision) = tokio::sync::oneshot::channel();
    events_tx
        .send(BridgeEvent::Review {
            request: file_review("c4", "a.txt", None, "new\n"),
            respond,
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    // sheet 的滑入动画按真实时间推进，测试里等它结束再按键。
    std::thread::sleep(std::time::Duration::from_millis(250));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("ctrl-enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(decision.blocking_recv().unwrap(), ReviewDecision::Allow);
}

#[gpui_kit::test]
fn review_shortcut_rejects(cx: &mut TestAppContext) {
    use timo_kernel::ReviewDecision;
    cx.update(gpui_kit::init);
    crate::i18n::init();
    let (handle, _app, events_tx, _commands) = open(cx);
    seed_tool_call(&events_tx, "c5", "edit");
    let (respond, decision) = tokio::sync::oneshot::channel();
    events_tx
        .send(BridgeEvent::Review {
            request: file_review("c5", "a.txt", None, "new\n"),
            respond,
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    // sheet 的滑入动画按真实时间推进，测试里等它结束再按键。
    std::thread::sleep(std::time::Duration::from_millis(250));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("ctrl-backspace", cx);
        window.render_frame(cx);
    })
    .unwrap();
    match decision.blocking_recv().unwrap() {
        ReviewDecision::Reject { reason } => assert!(!reason.trim().is_empty()),
        ReviewDecision::Allow => panic!("ctrl-backspace must reject"),
    }
}

#[gpui_kit::test]
fn settings_page_lists_credentials(cx: &mut TestAppContext) {
    use crate::bridge::{CredentialInfo, ProviderInfo};
    cx.update(gpui_kit::init);
    crate::i18n::init();
    let (handle, app, events_tx, _commands) = open(cx);
    app.update(cx, |app, cx| app.open_settings(cx));
    events_tx
        .send(BridgeEvent::Providers(vec![ProviderInfo {
            id: "deepseek".into(),
            models: vec!["deepseek-chat".into()],
            credential: CredentialInfo::Env("DEEPSEEK_API_KEY".into()),
            editable: true,
        }]))
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("settings-tab-Credentials").is_some());
        window.click("settings-tab-Credentials", cx);
        window.render_frame(cx);
        assert!(window.try_find("provider-set-deepseek").is_some());
        assert!(window.try_find("provider-remove-deepseek").is_some());
    })
    .unwrap();
}
