//! 设置页：常规、模型、凭证、工具与关于（设计 01 §9、02 §4~§5）。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::DesktopApp;
use crate::bridge::BridgeCommand;
use crate::settings::ThemePref;

/// 设置页分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    /// 常规。
    General,
    /// 模型。
    Model,
    /// 凭证。
    Credentials,
    /// 工具。
    Tools,
    /// 关于。
    About,
}

impl SettingsTab {
    const ALL: [Self; 5] = [
        Self::General,
        Self::Model,
        Self::Credentials,
        Self::Tools,
        Self::About,
    ];

    fn label(self) -> String {
        match self {
            Self::General => t!("settings.general").to_string(),
            Self::Model => t!("settings.model").to_string(),
            Self::Credentials => t!("settings.credentials").to_string(),
            Self::Tools => t!("settings.tools").to_string(),
            Self::About => t!("settings.about").to_string(),
        }
    }
}

impl DesktopApp {
    /// 打开设置页并拉取供应商列表。
    pub(crate) fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = true;
        self.palette_open = false;
        self.provider_error = None;
        self.provider_feedback = None;
        #[cfg(feature = "nsh-poc")]
        {
            self.nsh_page = false;
        }
        let _ = self.commands.send(BridgeCommand::ListProviders);
        cx.notify();
    }

    /// 关闭设置页并把焦点还给输入框。
    pub(crate) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = false;
        self.key_provider = None;
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// 切换语言并持久化（立即生效）。
    pub(crate) fn set_locale(&mut self, locale: &str, cx: &mut Context<Self>) {
        crate::i18n::set(locale);
        self.update_settings(|settings| settings.locale = locale.to_string());
        cx.notify();
    }

    /// 调整界面字号倍率并持久化。
    pub(crate) fn set_font_scale(&mut self, scale: f32, cx: &mut Context<Self>) {
        self.font_scale = scale.clamp(0.9, 1.3);
        crate::theme::set_font_scale(self.font_scale, cx);
        self.update_settings(|settings| settings.font_scale = self.font_scale);
        cx.notify();
    }

    /// 更新审阅开关；只影响后续请求。
    pub(crate) fn set_review_prefs(&mut self, files: bool, commands: bool, cx: &mut Context<Self>) {
        self.update_settings(|settings| {
            settings.review.files = files;
            settings.review.commands = commands;
        });
        let _ = self
            .commands
            .send(BridgeCommand::SetReviewPrefs { files, commands });
        cx.notify();
    }

    /// 开始为某供应商输入密钥。
    pub(crate) fn clear_session_allow(&mut self, cx: &mut Context<Self>) {
        self.review.clear_allow();
        cx.notify();
    }

    fn tab_button(&self, tab: SettingsTab, cx: &mut Context<Self>) -> AnyElement {
        let active = self.settings_tab == tab;
        Button::new(format!("settings-tab-{:?}", tab))
            .when(active, |button| button.primary())
            .when(!active, |button| button.ghost())
            .w_full()
            .justify_start()
            .label(tab.label())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.settings_tab = tab;
                if tab == SettingsTab::Credentials {
                    let _ = this.commands.send(BridgeCommand::ListProviders);
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn settings_content(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.settings_tab {
            SettingsTab::General => self.general_settings(cx).into_any_element(),
            SettingsTab::Model => self.model_settings(cx).into_any_element(),
            SettingsTab::Credentials => self.credential_settings(cx).into_any_element(),
            SettingsTab::Tools => self.tool_settings(cx).into_any_element(),
            SettingsTab::About => self.about_settings(cx).into_any_element(),
        }
    }

    /// 设置页主体（在聊天区位置打开）。
    pub(crate) fn settings_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut tabs = Vec::with_capacity(SettingsTab::ALL.len());
        for tab in SettingsTab::ALL {
            tabs.push(self.tab_button(tab, cx));
        }
        let reset = self.settings_reset.then(|| {
            div()
                .text_xs()
                .text_color(cx.theme().warning)
                .child(t!("settings.reset").to_string())
        });
        v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .h_full()
            .track_focus(&self.settings_focus)
            .p_6()
            .gap_4()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_lg()
                            .font_semibold()
                            .child(t!("settings.title").to_string()),
                    )
                    .child(
                        Button::new("settings-close")
                            .ghost()
                            .icon(IconName::Delete)
                            .label(t!("settings.close").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_settings(window, cx);
                            })),
                    ),
            )
            .children(reset)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .gap_6()
                    .child(v_flex().w(px(160.)).gap_1().children(tabs))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_3()
                            .child(self.settings_content(cx)),
                    ),
            )
            .into_any_element()
    }

    fn general_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self
            .settings
            .lock()
            .map(|s| s.locale.clone())
            .unwrap_or_default();
        let theme_pref = self.theme_pref;
        let scale = self.font_scale;
        v_flex()
            .gap_4()
            .child(
                v_flex()
                    .gap_2()
                    .child(section_title(t!("settings.language").to_string()))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(language_button("zh-CN", "中文", &locale, cx))
                            .child(language_button("en", "English", &locale, cx)),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(section_title(t!("settings.theme").to_string()))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(theme_button(ThemePref::System, theme_pref, cx))
                            .child(theme_button(ThemePref::Light, theme_pref, cx))
                            .child(theme_button(ThemePref::Dark, theme_pref, cx)),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(section_title(t!("settings.font_scale").to_string()))
                    .child(h_flex().gap_2().children([0.9, 1.0, 1.1, 1.2].map(|value| {
                        let active = (scale - value).abs() < 0.001;
                        Button::new(format!("font-scale-{value}"))
                            .when(active, |button| button.primary())
                            .when(!active, |button| button.outline())
                            .small()
                            .label(format!("{}%", (value * 100.) as i32))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_font_scale(value, cx);
                            }))
                            .into_any_element()
                    }))),
            )
    }

    fn model_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self
            .model
            .clone()
            .unwrap_or_else(|| t!("status.disconnected").to_string());
        let mut rows = Vec::new();
        for provider in &self.providers {
            rows.push(
                h_flex()
                    .gap_2()
                    .text_sm()
                    .child(div().font_semibold().child(provider.id.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(provider.models.join(" · ")),
                    )
                    .into_any_element(),
            );
        }
        v_flex()
            .gap_3()
            .child(section_title(t!("settings.current_model").to_string()))
            .child(div().text_sm().child(current))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("settings.model_hint").to_string()),
            )
            .child(section_title(t!("settings.available_models").to_string()))
            .child(v_flex().gap_2().children(rows))
    }

    fn tool_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (files, commands) = self
            .settings
            .lock()
            .map(|settings| (settings.review.files, settings.review.commands))
            .unwrap_or((true, true));
        let allow_count = self.review.allow.len();
        let weak_files = cx.entity().downgrade();
        let weak_commands = cx.entity().downgrade();
        v_flex()
            .gap_4()
            .child(section_title(t!("settings.review").to_string()))
            .child(
                Checkbox::new("review-files")
                    .checked(files)
                    .label(t!("settings.review_files").to_string())
                    .on_click(move |checked, _, cx| {
                        if let Some(app) = weak_files.upgrade() {
                            app.update(cx, |this, cx| {
                                let commands = this
                                    .settings
                                    .lock()
                                    .map(|settings| settings.review.commands)
                                    .unwrap_or(true);
                                this.set_review_prefs(*checked, commands, cx);
                            });
                        }
                    }),
            )
            .child(
                Checkbox::new("review-commands")
                    .checked(commands)
                    .label(t!("settings.review_commands").to_string())
                    .on_click(move |checked, _, cx| {
                        if let Some(app) = weak_commands.upgrade() {
                            app.update(cx, |this, cx| {
                                let files = this
                                    .settings
                                    .lock()
                                    .map(|settings| settings.review.files)
                                    .unwrap_or(true);
                                this.set_review_prefs(files, *checked, cx);
                            });
                        }
                    }),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("settings.session_allow", n = allow_count).to_string()),
                    )
                    .child(
                        Button::new("clear-session-allow")
                            .small()
                            .outline()
                            .label(t!("settings.clear_allow").to_string())
                            .on_click(cx.listener(|this, _, _, cx| this.clear_session_allow(cx))),
                    ),
            )
    }

    fn about_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(section_title(t!("settings.about").to_string()))
            .child(
                div()
                    .text_sm()
                    .child(format!("Timo v{}", env!("CARGO_PKG_VERSION"))),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.model.clone().unwrap_or_default()),
            )
    }
}

