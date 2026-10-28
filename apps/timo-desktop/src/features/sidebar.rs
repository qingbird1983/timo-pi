//! 会话侧栏：列表、新建、切换与删除确认（设计 01 §5.2）。

use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _, WindowExt as _,
    button::Button, button::ButtonVariants as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use timo_kernel::SessionInfo;

use crate::app::{DesktopApp, SIDEBAR_WIDTH};
use crate::bridge::BridgeCommand;
use crate::features::plugins::PluginsView;

impl DesktopApp {
    pub(crate) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = v_flex()
            .w(SIDEBAR_WIDTH)
            .h_full()
            .flex_shrink_0()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .child(
                h_flex()
                    .px_3()
                    .pt_4()
                    .pb_2()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().sidebar_foreground)
                            .child(t!("sidebar.title").to_string()),
                    )
                    .child(
                        Button::new("new-session")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip(t!("sidebar.new").to_string())
                            .disabled(self.chat.read(cx).running)
                            .on_click(cx.listener(|this, _, _, cx| this.new_session(cx))),
                    ),
            )
            .child(
                v_flex()
                    .pb_2()
                    .gap_1()
                    .child(self.plugin_nav_row(
                        "nav-installed",
                        t!("plugins.nav").to_string(),
                        IconName::Settings,
                        self.plugins_view == Some(PluginsView::Installed),
                        cx,
                    ))
                    .child(self.plugin_nav_row(
                        "nav-market",
                        t!("market.nav").to_string(),
                        IconName::Search,
                        self.plugins_view == Some(PluginsView::Market),
                        cx,
                    )),
            )
            .child(
                v_flex()
                    .id("session-list")
                    .flex_1()
                    .min_h_0()
                    .px_2()
                    .gap_1()
                    .overflow_y_scroll()
                    .when(self.sessions.is_empty(), |this| {
                        this.child(
                            div()
                                .px_2()
                                .py_2()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("sidebar.empty").to_string()),
                        )
                    })
                    .children({
                        let mut rows = Vec::with_capacity(self.sessions.len());
                        for session in &self.sessions {
                            rows.push(self.session_row(session, cx));
                        }
                        rows
                    }),
            );
        sidebar.id("sidebar").test_support()
    }

    fn session_row(&self, session: &SessionInfo, cx: &mut Context<Self>) -> AnyElement {
        let id = session.id.clone();
        let active = self.session_id.as_deref() == Some(session.id.as_str());
        let title = if session.first_message.trim().is_empty() {
            t!("chat.empty_session").to_string()
        } else {
            session.first_message.clone()
        };
        let delete_id = session.id.clone();
        let delete_title = title.clone();
        let meta = session_meta(session.modified_ms);
        h_flex()
            .id(format!("session-{}", session.id))
            .px_2()
            .py_2()
            .gap_1()
            .rounded_md()
            .items_center()
            .when(active, |this| this.bg(cx.theme().sidebar_accent))
            .hover(|this| this.bg(cx.theme().sidebar_accent))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.open_session(id.clone(), cx)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .truncate()
                            .text_color(if active {
                                cx.theme().sidebar_accent_foreground
                            } else {
                                cx.theme().sidebar_foreground
                            })
                            .child(title),
                    )
                    .when_some(meta, |this, meta| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(meta),
                        )
                    }),
            )
            .child(
                Button::new(format!("delete-session-{}", session.id))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Delete)
                    .tooltip(t!("chat.delete").to_string())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.confirm_delete(delete_id.clone(), delete_title.clone(), window, cx);
                    })),
            )
            .into_any_element()
    }

    pub(crate) fn confirm_delete(
        &mut self,
        id: String,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let commands = self.commands.clone();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let id = id.clone();
            let commands = commands.clone();
            alert
                .title(t!("chat.delete_title", title = title.clone()).to_string())
                .description(t!("chat.delete_body").to_string())
                .confirm()
                .ok_text(t!("chat.delete_confirm").to_string())
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .on_ok(move |_, _, _| {
                    let _ = commands.send(BridgeCommand::DeleteSession(id.clone()));
                    true
                })
        });
    }
}

/// 会话行的次要信息：相对修改时间（设计 01 §5.2 标题/时间）。
fn session_meta(modified_ms: i64) -> Option<String> {
    if modified_ms <= 0 {
        return None;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    let seconds = (now - modified_ms).max(0) / 1000;
    Some(if seconds < 60 {
        t!("sidebar.just_now").to_string()
    } else if seconds < 3600 {
        t!("sidebar.minutes_ago", n = seconds / 60).to_string()
    } else if seconds < 86_400 {
        t!("sidebar.hours_ago", n = seconds / 3600).to_string()
    } else {
        t!("sidebar.days_ago", n = seconds / 86_400).to_string()
    })
}
