//! 工具卡、会话附加区与输入区触发器的宿主容器（设计 03 §1）。
//!
//! 尺寸、边框与裁剪归宿主；插件只画内容。工具卡按工具名绑定到聊天 trace 行。

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, InteractiveElement as _, IntoElement as _, ParentElement as _, Styled as _,
    TestSupportExt as _, px,
};

use crate::app::DesktopApp;
use crate::features::chat::ToolCards;

use super::runtime::{PluginRuntime, VisibleSlot};

/// 工具卡容器高度上限（超出由宿主裁剪）。
const CARD_HEIGHT: f32 = 132.;
/// 会话附加区高度上限。
const ENTRY_HEIGHT: f32 = 48.;

/// 聊天区工具卡的视图表：工具名 → 已健康通过的实例。
pub(crate) struct ToolCardViews {
    views: Vec<VisibleSlot>,
}

impl ToolCardViews {
    pub(crate) fn collect(runtime: Option<&PluginRuntime>) -> Self {
        Self {
            views: runtime.map_or_else(Vec::new, PluginRuntime::visible_tool_cards),
        }
    }
}

impl ToolCards for ToolCardViews {
    fn card(&self, tool: &str, cx: &App) -> Option<AnyElement> {
        self.views
            .iter()
            .find(|slot| slot.binding.as_deref() == Some(tool))
            .map(|slot| card_container(slot, cx))
    }
}

impl DesktopApp {
    /// 会话条目下的插件附加区；没有实例时不占位。
    pub(crate) fn plugin_entry_extra(&self, session: &str) -> Option<AnyElement> {
        let runtime = self.plugin_runtime.as_ref()?;
        let slot = runtime
            .visible_entry_extras()
            .into_iter()
            .find(|slot| slot.binding.as_deref() == Some(session))?;
        Some(
            v_flex()
                .id(format!("plugin-entry-{session}"))
                .test_support()
                .pl_2()
                .pr_1()
                .pb_1()
                .max_h(px(ENTRY_HEIGHT))
                .overflow_hidden()
                .child(slot.view.into_any_element())
                .into_any_element(),
        )
    }

    /// 输入区上方的插件触发器；没有实例时不占位。
    pub(crate) fn plugin_composer_trigger(&self) -> Option<AnyElement> {
        let runtime = self.plugin_runtime.as_ref()?;
        let slot = runtime.visible_composer_trigger()?;
        Some(
            h_flex()
                .id(format!("plugin-trigger-{}", slot.plugin))
                .test_support()
                .max_h(px(32.))
                .items_center()
                .overflow_hidden()
                .child(slot.view.into_any_element())
                .into_any_element(),
        )
    }
}

fn card_container(slot: &VisibleSlot, cx: &App) -> AnyElement {
    v_flex()
        .id(format!("plugin-card-{}-{}", slot.plugin, slot.title))
        .test_support()
        .mt_1()
        .max_h(px(CARD_HEIGHT))
        .overflow_hidden()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .child(
            v_flex()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .child(slot.view.clone().into_any_element()),
        )
        .into_any_element()
}
