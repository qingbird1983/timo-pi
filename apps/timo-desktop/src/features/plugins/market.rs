//! 插件市场页：页签、筛选、错落卡片与索引配置（设计 08 §3~§4）。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use timo_kernel::plugins::market::{
    MARKET_CATEGORIES, MarketPlugin, MarketQuery, MarketSort, MarketTab, filter,
};

use crate::app::DesktopApp;
use crate::bridge::BridgeCommand;

#[path = "market_card.rs"]
mod card;

use card::estimated_height;

/// 市场页签（`Installed` 直接切到已安装页）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarketTabView {
    /// 发现。
    Discover,
    /// 主题与外观。
    Theme,
    /// 收藏。
    Favorites,
    /// 已安装（跳到 01）。
    Installed,
    /// 已屏蔽。
    Blocked,
    /// 高级。
    Advanced,
}

impl MarketTabView {
    const ALL: [Self; 6] = [
        Self::Discover,
        Self::Theme,
        Self::Favorites,
        Self::Installed,
        Self::Blocked,
        Self::Advanced,
    ];

    fn label(self) -> String {
        match self {
            Self::Discover => t!("market.tab_discover").to_string(),
            Self::Theme => t!("market.tab_theme").to_string(),
            Self::Favorites => t!("market.tab_favorites").to_string(),
            Self::Installed => t!("market.tab_installed").to_string(),
            Self::Blocked => t!("market.tab_blocked").to_string(),
            Self::Advanced => t!("market.tab_advanced").to_string(),
        }
    }

    fn query_tab(self) -> Option<MarketTab> {
        match self {
            Self::Discover => Some(MarketTab::Discover),
            Self::Theme => Some(MarketTab::Theme),
            Self::Favorites => Some(MarketTab::Favorites),
            Self::Blocked => Some(MarketTab::Blocked),
            Self::Installed | Self::Advanced => None,
        }
    }
}

impl DesktopApp {
    /// 搜索框回车/筛选按钮：把输入值写入筛选条件。
    pub(crate) fn apply_market_search(&mut self, cx: &mut Context<Self>) {
        self.market_search = self.market_search_input.read(cx).value().trim().to_string();
        cx.notify();
    }

