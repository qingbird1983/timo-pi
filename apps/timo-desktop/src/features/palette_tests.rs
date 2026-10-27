#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::InsertPromptAction;
use crate::app::tests::open;
use crate::bridge::BridgeEvent;
use gpui_kit::AppContext as _;
use gpui_kit::TestAppContext;
use gpui_kit::test::TestWindowExt as _;
use timo_kernel::{CommandSource, CommandStatus, SlashCommand, UsageSummary, UsageTotals};

#[gpui_kit::test]
fn palette_toggles_with_shortcut(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    crate::i18n::init();
    let (handle, app, events_tx, _commands) = open(cx);
    events_tx
        .send(BridgeEvent::Ready {
            model: "deepseek/deepseek-chat".into(),
            sessions: vec![],
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        app.update(cx, |app, cx| app.focus_composer(window, cx));
        window.press("ctrl-k", cx);
        window.render_frame(cx);
        assert!(app.read(cx).palette_open);
        assert!(window.try_find("palette-panel").is_some());
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(window.try_find("palette-panel").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn palette_clears_query_on_reopen(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    crate::i18n::init();
    let (handle, app, events_tx, _commands) = open(cx);
    events_tx
        .send(BridgeEvent::Ready {
            model: "deepseek/deepseek-chat".into(),
            sessions: vec![],
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        app.update(cx, |app, cx| app.focus_composer(window, cx));
        window.press("ctrl-k", cx);
        window.render_frame(cx);
        let palette = app.read(cx).palette.clone();
        palette.update(cx, |state, cx| state.set_query("设置", window, cx));
        window.render_frame(cx);
        window.press("escape", cx);
        window.render_frame(cx);
        app.update(cx, |app, cx| app.focus_composer(window, cx));
        window.press("ctrl-k", cx);
        window.render_frame(cx);
        assert!(app.read(cx).palette_open);
        let query = app.read(cx).palette.read(cx).query(cx);
        assert_eq!(query.as_ref(), "");
    })
    .unwrap();
}

#[gpui_kit::test]
fn commands_and_usage_events_reach_the_shell(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    crate::i18n::init();
    let (handle, app, events_tx, _commands) = open(cx);
    events_tx
        .send(BridgeEvent::Ready {
            model: "deepseek/deepseek-chat".into(),
            sessions: vec![],
        })
        .unwrap();
    events_tx
        .send(BridgeEvent::Commands(vec![SlashCommand {
            name: "skill:pdf".into(),
            description: "does pdf".into(),
            argument_hint: None,
            source: CommandSource::Skill,
            status: CommandStatus::Ready,
        }]))
        .unwrap();
    events_tx
        .send(BridgeEvent::Usage(UsageSummary {
            totals: UsageTotals {
                input: 10,
                output: 5,
                ..UsageTotals::default()
            },
            buckets: vec![],
        }))
        .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(app.read(cx).slash_commands.len(), 1);
        assert_eq!(app.read(cx).usage.totals.input, 10);
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        app.update(cx, |app, cx| {
            app.palette_open = true;
            app.on_insert_prompt(
                &InsertPromptAction {
                    text: "/skill:pdf ".into(),
                },
                window,
                cx,
            );
        });
        assert_eq!(app.read(cx).input.read(cx).value(), "/skill:pdf ");
        assert!(!app.read(cx).palette_open);
    })
    .unwrap();
}
