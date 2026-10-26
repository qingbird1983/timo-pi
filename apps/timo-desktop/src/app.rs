//! 主窗口视图：标题栏、会话侧栏、聊天区、状态栏与内核桥接（设计 01 §4~§5）。

use gpui_kit::component::TitleBar;
use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::component::message_scroller::MessageScrollerState;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::*;
use timo_kernel::{KernelEvent, KernelOptions, SessionInfo};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::bridge::{BridgeCommand, BridgeEvent};
use crate::features::chat::{self, ChatState};
use crate::theme;

/// 侧栏默认宽度（设计 01 §2：240~360，默认 248）。
pub const SIDEBAR_WIDTH: Pixels = px(248.);

/// 桌面壳根视图。
pub struct DesktopApp {
    pub(crate) dark: bool,
    pub(crate) commands: UnboundedSender<BridgeCommand>,
    pub(crate) chat: Entity<ChatState>,
    pub(crate) scroller: Entity<MessageScrollerState>,
    pub(crate) input: Entity<TextareaState>,
    pub(crate) sessions: Vec<SessionInfo>,
    pub(crate) session_id: Option<String>,
    pub(crate) session_title: String,
    pub(crate) model: Option<String>,
    pub(crate) ready: bool,
    pub(crate) fatal: Option<String>,
    pub(crate) notice: Option<String>,
    pub(crate) rows: usize,
    _subscriptions: Vec<Subscription>,
    _events: Task<()>,
}

