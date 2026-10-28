//! 主窗口视图：标题栏、会话侧栏、聊天区、状态栏与内核桥接（设计 01 §4~§5）。

use std::sync::{Arc, Mutex};

use gpui_kit::component::TitleBar;
use gpui_kit::component::command::CommandState;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::message_scroller::MessageScrollerState;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::*;
use timo_kernel::{KernelOptions, SessionInfo, SlashCommand, UsageSummary};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::bridge::{BridgeCommand, BridgeEvent, ProviderInfo, TrustReport};
use crate::features::chat::ChatState;
use crate::features::palette::{
    AcceptReviewAction, CloseOverlayAction, NewSessionAction, OpenSettingsAction,
    RejectReviewAction, StopAction, TogglePalette,
};
use crate::features::review::ReviewState;
use crate::features::settings_view::SettingsTab;
use crate::features::trust::TrustState;
use crate::settings::{DesktopSettings, ThemePref};
use crate::theme;

#[path = "app_events.rs"]
mod events;

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
    pub(crate) slash_commands: Vec<SlashCommand>,
    pub(crate) usage: UsageSummary,
    pub(crate) ready: bool,
    pub(crate) fatal: Option<String>,
    pub(crate) notice: Option<String>,
    pub(crate) rows: usize,
    pub(crate) settings: Arc<Mutex<DesktopSettings>>,
    pub(crate) persist_settings: bool,
    pub(crate) theme_pref: ThemePref,
    pub(crate) font_scale: f32,
    pub(crate) settings_reset: bool,
    pub(crate) review: ReviewState,
    pub(crate) trust: TrustState,
    pub(crate) trust_report: Option<TrustReport>,
    pub(crate) palette: Entity<CommandState>,
    pub(crate) palette_open: bool,
    pub(crate) settings_open: bool,
    pub(crate) settings_tab: SettingsTab,
    pub(crate) settings_focus: FocusHandle,
    pub(crate) providers: Vec<ProviderInfo>,
    pub(crate) provider_error: Option<String>,
    pub(crate) provider_feedback: Option<String>,
    pub(crate) key_provider: Option<String>,
    pub(crate) key_input: Entity<InputState>,
    #[cfg(feature = "nsh-poc")]
    pub(crate) nsh_page: bool,
    #[cfg(feature = "nsh-poc")]
    pub(crate) nsh_preview: Option<crate::features::nsh_poc::NshPreview>,
    _subscriptions: Vec<Subscription>,
    _events: Task<()>,
}

