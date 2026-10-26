//! 明暗外观：只通过 gpui-kit 的语义 token 生效（P3-D11），不写裸色值。

use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{App, Window};

/// 当前是否为深色外观。
pub fn is_dark(cx: &App) -> bool {
    cx.theme().mode.is_dark()
}

/// 应用明暗外观；`window` 为 `None` 时由全局主题刷新所有窗口。
pub fn set_dark(dark: bool, window: Option<&mut Window>, cx: &mut App) {
    let mode = if dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    Theme::change(mode, window, cx);
}
