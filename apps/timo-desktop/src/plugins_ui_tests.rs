//! 插件市场/已安装页的 UI 冒烟：入口、卡片按钮与详情按钮可渲染且可点击。
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use crate::app::DesktopApp;
use crate::bridge::{BridgeEvent, MarketSnapshot};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size};
use timo_kernel::plugins::market::{MarketState, parse_index};
use timo_kernel::plugins::{
    Declared, PermissionTier, PluginEntry, PluginKind, PluginOrigin, PluginSource, PluginState,
};

fn entry() -> PluginEntry {
    PluginEntry {
        id: "session-search".into(),
        kind: PluginKind::Gui,
        origin: PluginOrigin::Installed,
        name: "会话搜索".into(),
        version: "1.0.0".into(),
        enabled: true,
        tier: PermissionTier::ReadOnly,
        declared: Declared::default(),
        state: PluginState::Enabled,
        source: PluginSource::LocalFile,
        source_hash: String::new(),
        configurable: false,
        diagnostics: vec![],
        path: PathBuf::from("plugins/session-search"),
    }
}

fn index() -> timo_kernel::plugins::market::MarketIndex {
    let text = serde_json::json!({
        "schema": 1,
        "market_version": "1.0",
        "updated_at": "2026-10-28T12:00:00Z",
        "plugins": [{
            "id": "session-search",
            "name": "会话搜索",
            "kind": "gui",
            "version": "1.0.0",
            "author": { "name": "timo" },
            "description": "在侧栏搜索历史会话。",
            "categories": ["会话与消息"],
            "screenshots": ["https://example.com/a.png"],
            "stats": { "downloads_30d": 12, "stars": 3 },
            "requires": { "timo": ">=0.4.0", "api": 1 },
            "package": {
                "url": "https://example.com/session-search.nshpkg",
                "sha256": "a".repeat(64)
            },
            "discussion": "https://example.com/discuss"
        }, {
            "id": "usage-board",
            "name": "用量看板",
            "kind": "gui",
            "version": "0.4.2",
            "author": { "name": "community" },
            "description": "统计 Token 与费用。",
            "categories": ["用量与计费"],
            "screenshots": [],
            "stats": { "downloads_30d": 8, "stars": 2 },
            "requires": { "timo": ">=0.4.0", "api": 1 },
            "package": {
                "url": "https://example.com/usage.nshpkg",
                "sha256": "b".repeat(64)
            }
        }]
    })
    .to_string();
    parse_index(&text).unwrap().index
}

#[gpui_kit::test]
fn market_and_installed_entries_render_their_actions(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    crate::i18n::init();

    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let (command_tx, _command_rx) = tokio::sync::mpsc::unbounded_channel();
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
            |window, cx| cx.new(|cx| DesktopApp::with_bridge(command_tx, event_rx, window, cx)),
        )
        .expect("open test window")
    });

    event_tx.send(BridgeEvent::Plugins(vec![entry()])).unwrap();
    event_tx
        .send(BridgeEvent::Market(MarketSnapshot {
            index: Some(index()),
            warnings: vec![],
            offline: None,
            state: MarketState::default(),
            index_url: Some("https://example.com/index.json".into()),
        }))
        .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("nav-market", cx);
        window.render_frame(cx);
        for id in [
            "market-tab-Discover",
            "market-tab-Advanced",
            "market-chip-all",
            "market-open-session-search",
            "market-install-usage-board",
            "market-fav-session-search",
            "market-block-session-search",
        ] {
            assert!(window.find(id).visible(), "missing {id}");
        }
        let install = window.find("market-install-usage-board");
        assert_eq!(install.disabled(), Some(true));

        window.click("market-tab-Advanced", cx);
        window.render_frame(cx);
        assert!(window.find("market-url-save").visible());
        assert!(window.find("market-url-clear").visible());

        window.click("nav-installed", cx);
        window.render_frame(cx);
        assert!(window.find("plugins-refresh").visible());
        assert!(window.find("plugins-add").visible());
        assert!(window.find("plugin-detail-session-search").visible());
        assert!(window.find("plugin-toggle-session-search").visible());

        window.click("plugin-detail-session-search", cx);
        window.render_frame(cx);
        for id in [
            "plugin-tier-session-search-read-only",
            "plugin-revoke-session-search",
            "plugin-uninstall-session-search",
        ] {
            assert!(window.find(id).visible(), "missing {id}");
        }
        assert!(
            window
                .find("plugin-tier-session-search-workspace")
                .disabled()
                .is_some_and(|disabled| disabled)
                || !window
                    .find("plugin-tier-session-search-workspace")
                    .visible()
        );
    })
    .unwrap();

    let _ = app;
}