impl DesktopApp {
    /// 创建根视图并启动真实内核桥接。
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (settings, reset) = DesktopSettings::load();
        crate::i18n::set(&settings.locale);
        theme::apply(settings.theme, settings.font_scale, Some(window), cx);
        let bridge = crate::bridge::spawn(KernelOptions::default(), settings.review);
        Self::build(
            bridge.commands,
            bridge.events,
            settings,
            reset,
            true,
            window,
            cx,
        )
    }

    /// 用给定桥接两端创建视图（测试可注入假内核；不读写真实桌设）。
    pub fn with_bridge(
        commands: UnboundedSender<BridgeCommand>,
        events: UnboundedReceiver<BridgeEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::build(
            commands,
            events,
            DesktopSettings::default(),
            false,
            false,
            window,
            cx,
        )
    }

    fn build(
        commands: UnboundedSender<BridgeCommand>,
        events: UnboundedReceiver<BridgeEvent>,
        settings: DesktopSettings,
        settings_reset: bool,
        persist_settings: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder(t!("chat.placeholder").to_string())
        });
        let mut subscriptions = vec![cx.subscribe_in(
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
        subscriptions.push(cx.observe_window_appearance(window, |this, window, cx| {
            if this.theme_pref == ThemePref::System {
                theme::apply(ThemePref::System, this.font_scale, Some(window), cx);
                this.dark = theme::is_dark(cx);
            }
        }));
        let shared = Arc::new(Mutex::new(settings.clone()));
        let quit_settings = shared.clone();
        let persist = persist_settings;
        subscriptions.push(cx.on_app_quit(move |_, _| {
            let settings = if persist {
                quit_settings.lock().ok().map(|settings| settings.clone())
            } else {
                None
            };
            async move {
                if let Some(settings) = settings {
                    let _ = settings.save();
                }
            }
        }));
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
        let key_input = cx.new(|cx| InputState::new(window, cx));
        key_input.update(cx, |input, cx| input.set_masked(true, window, cx));
        cx.bind_keys([
            KeyBinding::new("secondary-k", TogglePalette, None),
            KeyBinding::new("secondary-n", NewSessionAction, None),
            KeyBinding::new("secondary-,", OpenSettingsAction, None),
            KeyBinding::new("secondary-.", StopAction, None),
            KeyBinding::new("secondary-enter", AcceptReviewAction, Some("ReviewSheet")),
            KeyBinding::new(
                "secondary-backspace",
                RejectReviewAction,
                Some("ReviewSheet"),
            ),
            KeyBinding::new("escape", CloseOverlayAction, None),
        ]);
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
            slash_commands: vec![],
            usage: UsageSummary::default(),
            ready: false,
            fatal: None,
            notice: None,
            rows: 0,
            settings: shared,
            persist_settings,
            theme_pref: settings.theme,
            font_scale: settings.font_scale,
            settings_reset,
            review: ReviewState::new(window, cx),
            trust: TrustState::new(cx),
            trust_report: None,
            palette: cx.new(|cx| CommandState::new(window, cx)),
            palette_open: false,
            settings_open: false,
            settings_tab: SettingsTab::General,
            settings_focus: cx.focus_handle(),
            providers: vec![],
            provider_error: None,
            provider_feedback: None,
            key_provider: None,
            key_input,
            #[cfg(feature = "nsh-poc")]
            nsh_page: false,
            #[cfg(feature = "nsh-poc")]
            nsh_preview: None,
            _subscriptions: subscriptions,
            _events: task,
        }
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
        let version = format!("v{}", env!("CARGO_PKG_VERSION"));
        let right = if self.usage.totals.tokens() > 0 {
            format!(
                "{} tok · ${:.4} · {version}",
                self.usage.totals.tokens(),
                self.usage.totals.cost
            )
        } else {
            version
        };
        StatusBar::new().left(state).left(model).right(right)
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

    /// 中央区域（设置页或对话）；预览页由 `nsh-poc` feature 的包装接入。
    pub(crate) fn main_panel(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.settings_open {
            self.settings_page(cx)
        } else {
            let notice = self.notice_bar(cx);
            v_flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(self.chat_area(cx))
                .children(notice)
                .child(self.composer(cx))
                .into_any_element()
        }
    }

    /// 默认构建没有预览页（供 Esc 关闭分支共用）。
    #[cfg(not(feature = "nsh-poc"))]
    pub(crate) fn nsh_page_open(&self) -> bool {
        false
    }

    #[cfg(not(feature = "nsh-poc"))]
    pub(crate) fn close_nsh_page(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}
}

impl Render for DesktopApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.remember_window(window);
        if self.review.queue.is_empty() && self.review.open {
            self.review.open = false;
            if window.has_active_sheet(cx) {
                cx.defer_in(window, |_, window, cx| window.close_sheet(cx));
            }
        }
        if !self.review.queue.is_empty() && !self.review.open {
            cx.defer_in(window, |this, window, cx| {
                this.open_next_review(window, cx);
            });
        }
        if self.trust.pending.is_none() && self.trust.open {
            self.trust.open = false;
            if window.has_active_sheet(cx) {
                cx.defer_in(window, |_, window, cx| window.close_sheet(cx));
            }
        }
        if self.trust.pending.is_some() && !self.trust.open {
            cx.defer_in(window, |this, window, cx| {
                this.open_trust_sheet(window, cx);
            });
        }
        let center = {
            #[cfg(feature = "nsh-poc")]
            {
                self.center_panel(window, cx)
            }
            #[cfg(not(feature = "nsh-poc"))]
            {
                self.main_panel(window, cx)
            }
        };
        let palette = self.command_palette(cx);
        v_flex()
            .relative()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.title_bar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(self.sidebar(cx))
                    .child(center),
            )
            .child(self.status_bar(cx))
            .on_action(cx.listener(Self::on_toggle_palette))
            .on_action(cx.listener(Self::on_new_session))
            .on_action(cx.listener(Self::on_delete_current))
            .on_action(cx.listener(Self::on_stop))
            .on_action(cx.listener(Self::on_resend))
            .on_action(cx.listener(Self::on_focus_composer))
            .on_action(cx.listener(Self::on_open_settings))
            .on_action(cx.listener(Self::on_open_model))
            .on_action(cx.listener(Self::on_toggle_locale))
            .on_action(cx.listener(Self::on_toggle_theme))
            .on_action(cx.listener(Self::on_open_session))
            .on_action(cx.listener(Self::on_insert_prompt))
            .on_action(cx.listener(Self::on_accept_review))
            .on_action(cx.listener(Self::on_reject_review))
            .on_action(cx.listener(Self::on_close_overlay))
            .child(palette)
    }
}

#[path = "app_actions.rs"]
mod actions;

#[cfg(test)]
#[path = "app_tests.rs"]
pub(crate) mod tests;