pub(super) fn section_title(text: String) -> AnyElement {
    div()
        .text_xs()
        .font_semibold()
        .child(text)
        .into_any_element()
}

fn language_button(
    locale: &'static str,
    label: &'static str,
    current: &str,
    cx: &mut Context<DesktopApp>,
) -> AnyElement {
    let active = current == locale;
    Button::new(format!("locale-{locale}"))
        .when(active, |button| button.primary())
        .when(!active, |button| button.outline())
        .small()
        .label(label)
        .on_click(cx.listener(move |this, _, _, cx| this.set_locale(locale, cx)))
        .into_any_element()
}

fn theme_button(pref: ThemePref, current: ThemePref, cx: &mut Context<DesktopApp>) -> AnyElement {
    let label = match pref {
        ThemePref::System => t!("settings.theme_system").to_string(),
        ThemePref::Light => t!("settings.theme_light").to_string(),
        ThemePref::Dark => t!("settings.theme_dark").to_string(),
    };
    let active = current == pref;
    Button::new(format!("theme-{:?}", pref))
        .when(active, |button| button.primary())
        .when(!active, |button| button.outline())
        .small()
        .label(label)
        .on_click(cx.listener(move |this, _, window, cx| {
            this.apply_theme(pref, Some(window), cx);
        }))
        .into_any_element()
}
