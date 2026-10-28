//! 命令面板（Ctrl/Cmd+K；设计 01 §8）。

use gpui_kit::component::command::{Command, CommandGroup, CommandItem};
use gpui_kit::component::{ActiveTheme as _, IconName, v_flex};
use gpui_kit::*;

use timo_kernel::{CommandSource, CommandStatus};

use crate::app::DesktopApp;
use crate::features::settings_view::SettingsTab;

actions!(
    timo_desktop,
    [
        TogglePalette,
        NewSessionAction,
        DeleteCurrentSessionAction,
        StopAction,
        ResendAction,
        FocusComposerAction,
        OpenSettingsAction,
        OpenModelSettingsAction,
        ToggleLocaleAction,
        ToggleThemeAction,
        AcceptReviewAction,
        RejectReviewAction,
        CloseOverlayAction,
    ]
);

/// 切换到指定会话。
#[derive(Clone, PartialEq, Eq, Debug, Action)]
#[action(namespace = timo_desktop, no_json)]
pub struct OpenSessionAction {
    /// 会话 id。
    pub id: String,
}

/// 把一段命令文本插入输入框（技能命令）。
#[derive(Clone, PartialEq, Eq, Debug, Action)]
#[action(namespace = timo_desktop, no_json)]
pub struct InsertPromptAction {
    /// 要插入的文本。
    pub text: String,
}

impl DesktopApp {
    /// 打开/关闭命令面板。
    pub(crate) fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = !self.palette_open;
        if self.palette_open {
            self.settings_open = false;
            #[cfg(feature = "nsh-poc")]
            {
                self.nsh_page = false;
            }
            self.palette.update(cx, |state, cx| {
                state.set_query("", window, cx);
                state.focus(window, cx);
            });
        } else {
            // 关闭后把焦点还给输入框，保证全局快捷键继续可用。
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    /// 命令面板浮层（不遮挡时为空）。
    pub(crate) fn command_palette(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.palette_open {
            return div().into_any_element();
        }
        let weak_close = cx.entity().downgrade();
        let mut sessions = CommandGroup::new().label(t!("palette.sessions").to_string());
        for session in &self.sessions {
            let title = if session.first_message.trim().is_empty() {
                t!("chat.empty_session").to_string()
            } else {
                session.first_message.clone()
            };
            sessions = sessions.item(
                CommandItem::new()
                    .label(title)
                    .icon(IconName::Inbox)
                    .action(Box::new(OpenSessionAction {
                        id: session.id.clone(),
                    })),
            );
        }
        let session_group = sessions
            .item(
                CommandItem::new()
                    .label(t!("palette.new_session").to_string())
                    .icon(IconName::Plus)
                    .action(Box::new(NewSessionAction)),
            )
            .item(
                CommandItem::new()
                    .label(t!("palette.delete_session").to_string())
                    .icon(IconName::Delete)
                    .action(Box::new(DeleteCurrentSessionAction)),
            );
        let skills: Vec<CommandItem> = self
            .slash_commands
            .iter()
            .filter(|command| {
                command.source == CommandSource::Skill && command.status == CommandStatus::Ready
            })
            .map(|command| {
                CommandItem::new()
                    .label(format!("/{}", command.name))
                    .icon(IconName::Bot)
                    .action(Box::new(InsertPromptAction {
                        text: format!("/{} ", command.name),
                    }))
            })
            .collect();
        let palette = Command::new(&self.palette)
            .placeholder(t!("palette.placeholder").to_string())
            .max_h(px(360.))
            .bordered(false)
            .group(session_group);
        let palette = if skills.is_empty() {
            palette
        } else {
            palette.group(
                CommandGroup::new()
                    .label(t!("palette.skills").to_string())
                    .items(skills),
            )
        };
        let palette = palette
            .group(
                CommandGroup::new()
                    .label(t!("palette.run").to_string())
                    .item(
                        CommandItem::new()
                            .label(t!("palette.stop").to_string())
                            .icon(IconName::Square)
                            .action(Box::new(StopAction)),
                    )
                    .item(
                        CommandItem::new()
                            .label(t!("palette.resend").to_string())
                            .icon(IconName::Undo2)
                            .action(Box::new(ResendAction)),
                    )
                    .item(
                        CommandItem::new()
                            .label(t!("palette.focus").to_string())
                            .icon(IconName::Search)
                            .action(Box::new(FocusComposerAction)),
                    ),
            )
            .group(
                CommandGroup::new()
                    .label(t!("palette.ui").to_string())
                    .item(
                        CommandItem::new()
                            .label(t!("palette.settings").to_string())
                            .icon(IconName::Settings)
                            .action(Box::new(OpenSettingsAction)),
                    )
                    .item(
                        CommandItem::new()
                            .label(t!("palette.toggle_locale").to_string())
                            .icon(IconName::Globe)
                            .action(Box::new(ToggleLocaleAction)),
                    )
                    .item(
                        CommandItem::new()
                            .label(t!("palette.toggle_theme").to_string())
                            .icon(IconName::Sun)
                            .action(Box::new(ToggleThemeAction)),
                    ),
            )
            .group(
                CommandGroup::new()
                    .label(t!("palette.model").to_string())
                    .item(
                        CommandItem::new()
                            .label(t!("palette.choose_model").to_string())
                            .icon(IconName::Bot)
                            .action(Box::new(OpenModelSettingsAction)),
                    ),
            )
            .on_cancel(move |window, cx| {
                if let Some(app) = weak_close.upgrade() {
                    app.update(cx, |this, cx| {
                        this.palette_open = false;
                        this.focus_composer(window, cx);
                        cx.notify();
                    });
                }
            });

        div()
            .id("palette-overlay")
            .test_support()
            .absolute()
            .inset_0()
            .occlude()
            .bg(cx.theme().overlay)
            .on_click(cx.listener(|this, _, window, cx| {
                this.palette_open = false;
                this.focus_composer(window, cx);
                cx.notify();
            }))
            .child(
                v_flex()
                    .absolute()
                    .top(px(96.))
                    .left_0()
                    .right_0()
                    .items_center()
                    .child(
                        v_flex()
                            .id("palette-panel")
                            .test_support()
                            .w(px(560.))
                            .occlude()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().background)
                            .shadow_xl()
                            .on_click(|_, _, _| {})
                            .child(palette),
                    ),
            )
            .into_any_element()
    }