    pub(crate) fn market_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.market_tab == MarketTabView::Installed {
            return self.installed_page(cx);
        }
        let actions = vec![
            Button::new("market-refresh")
                .ghost()
                .small()
                .icon(IconName::Globe)
                .label(t!("market.refresh").to_string())
                .on_click(cx.listener(|this, _, _, cx| {
                    let _ = this.commands.send(BridgeCommand::RefreshMarket);
                    cx.notify();
                }))
                .into_any_element(),
            Button::new("market-close")
                .ghost()
                .small()
                .icon(IconName::Delete)
                .label(t!("plugins.close").to_string())
                .on_click(cx.listener(|this, _, window, cx| this.close_plugins(window, cx)))
                .into_any_element(),
        ];
        let mut body = vec![
            self.plugins_head(
                t!("market.title").to_string(),
                t!("market.subtitle").to_string(),
                actions,
                cx,
            ),
            self.market_tabs(cx),
        ];
        if let Some(message) = &self.market_error {
            body.push(banner(message.clone(), cx, true));
        }
        if self.market_offline.is_some() {
            body.push(banner(t!("market.offline").to_string(), cx, false));
        }
        if !self.market_warnings.is_empty() {
            body.push(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("market.warnings", n = self.market_warnings.len()).to_string())
                    .into_any_element(),
            );
        }
        if self.market_tab == MarketTabView::Advanced {
            body.push(self.market_advanced(cx));
        } else {
            body.push(self.market_toolbar(cx));
            body.push(self.market_cards(window, cx));
        }
        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .p_6()
            .gap_3()
            .children(body)
            .into_any_element()
    }

    fn market_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut tabs = Vec::new();
        for tab in MarketTabView::ALL {
            let active = self.market_tab == tab;
            tabs.push(
                Button::new(format!("market-tab-{:?}", tab))
                    .when(active, |button| button.primary())
                    .when(!active, |button| button.ghost())
                    .small()
                    .label(tab.label())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if tab == MarketTabView::Installed {
                            this.open_installed(cx);
                        } else {
                            this.market_tab = tab;
                            cx.notify();
                        }
                    }))
                    .into_any_element(),
            );
        }
        h_flex().gap_1().children(tabs).into_any_element()
    }

    fn market_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut chips = vec![
            Button::new("market-chip-all")
                .when(self.market_category.is_none(), |button| button.primary())
                .when(self.market_category.is_some(), |button| button.ghost())
                .xsmall()
                .label(t!("market.all").to_string())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.market_category = None;
                    cx.notify();
                }))
                .into_any_element(),
        ];
        for category in MARKET_CATEGORIES {
            let active = self.market_category.as_deref() == Some(category);
            chips.push(
                Button::new(format!("market-chip-{category}"))
                    .when(active, |button| button.primary())
                    .when(!active, |button| button.ghost())
                    .xsmall()
                    .label(category.to_string())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.market_category = if this.market_category.as_deref() == Some(category)
                        {
                            None
                        } else {
                            Some(category.to_string())
                        };
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        let mut sorts = Vec::new();
        for (sort, label) in [
            (
                MarketSort::Recommended,
                t!("market.sort_recommended").to_string(),
            ),
            (
                MarketSort::Downloads,
                t!("market.sort_downloads").to_string(),
            ),
            (MarketSort::Stars, t!("market.sort_stars").to_string()),
        ] {
            sorts.push(
                Button::new(format!("market-sort-{sort:?}"))
                    .when(self.market_sort == sort, |button| button.primary())
                    .when(self.market_sort != sort, |button| button.ghost())
                    .xsmall()
                    .label(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.market_sort = sort;
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(Input::new(&self.market_search_input).w(px(320.)))
                    .child(
                        Button::new("market-filter")
                            .ghost()
                            .small()
                            .label(t!("market.filter").to_string())
                            .on_click(cx.listener(|this, _, _, cx| this.apply_market_search(cx))),
                    )
                    .when(!self.market_search.is_empty(), |this| {
                        this.child(
                            Button::new("market-clear-search")
                                .ghost()
                                .xsmall()
                                .label(t!("market.clear").to_string())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.market_search.clear();
                                    this.market_search_input.update(cx, |input, cx| {
                                        input.set_value("", window, cx);
                                    });
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(h_flex().flex_wrap().gap_1().children(chips))
            .child(h_flex().items_center().gap_1().children(sorts))
            .into_any_element()
    }

    fn market_cards(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(index) = &self.market_index else {
            return v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("market.empty").to_string()),
                )
                .child(
                    Button::new("market-setup")
                        .primary()
                        .small()
                        .label(t!("market.configure").to_string())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.market_tab = MarketTabView::Advanced;
                            cx.notify();
                        })),
                )
                .into_any_element();
        };
        let Some(tab) = self.market_tab.query_tab() else {
            return v_flex().into_any_element();
        };
        let filtered = filter(
            index,
            &self.market_state,
            &MarketQuery {
                tab,
                category: self.market_category.clone(),
                search: self.market_search.clone(),
                sort: self.market_sort,
            },
        );
        if filtered.is_empty() {
            return v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("market.empty").to_string()),
                )
                .into_any_element();
        }
        let available = f32::from(window.bounds().size.width) - 248.0 - 48.0 - 24.0;
        let columns = (((available + 12.0) / 222.0).floor() as usize).clamp(1, 3);
        let mut buckets: Vec<Vec<MarketPlugin>> = vec![Vec::new(); columns];
        let mut heights = vec![0.0_f32; columns];
        for plugin in filtered {
            let target = heights
                .iter()
                .enumerate()
                .min_by(|left, right| left.1.total_cmp(right.1))
                .map_or(0, |(index, _)| index);
            heights[target] += estimated_height(&plugin);
            buckets[target].push(plugin);
        }
        let mut cols = Vec::new();
        for bucket in buckets {
            let mut cards = Vec::new();
            for plugin in &bucket {
                cards.push(self.market_card(plugin, cx));
            }
            cols.push(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_3()
                    .children(cards)
                    .into_any_element(),
            );
        }
        v_flex()
            .id("market-cards")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(h_flex().items_start().gap_3().children(cols))
            .into_any_element()
    }

    fn market_advanced(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self
            .market_url
            .clone()
            .unwrap_or_else(|| t!("market.url_none").to_string());
        let mut warnings = Vec::new();
        for warning in self.market_warnings.iter().take(5) {
            warnings.push(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(warning.clone())
                    .into_any_element(),
            );
        }
        v_flex()
            .id("market-advanced")
            .flex_1()
            .min_h_0()
            .gap_3()
            .overflow_y_scroll()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("market.url_current", url = current).to_string()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("market.url_hint").to_string()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(Input::new(&self.market_url_input).w(px(420.)))
                    .child(
                        Button::new("market-url-save")
                            .primary()
                            .small()
                            .label(t!("market.apply_url").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                let url = this.market_url_input.read(cx).value().trim().to_string();
                                if url.is_empty() {
                                    this.notice = Some(t!("market.url_empty").to_string());
                                } else {
                                    let _ = this
                                        .commands
                                        .send(BridgeCommand::SaveMarketIndex { url: Some(url) });
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("market-url-clear")
                            .ghost()
                            .small()
                            .label(t!("market.url_clear").to_string())
                            .on_click(cx.listener(|this, _, _, _| {
                                let _ = this
                                    .commands
                                    .send(BridgeCommand::SaveMarketIndex { url: None });
                            })),
                    ),
            )
            .children(warnings)
            .into_any_element()
    }
}

fn banner(text: String, cx: &mut Context<DesktopApp>, danger: bool) -> AnyElement {
    let color = if danger {
        cx.theme().danger
    } else {
        cx.theme().warning
    };
    div()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(color)
        .text_xs()
        .text_color(color)
        .child(text)
        .into_any_element()
}
