//! 侧栏 panel 容器：宿主画尺寸/边框/标题栏，插件只画内容（设计 03 §1）。

use gpui_kit::component::{ActiveTheme as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, TestSupportExt as _, div, px,
};

use super::runtime::VisibleSlot;
use crate::app::DesktopApp;

/// 单个 panel 容器的固定高度。
const PANEL_HEIGHT: f32 = 168.;
/// 面板区上限；超出部分由整段滚动，不挤占会话列表。
const SECTION_HEIGHT: f32 = 360.;

impl DesktopApp {
    /// 侧栏停靠的插件面板；没有健康实例时完全不占位。
    pub(crate) fn plugin_panel_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(runtime) = self.plugin_runtime.as_ref() else {
            return div().into_any_element();
        };
        let panels = runtime.visible_panels();
        if panels.is_empty() {
            return div().into_any_element();
        }
        let mut section = v_flex()
            .id("plugin-panels")
            .test_support()
            .max_h(px(SECTION_HEIGHT))
            .px_2()
            .pb_2()
            .gap_1()
            .overflow_y_scroll();
        for panel in panels {
            section = section.child(container(panel, cx));
        }
        section.into_any_element()
    }
}

fn container(panel: VisibleSlot, cx: &mut Context<DesktopApp>) -> AnyElement {
    let title = if panel.title.trim().is_empty() {
        panel.plugin.clone()
    } else {
        panel.title.clone()
    };
    v_flex()
        .id(format!("plugin-panel-{}", panel.plugin))
        .test_support()
        .h(px(PANEL_HEIGHT))
        .flex_shrink_0()
        .w_full()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .overflow_hidden()
        .child(
            h_flex()
                .px_2()
                .py_1()
                .gap_2()
                .items_center()
                .border_b_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().muted)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child(title),
                ),
        )
        .child(
            v_flex()
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_hidden()
                .child(panel.view.into_any_element()),
        )
        .into_any_element()
}
