//! Timo 桌面壳（Phase 3）。
//!
//! 只依赖 `gpui-kit` 门面，不把 GPUI/NSH 带进内核；内核经桥接层接入（后续批次）。
//! 版本边界见 `docs/design/phase-3/00-overview.md` §3，布局见设计 01。

#[macro_use]
extern crate rust_i18n;

pub mod app;
mod bridge;
mod features;
pub mod i18n;
pub mod theme;

use gpui_kit::component::TitleBar;
use gpui_kit::*;

/// 打开主窗口并进入事件循环。
pub fn run() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            i18n::init();

            let mut options = TitleBar::window_options();
            if let Some(titlebar) = options.titlebar.as_mut() {
                titlebar.title = Some(t!("app.title").to_string().into());
            }
            options.window_bounds = Some(WindowBounds::centered(size(px(1280.), px(800.)), cx));
            options.window_min_size = Some(size(px(960.), px(600.)));

            if let Err(error) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| app::DesktopApp::new(window, cx))
            }) {
                eprintln!("failed to open the Timo window: {error}");
                cx.quit();
            }
        });
}

i18n!("locales", fallback = "zh-CN");
