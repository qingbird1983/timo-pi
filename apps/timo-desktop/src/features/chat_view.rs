//! 聊天区：消息流、欢迎态、顶部运行控制与输入框（设计 01 §5~§6）。

use gpui_kit::component::input::Textarea;
use gpui_kit::component::message_scroller::MessageScroller;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
    button::Button, button::ButtonVariants as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::DesktopApp;
use crate::features::chat;

impl DesktopApp {
    pub(crate) fn chat_area(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(self.chat_header(cx))
            .child(if self.chat.read(cx).rows.is_empty() {
                self.welcome(cx).into_any_element()
            } else {
                let scroll = MessageScroller::new("chat-thread", self.scroller.clone(), {
                    let chat = self.chat.clone();
                    move |index, _window, cx| chat::render_row(index, &chat, cx)
                })
                .px_6()
                .py_4()
                .flex_1()
                .min_h_0();
                scroll.into_any_element()
            })
    }

    fn chat_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.chat.read(cx).running;
        let usage = self
            .chat
            .read(cx)
            .usage
            .as_ref()
            .map(|usage| t!("chat.usage", tokens = usage.total_tokens).to_string());
        h_flex()
            .h(px(44.))
            .flex_shrink_0()
            .px_4()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .text_sm()
                    .font_medium()
                    .truncate()
                    .child(self.session_title.clone()),
            )
            .when(running, |this| {
                this.child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(Icon::new(IconName::Loader).xsmall())
                        .child(t!("status.running").to_string()),
                )
            })
            .child(div().flex_1())
            .when_some(usage, |this, usage| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(usage),
                )
            })
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
            )
    }

    fn welcome(&self, cx: &Context<Self>) -> impl IntoElement {
        let reason = if !self.ready {
            Some(
                self.fatal
                    .as_ref()
                    .map(|_| t!("chat.config_hint").to_string())
                    .unwrap_or_else(|| t!("chat.booting").to_string()),
            )
        } else {
            None
        };
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                Icon::new(IconName::Bot)
                    .large()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .text_base()
                    .font_medium()
                    .child(t!("chat.welcome_title").to_string()),
            )
            .child(
                div()
                    .max_w(px(420.))
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("chat.welcome_hint").to_string()),
            )
            .when_some(reason, |this, reason| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(reason),
                )
            })
    }

    pub(crate) fn composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.chat.read(cx).running;
        let enabled = self.ready && !running;
        let draft = !self.input.read(cx).value().trim().is_empty();
        let model = self
            .model
            .clone()
            .unwrap_or_else(|| t!("status.disconnected").to_string());
        v_flex()
            .flex_shrink_0()
            .px_4()
            .pt_2()
            .pb_3()
            .gap_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .p_2()
                    .child(
                        Textarea::new(&self.input)
                            .w_full()
                            .min_h(px(64.))
                            .max_h(px(160.))
                            .disabled(!enabled)
                            .accessibility_id("composer-input"),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        h_flex()
                            .flex_1()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(Icon::new(IconName::Bot).xsmall())
                            .child(model)
                            .when(!enabled && !running, |this| {
                                this.child(t!("chat.config_hint").to_string())
                            }),
                    )
                    .child(if running {
                        Button::new("stop")
                            .danger()
                            .small()
                            .label(t!("chat.stop").to_string())
                            .on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
                    } else {
                        Button::new("send")
                            .primary()
                            .small()
                            .label(t!("chat.send").to_string())
                            .disabled(!enabled || !draft)
                            .on_click(cx.listener(|this, _, window, cx| this.send(window, cx)))
                    }),
            )
            .id("composer")
            .test_support()
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
}
