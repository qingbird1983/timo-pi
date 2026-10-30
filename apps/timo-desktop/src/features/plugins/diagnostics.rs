//! 插件详情里的「诊断」卡（设计 03 §5）：最近 50 条，不展示堆栈。

use gpui_kit::component::{ActiveTheme as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::*;
use timo_kernel::plugins::PluginEntry;

use crate::app::DesktopApp;

/// 诊断卡：运行期已按「首条 + 计数」去重，这里只负责展示。
pub(crate) fn diagnostics_card(entry: &PluginEntry, cx: &mut Context<DesktopApp>) -> AnyElement {
    let start = entry.diagnostics.len().saturating_sub(50);
    let mut rows: Vec<AnyElement> = entry.diagnostics[start..]
        .iter()
        .map(|diagnostic| {
            h_flex()
                .items_baseline()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(diagnostic.code.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(diagnostic.message.clone()),
                )
                .into_any_element()
        })
        .collect();
    if rows.is_empty() {
        rows.push(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(t!("plugins.diagnostics_empty").to_string())
                .into_any_element(),
        );
    }
    v_flex()
        .id(format!("plugin-diagnostics-{}", entry.id))
        .test_support()
        .gap_1()
        .p_2()
        .rounded_md()
        .bg(cx.theme().muted)
        .child(
            div()
                .text_xs()
                .font_semibold()
                .child(t!("plugins.diagnostics").to_string()),
        )
        .children(rows)
        .into_any_element()
}