    /// 面板里选中技能：插入 `/skill:name ` 并聚焦输入框。
    pub(crate) fn on_insert_prompt(
        &mut self,
        action: &InsertPromptAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_open = false;
        let text = action.text.clone();
        self.input
            .update(cx, |input, cx| input.insert(&text, window, cx));
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(crate) fn on_toggle_palette(
        &mut self,
        _: &TogglePalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_palette(window, cx);
    }

    pub(crate) fn on_open_settings(
        &mut self,
        _: &OpenSettingsAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_open = false;
        self.settings_tab = SettingsTab::General;
        self.open_settings(cx);
        window.focus(&self.settings_focus, cx);
    }

    pub(crate) fn on_open_model(
        &mut self,
        _: &OpenModelSettingsAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_open = false;
        self.settings_tab = SettingsTab::Model;
        self.open_settings(cx);
        window.focus(&self.settings_focus, cx);
    }
}

impl DesktopApp {
    /// 新建会话（命令面板）。
    pub(crate) fn on_new_session(
        &mut self,
        _: &NewSessionAction,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_open = false;
        self.new_session(cx);
    }

    /// 删除当前会话（先确认）。
    pub(crate) fn on_delete_current(
        &mut self,
        _: &DeleteCurrentSessionAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_open = false;
        if let Some(id) = self.session_id.clone() {
            let title = self.session_title.clone();
            self.confirm_delete(id, title, window, cx);
        }
    }

    /// 取消当前任务。
    pub(crate) fn on_stop(&mut self, _: &StopAction, _: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = false;
        self.stop(cx);
    }

    /// 重发上一条用户消息。
    pub(crate) fn on_resend(&mut self, _: &ResendAction, _: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = false;
        self.resend_last(cx);
    }

    /// 聚焦输入框。
    pub(crate) fn on_focus_composer(
        &mut self,
        _: &FocusComposerAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_open = false;
        self.focus_composer(window, cx);
    }

    /// 中英切换。
    pub(crate) fn on_toggle_locale(
        &mut self,
        _: &ToggleLocaleAction,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_open = false;
        let next = if crate::i18n::current() == "en" {
            "zh-CN"
        } else {
            "en"
        };
        self.set_locale(next, cx);
    }

    /// 明暗切换。
    pub(crate) fn on_toggle_theme(
        &mut self,
        _: &ToggleThemeAction,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_open = false;
        let pref = if crate::theme::is_dark(cx) {
            crate::settings::ThemePref::Light
        } else {
            crate::settings::ThemePref::Dark
        };
        self.apply_theme(pref, None, cx);
    }

    /// 切换到指定会话。
    pub(crate) fn on_open_session(
        &mut self,
        action: &OpenSessionAction,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_open = false;
        self.open_session(action.id.clone(), cx);
    }

    /// 接受当前审阅（Ctrl/Cmd+Enter）。
    pub(crate) fn on_accept_review(
        &mut self,
        _: &AcceptReviewAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.review.open {
            self.accept_review(window, cx);
        }
    }

    /// 拒绝当前审阅（Ctrl/Cmd+Backspace）。
    pub(crate) fn on_reject_review(
        &mut self,
        _: &RejectReviewAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.review.open {
            self.reject_review(window, cx);
        }
    }

    /// Esc：先关命令面板，再关设置页；审阅由 sheet 自己处理。
    pub(crate) fn on_close_overlay(
        &mut self,
        _: &CloseOverlayAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette_open {
            self.palette_open = false;
            self.focus_composer(window, cx);
            cx.notify();
        } else if !self.review.open && self.settings_open {
            self.close_settings(window, cx);
        } else if !self.review.open && self.plugins_view.is_some() {
            self.close_plugins(window, cx);
        } else if !self.review.open && self.nsh_page_open() {
            self.close_nsh_page(window, cx);
        }
    }
}

#[cfg(test)]
#[path = "palette_tests.rs"]
mod tests;
