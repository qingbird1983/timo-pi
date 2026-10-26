//! 桌面壳 UI 冒烟：骨架可渲染、侧栏宽度符合设计、明暗切换真实生效、双语文案可切。
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size};

use timo_desktop::app::{DesktopApp, SIDEBAR_WIDTH};

#[gpui_kit::test]
fn shell_renders_and_toggles_theme(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    timo_desktop::i18n::init();

    let (handle, app) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1280.), px(800.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| DesktopApp::new(window, cx)),
        )
        .expect("open test window")
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);

        let sidebar = window.find("sidebar");
        assert!(sidebar.visible());
        assert_eq!(sidebar.bounds().size.width, SIDEBAR_WIDTH);

        window.click("theme-toggle", cx);
        assert!(timo_desktop::theme::is_dark(cx));
        assert!(app.read(cx).is_dark());

        window.click("theme-toggle", cx);
        assert!(!timo_desktop::theme::is_dark(cx));
        assert!(!app.read(cx).is_dark());
    })
    .unwrap();

    assert_eq!(timo_desktop::i18n::text("sidebar.title"), "会话");
    timo_desktop::i18n::set("en");
    assert_eq!(timo_desktop::i18n::current(), "en");
    assert_eq!(timo_desktop::i18n::text("sidebar.title"), "Sessions");
    assert_eq!(timo_desktop::i18n::text("status.ready"), "Ready");
    timo_desktop::i18n::init();
}
