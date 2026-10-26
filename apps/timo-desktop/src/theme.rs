//! 明暗外观：只通过 gpui-kit 的语义 token 生效（P3-D11），不写裸色值。

use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{App, Window, px};

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

/// 按设置应用外观；`System` 走系统外观，明/暗强制对应模式。
pub fn apply(
    pref: crate::settings::ThemePref,
    font_scale: f32,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    match pref {
        crate::settings::ThemePref::System => Theme::sync_system_appearance(window, cx),
        crate::settings::ThemePref::Light => set_dark(false, window, cx),
        crate::settings::ThemePref::Dark => set_dark(true, window, cx),
    }
    set_font_scale(font_scale, cx);
}

/// 调整界面字号（0.9~1.3）；主题字体尺寸只走语义 token，不写裸值。
pub fn set_font_scale(scale: f32, cx: &mut App) {
    let scale = scale.clamp(0.9, 1.3);
    Theme::update(cx, |theme| {
        theme.font_size = px(16. * scale);
        theme.mono_font_size = px(13. * scale);
    });
}
