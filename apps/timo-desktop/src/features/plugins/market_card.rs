//! 市场卡片：单卡渲染与列高估算（设计 08 §3.1）。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use timo_kernel::plugins::market::MarketPlugin;

use crate::app::DesktopApp;
use crate::bridge::BridgeCommand;

impl DesktopApp {
    pub(super) fn market_card(&self, plugin: &MarketPlugin, cx: &mut Context<Self>) -> AnyElement {
        let installed = self.plugins.iter().any(|entry| entry.id == plugin.id);
        let favorite = self
            .market_state
            .favorites
            .iter()
            .any(|id| id == &plugin.id);
        let blocked = self.market_state.blocked.iter().any(|id| id == &plugin.id);
        let actionable = if installed {
            Button::new(format!("market-open-{}", plugin.id))
                .primary()
                .xsmall()
                .label(t!("market.open").to_string())
                .on_click(cx.listener(|this, _, _, cx| this.open_installed(cx)))
                .into_any_element()
        } else {
            Button::new(format!("market-install-{}", plugin.id))
                .primary()
                .xsmall()
                .label(t!("market.install").to_string())
                .tooltip(t!("market.install_p1").to_string())
                .disabled(true)
                .into_any_element()
        };
        let mut shots = Vec::new();
        if !plugin.screenshots.is_empty() {
            shots.push(shot(px(84.), cx));
        }
        if plugin.screenshots.len() > 1 {
            shots.push(shot(px(56.), cx));
        }
        let require = plugin
            .requires
            .timo
            .as_ref()
            .map(|version| format!("Timo {version}"))
            .into_iter()
            .chain(plugin.requires.api.map(|api| format!("API v{api}")))
            .collect::<Vec<_>>()
            .join(" · ");
        let mut tags: Vec<AnyElement> = plugin
            .categories
            .iter()
            .take(2)
            .map(|category| tag(category.clone(), cx))
            .collect();
        if !require.is_empty() {
            tags.push(tag(require, cx));
        }
        v_flex()
            .p_3()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .items_start()
                    .justify_between()
                    .gap_2()
                    .child(div().text_sm().font_semibold().child(plugin.name.clone()))
                    .child(actionable),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{} · v{}",
                        if plugin.author.name.is_empty() {
                            "community".to_string()
                        } else {
                            plugin.author.name.clone()
                        },
                        plugin.version
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        t!(
                            "market.stats",
                            downloads = plugin.stats.downloads_30d,
                            stars = plugin.stats.stars
                        )
                        .to_string(),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .max_h(px(76.))
                    .overflow_hidden()
                    .child(plugin.description.clone()),
            )
            .children(shots)
            .child(h_flex().flex_wrap().gap_1().children(tags))
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new(format!("market-fav-{}", plugin.id))
                                    .ghost()
                                    .xsmall()
                                    .label(if favorite {
                                        t!("market.favorited").to_string()
                                    } else {
                                        t!("market.favorite").to_string()
                                    })
                                    .on_click(cx.listener({
                                        let id = plugin.id.clone();
                                        move |this, _, _, _| {
                                            let _ = this.commands.send(
                                                BridgeCommand::ToggleMarketFavorite {
                                                    id: id.clone(),
                                                },
                                            );
                                        }
                                    })),
                            )
                            .child(
                                Button::new(format!("market-block-{}", plugin.id))
                                    .ghost()
                                    .xsmall()
                                    .label(if blocked {
                                        t!("market.unblock").to_string()
                                    } else {
                                        t!("market.block").to_string()
                                    })
                                    .on_click(cx.listener({
                                        let id = plugin.id.clone();
                                        move |this, _, _, _| {
                                            let _ = this.commands.send(
                                                BridgeCommand::ToggleMarketBlocked {
                                                    id: id.clone(),
                                                },
                                            );
                                        }
                                    })),
                            ),
                    )
                    .when(plugin.discussion.is_some(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("market.comments").to_string()),
                        )
                    }),
            )
            .into_any_element()
    }
}

pub(super) fn estimated_height(plugin: &MarketPlugin) -> f32 {
    let description = plugin.description.chars().count().min(160) as f32 * 0.5;
    let shots = plugin.screenshots.len().min(2) as f32 * 70.0;
    120.0 + description + shots
}

fn shot(height: Pixels, cx: &mut Context<DesktopApp>) -> AnyElement {
    div()
        .h(height)
        .w_full()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().muted)
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(t!("market.screenshot").to_string())
        .into_any_element()
}

fn tag(text: String, cx: &mut Context<DesktopApp>) -> AnyElement {
    div()
        .px_2()
        .py_0p5()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}
