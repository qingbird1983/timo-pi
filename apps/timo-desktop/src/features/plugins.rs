//! 插件页面：左栏两个入口、已安装管理与市场（设计 01 §5、08 §3）。

mod install_wizard;
mod installed;
mod market;
pub(crate) mod nsh;

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::DesktopApp;
use crate::bridge::BridgeCommand;

pub(crate) use market::MarketTabView;

/// 中央区域当前打开哪一页。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PluginsView {
    /// 已安装管理。
    Installed,
    /// 市场发现。
    Market,
}

impl DesktopApp {
    /// 打开已安装管理并拉取清单。
    pub(crate) fn open_installed(&mut self, cx: &mut Context<Self>) {
        self.open_plugins_page(PluginsView::Installed, cx);
    }

    /// 打开插件市场并读取缓存。
    pub(crate) fn open_market(&mut self, cx: &mut Context<Self>) {
        self.open_plugins_page(PluginsView::Market, cx);
    }

    fn open_plugins_page(&mut self, view: PluginsView, cx: &mut Context<Self>) {
        self.plugins_view = Some(view);
        self.settings_open = false;
        self.palette_open = false;
        self.plugins_error = None;
        self.market_error = None;
        self.nsh_page = false;
        let _ = self.commands.send(BridgeCommand::ListPlugins);
        if view == PluginsView::Market {
            let _ = self.commands.send(BridgeCommand::LoadMarket);
        }
        cx.notify();
    }

    /// 关闭插件页并回到对话。
    pub(crate) fn close_plugins(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.plugins_view = None;
        self.plugins_selected = None;
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// 侧栏导航行（粗糙轮廓：文字 + 图标 + 选中底色）。
    pub(crate) fn plugin_nav_row(
        &self,
        id: &'static str,
        label: String,
        icon: IconName,
        active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .id(id)
            .test_support()
            .mx_2()
            .px_2()
            .py_2()
            .gap_2()
            .rounded_md()
            .items_center()
            .cursor_pointer()
            .when(active, |this| this.bg(cx.theme().sidebar_accent))
            .hover(|this| this.bg(cx.theme().sidebar_accent))
            .on_click(cx.listener(move |this, _, _, cx| {
                if id == "nav-installed" {
                    this.open_installed(cx);
                } else {
                    this.open_market(cx);
                }
            }))
            .child(
                Icon::new(icon)
                    .xsmall()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(if active {
                        cx.theme().sidebar_accent_foreground
                    } else {
                        cx.theme().sidebar_foreground
                    })
                    .child(label),
            )
            .into_any_element()
    }

    /// 插件页主体。
    pub(crate) fn plugins_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.plugins_view {
            Some(PluginsView::Installed) => self.installed_page(cx),
            Some(PluginsView::Market) => self.market_page(window, cx),
            None => v_flex().into_any_element(),
        }
    }

    /// 页头（标题 + 副标题 + 右侧操作）。
    pub(crate) fn plugins_head(
        &self,
        title: String,
        subtitle: String,
        actions: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_lg().font_semibold().child(title))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(subtitle),
                    ),
            )
            .child(h_flex().items_center().gap_2().children(actions))
            .into_any_element()
    }
}
