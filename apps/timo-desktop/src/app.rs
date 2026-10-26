//! 主窗口视图：标题栏、会话侧栏、聊天区、状态栏的最小骨架。
//!
//! 本批只验证窗口与区域关系；会话/聊天/审阅在后续批次接入（设计 00 §8）。

use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, TitleBar, button::Button,
    button::ButtonVariants as _, h_flex, v_flex,
};
use gpui_kit::*;

use crate::theme;

/// 侧栏默认宽度（设计 01 §2：240~360，默认 248）。
pub const SIDEBAR_WIDTH: Pixels = px(248.);

/// 桌面壳根视图。
pub struct DesktopApp {
    dark: bool,
}

impl DesktopApp {
    /// 创建根视图；初始外观跟随全局主题。
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            dark: theme::is_dark(cx),
        }
    }

    fn toggle_theme(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.dark = !self.dark;
        theme::set_dark(self.dark, None, cx);
        cx.notify();
    }

    /// 当前是否为深色外观（测试与后续设置页读取）。
    pub fn is_dark(&self) -> bool {
        self.dark
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

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w(SIDEBAR_WIDTH)
            .h_full()
            .flex_shrink_0()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .child(
                h_flex().px_3().pt_4().pb_2().items_center().child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(cx.theme().sidebar_foreground)
                        .child(t!("sidebar.title").to_string()),
                ),
            )
            .child(
                div()
                    .px_3()
                    .py_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("sidebar.empty").to_string()),
            )
            .id("sidebar")
            .test_support()
    }

    fn chat(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                h_flex()
                    .h(px(44.))
                    .flex_shrink_0()
                    .px_4()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(t!("chat.draft").to_string()),
                    )
                    .child(
                        div()
                            .id("theme-toggle")
                            .child(
                                Button::new("theme-toggle-button")
                                    .ghost()
                                    .xsmall()
                                    .icon(self.theme_icon())
                                    .tooltip(self.theme_tooltip())
                                    .on_click(cx.listener(Self::toggle_theme)),
                            )
                            .test_support(),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(
                        Icon::new(IconName::Bot)
                            .large()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(div().text_sm().child(t!("chat.shell_ready").to_string()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("chat.shell_pending").to_string()),
                    ),
            )
    }

    fn theme_icon(&self) -> IconName {
        if self.dark {
            IconName::Sun
        } else {
            IconName::Moon
        }
    }

    fn theme_tooltip(&self) -> String {
        if self.dark {
            t!("theme.to_light").to_string()
        } else {
            t!("theme.to_dark").to_string()
        }
    }

    fn status_bar(&self) -> impl IntoElement {
        StatusBar::new()
            .left(t!("status.ready").to_string())
            .left(t!("status.kernel_offline").to_string())
            .right(format!("v{}", env!("CARGO_PKG_VERSION")))
    }
}

impl Render for DesktopApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.title_bar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.sidebar(cx))
                    .child(self.chat(cx)),
            )
            .child(self.status_bar())
    }
}
