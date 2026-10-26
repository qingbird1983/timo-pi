//! 主视图的动作方法：发送/恢复会话、设置持久化与外观（从 `app.rs` 拆出）。

use gpui_kit::*;

use crate::app::DesktopApp;
use crate::bridge::BridgeCommand;
use crate::features::chat;
use crate::settings::{DesktopSettings, ThemePref};
use crate::theme;

impl DesktopApp {
    /// 当前是否为深色外观（测试与设置页读取）。
    pub fn is_dark(&self) -> bool {
        self.dark
    }

    /// 把焦点交给输入框。
    pub fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    pub(crate) fn toggle_theme(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let pref = if theme::is_dark(cx) {
            ThemePref::Light
        } else {
            ThemePref::Dark
        };
        self.apply_theme(pref, None, cx);
    }

    /// 应用并持久化外观偏好。
    pub(crate) fn apply_theme(
        &mut self,
        pref: ThemePref,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        self.theme_pref = pref;
        theme::apply(pref, self.font_scale, window, cx);
        self.dark = theme::is_dark(cx);
        self.update_settings(|settings| settings.theme = pref);
        cx.notify();
    }

    /// 修改桌面设置并（真实运行时）写盘。
    pub(crate) fn update_settings(&self, edit: impl FnOnce(&mut DesktopSettings)) {
        let Ok(mut settings) = self.settings.lock() else {
            return;
        };
        edit(&mut settings);
        if self.persist_settings {
            let _ = settings.save();
        }
    }

    pub(crate) fn remember_window(&self, window: &Window) {
        let size = window.bounds().size;
        if let Ok(mut settings) = self.settings.lock() {
            settings.window.width = f32::from(size.width);
            settings.window.height = f32::from(size.height);
        }
    }

    pub(crate) fn cancel_pending_reviews(&mut self, cx: &mut Context<Self>) {
        if self.review.queue.is_empty() {
            return;
        }
        let ids: Vec<String> = self
            .review
            .queue
            .iter()
            .map(|pending| pending.request.tool_call_id.clone())
            .collect();
        self.review.queue.clear();
        for id in ids {
            self.chat
                .update(cx, |chat, _| chat.resolve_review(&id, false));
        }
        cx.notify();
    }

    pub(crate) fn sync_scroller(&mut self, cx: &mut Context<Self>) {
        self.rows = self.chat.read(cx).rows.len();
        let rows = self.rows;
        self.scroller
            .update(cx, |scroller, cx| scroller.reset(rows, cx));
    }

    pub(crate) fn refresh_title(&mut self, cx: &mut Context<Self>) {
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
        #[cfg(feature = "nsh-poc")]
        {
            self.nsh_page = false;
        }
    }

    pub(crate) fn open_session(&mut self, id: String, cx: &mut Context<Self>) {
        if self.chat.read(cx).running {
            self.notice = Some(t!("chat.busy").to_string());
            return;
        }
        #[cfg(feature = "nsh-poc")]
        {
            self.nsh_page = false;
        }
        let _ = self.commands.send(BridgeCommand::OpenSession(id));
    }

    pub(crate) fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        self.send_text(text, cx);
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    /// 发送一段文本（Enter 与“重发上一条”共用）。
    pub(crate) fn send_text(&mut self, text: String, cx: &mut Context<Self>) {
        if !self.ready || self.chat.read(cx).running {
            return;
        }
        self.chat.update(cx, |chat, _| {
            chat.push_user(&text);
            chat.running = true;
        });
        let _ = self.commands.send(BridgeCommand::Run(text));
        self.notice = None;
        self.sync_scroller(cx);
    }

    pub(crate) fn resend_last(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.chat.read(cx).last_user_text() {
            self.send_text(text, cx);
        }
    }

    pub(crate) fn stop(&mut self, cx: &mut Context<Self>) {
        let _ = self.commands.send(BridgeCommand::Cancel);
        cx.notify();
    }
}