impl DesktopApp {
    /// 创建根视图并启动真实内核桥接。
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let bridge = crate::bridge::spawn(KernelOptions::default());
        Self::with_bridge(bridge.commands, bridge.events, window, cx)
    }

    /// 用给定桥接两端创建视图（测试可注入假内核）。
    pub fn with_bridge(
        commands: UnboundedSender<BridgeCommand>,
        events: UnboundedReceiver<BridgeEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder(t!("chat.placeholder").to_string())
        });
        let subscriptions = vec![cx.subscribe_in(
            &input,
            window,
            |this, _state, event: &InputEvent, window, cx| {
                if matches!(
                    event,
                    InputEvent::PressEnter {
                        shift: false,
                        secondary: false
                    }
                ) {
                    this.send(window, cx);
                }
            },
        )];
        let task = cx.spawn(async move |this, cx| {
            let mut events = events;
            while let Some(event) = events.recv().await {
                if this
                    .update(cx, |this, cx| {
                        this.handle_bridge(event, cx);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            dark: theme::is_dark(cx),
            commands,
            chat: cx.new(|_| ChatState::default()),
            scroller: cx.new(|cx| MessageScrollerState::new(0, cx)),
            input,
            sessions: vec![],
            session_id: None,
            session_title: t!("chat.empty_session").to_string(),
            model: None,
            ready: false,
            fatal: None,
            notice: None,
            rows: 0,
            _subscriptions: subscriptions,
            _events: task,
        }
    }

    /// 当前是否为深色外观（测试与后续设置页读取）。
    pub fn is_dark(&self) -> bool {
        self.dark
    }

    /// 把焦点交给输入框。
    pub fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    pub(crate) fn toggle_theme(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.dark = !self.dark;
        theme::set_dark(self.dark, None, cx);
        cx.notify();
    }

    fn handle_bridge(&mut self, event: BridgeEvent, cx: &mut Context<Self>) {
        match event {
            BridgeEvent::Ready { model, sessions } => {
                self.ready = true;
                self.fatal = None;
                self.model = Some(model);
                self.sessions = sessions;
            }
            BridgeEvent::Sessions(sessions) => {
                self.sessions = sessions;
            }
            BridgeEvent::Deleted(id) => {
                if self.session_id.as_deref() == Some(id.as_str()) {
                    self.session_id = None;
                    self.session_title = t!("chat.empty_session").to_string();
                    self.chat.update(cx, |chat, _| chat.reset());
                    self.sync_scroller(cx);
                }
            }
            BridgeEvent::Opened { id, history } => {
                self.session_id = id;
                self.chat.update(cx, |chat, _| chat.load_history(&history));
                self.sync_scroller(cx);
                self.refresh_title(cx);
            }
            BridgeEvent::Running => {
                self.chat.update(cx, |chat, _| chat.running = true);
            }
            BridgeEvent::Finished => {
                self.chat.update(cx, |chat, _| chat.running = false);
                self.refresh_title(cx);
            }
            BridgeEvent::Kernel(event) => {
                let before = self.chat.read(cx).rows.len();
                self.chat.update(cx, |chat, _| chat.apply(&event));
                let after = self.chat.read(cx).rows.len();
                if after != before {
                    self.sync_scroller(cx);
                }
                if matches!(event, KernelEvent::AgentFinished { .. }) {
                    self.refresh_title(cx);
                }
            }
            BridgeEvent::Fatal(message) => {
                self.ready = false;
                self.fatal = Some(message);
            }
            BridgeEvent::OpenFailed(message) => {
                self.notice = Some(t!("chat.open_failed", message = message).to_string());
            }
            BridgeEvent::DeleteFailed(message) => {
                self.notice = Some(t!("chat.delete_failed", message = message).to_string());
            }
            BridgeEvent::RunFailed(message) => {
                self.notice = Some(t!("chat.run_failed", message = message).to_string());
            }
            BridgeEvent::Busy => {
                self.notice = Some(t!("chat.busy").to_string());
            }
            BridgeEvent::Lagged(skipped) => {
                self.notice = Some(t!("chat.lagged", n = skipped).to_string());
            }
        }
    }

    fn sync_scroller(&mut self, cx: &mut Context<Self>) {
        self.rows = self.chat.read(cx).rows.len();
        let rows = self.rows;
        self.scroller
            .update(cx, |scroller, cx| scroller.reset(rows, cx));
    }

    fn refresh_title(&mut self, cx: &mut Context<Self>) {
        let id = self.session_id.clone();
        self.session_title = id
            .as_ref()
            .and_then(|id| {
                self.sessions
                    .iter()
                    .find(|session| &session.id == id)
                    .map(|session| session.first_message.clone())
            })
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| t!("chat.empty_session").to_string());
        if let Some(last) = self
            .chat
            .read(cx)
            .rows
            .iter()
            .find_map(|row| match &row.kind {
                chat::RowKind::User(text) => Some(text.clone()),
                _ => None,
            })
            && self.session_id.is_none()
        {
            self.session_title = last;
        }
    }

    pub(crate) fn new_session(&mut self, cx: &mut Context<Self>) {
        if self.chat.read(cx).running {
            self.notice = Some(t!("chat.busy").to_string());
            return;
        }
        let _ = self.commands.send(BridgeCommand::NewSession);
        self.session_id = None;
        self.session_title = t!("chat.empty_session").to_string();
        self.chat.update(cx, |chat, _| chat.reset());
        self.sync_scroller(cx);
    }

    pub(crate) fn open_session(&mut self, id: String, cx: &mut Context<Self>) {
        if self.chat.read(cx).running {
            self.notice = Some(t!("chat.busy").to_string());
            return;
        }
        let _ = self.commands.send(BridgeCommand::OpenSession(id));
    }

    pub(crate) fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready || self.chat.read(cx).running {
            return;
        }
        let text = self.input.read(cx).value().to_string();
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.chat.update(cx, |chat, _| {
            chat.push_user(&text);
            chat.running = true;
        });
        let _ = self.commands.send(BridgeCommand::Run(text));
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.notice = None;
        self.sync_scroller(cx);
    }

    pub(crate) fn stop(&mut self, cx: &mut Context<Self>) {
        let _ = self.commands.send(BridgeCommand::Cancel);
        cx.notify();
    }

    fn title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        TitleBar::new().child(
            h_flex()
                .items_center()
                .gap_2()
                .child(
                    Icon::new(IconName::Bot)
                        .small()
                        .text_color(cx.theme().primary),
                )
                .child(
                    h_flex()
                        .items_baseline()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .child(t!("app.title").to_string()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("app.tagline").to_string()),
                        ),
                ),
        )
    }

    fn status_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let state = if !self.ready {
            if self.fatal.is_some() {
                t!("status.offline").to_string()
            } else {
                t!("chat.booting").to_string()
            }
        } else if self.chat.read(cx).running {
            t!("status.running").to_string()
        } else {
            t!("status.ready").to_string()
        };
        let model = self
            .model
            .clone()
            .unwrap_or_else(|| t!("status.disconnected").to_string());
        StatusBar::new()
            .left(state)
            .left(model)
            .right(format!("v{}", env!("CARGO_PKG_VERSION")))
    }

    fn notice_bar(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let message = self
            .fatal
            .as_ref()
            .map(|error| t!("chat.fatal", message = error).to_string())
            .or_else(|| self.notice.clone())?;
        Some(
            h_flex()
                .mx_4()
                .mb_2()
                .px_3()
                .py_2()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().danger)
                .text_xs()
                .text_color(cx.theme().danger)
                .child(message)
                .into_any_element(),
        )
    }
}

impl Render for DesktopApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let notice = self.notice_bar(cx);
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.title_bar(cx))
            .child(
                h_flex().flex_1().min_h_0().child(self.sidebar(cx)).child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.chat_area(cx))
                        .children(notice)
                        .child(self.composer(cx)),
                ),
            )
            .child(self.status_bar(cx))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::{DesktopApp, chat};
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AnyWindowHandle, AppContext as _, Bounds, Entity, TestAppContext, WindowBounds,
        WindowOptions, px, size,
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
}
