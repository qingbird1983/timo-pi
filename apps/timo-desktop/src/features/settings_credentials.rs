//! 设置页的凭证分组：钥匙串写入、移除与状态展示（设计 02 §5）。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, WindowExt as _, h_flex,
    v_flex,
};
use gpui_kit::*;

use crate::app::DesktopApp;
use crate::bridge::{BridgeCommand, CredentialInfo, ProviderInfo};

impl DesktopApp {
    pub(crate) fn start_key(
        &mut self,
        provider: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.key_provider = Some(provider);
        self.key_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        window.focus(&self.key_input.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// 保存密钥：写钥匙串并改写 models.json 引用，然后立即清空输入。
    pub(crate) fn save_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(provider) = self.key_provider.clone() else {
            return;
        };
        let secret = self.key_input.read(cx).value().trim().to_string();
        if secret.is_empty() {
            return;
        }
        let _ = self.commands.send(BridgeCommand::SaveProviderKey {
            provider: provider.clone(),
            secret,
        });
        self.key_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.key_provider = None;
        self.provider_error = None;
        self.provider_feedback = None;
        cx.notify();
    }

    /// 移除某供应商的钥匙串条目（保留配置引用）。
    pub(crate) fn confirm_remove_key(
        &mut self,
        provider: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let commands = self.commands.clone();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let provider = provider.clone();
            let commands = commands.clone();
            alert
                .title(t!("settings.remove_key_title", provider = provider.clone()).to_string())
                .description(t!("settings.remove_key_body").to_string())
                .confirm()
                .ok_text(t!("settings.remove").to_string())
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .on_ok(move |_, _, _| {
                    let _ = commands.send(BridgeCommand::RemoveProviderKey {
                        provider: provider.clone(),
                    });
                    true
                })
        });
    }

    pub(crate) fn credential_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows = Vec::new();
        for provider in &self.providers {
            rows.push(self.provider_row(provider, cx));
        }
        let mut body = v_flex()
            .gap_3()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("settings.credentials_hint").to_string()),
            )
            .child(v_flex().gap_3().children(rows));
        if let Some(provider) = &self.key_provider {
            body = body.child(
                v_flex()
                    .gap_2()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .p_3()
                    .child(
                        div()
                            .text_sm()
                            .child(t!("settings.key_for", provider = provider.clone()).to_string()),
                    )
                    .child(Input::new(&self.key_input).w_full())
                    .child(
                        h_flex()
                            .gap_2()
                            .justify_end()
                            .child(
                                Button::new("key-cancel")
                                    .ghost()
                                    .label(t!("settings.cancel").to_string())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.key_provider = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("key-save")
                                    .primary()
                                    .label(t!("settings.save").to_string())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_key(window, cx);
                                    })),
                            ),
                    ),
            );
        }
        if let Some(error) = &self.provider_error {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        if let Some(feedback) = &self.provider_feedback {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().success)
                    .child(feedback.clone()),
            );
        }
        body.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(t!("settings.keychain_hint").to_string()),
        )
    }

    fn provider_row(&self, provider: &ProviderInfo, cx: &mut Context<Self>) -> AnyElement {
        let id = provider.id.clone();
        let start_id = id.clone();
        let remove_id = id.clone();
        let status = match &provider.credential {
            CredentialInfo::Keychain { service, account } => t!(
                "settings.cred_keychain",
                service = service.clone(),
                account = account.clone()
            )
            .to_string(),
            CredentialInfo::Env(name) => t!("settings.cred_env", name = name.clone()).to_string(),
            CredentialInfo::Command => t!("settings.cred_command").to_string(),
            CredentialInfo::Literal => t!("settings.cred_literal").to_string(),
            CredentialInfo::Builtin => t!("settings.cred_builtin").to_string(),
            CredentialInfo::None => t!("settings.cred_none").to_string(),
        };
        h_flex()
            .items_center()
            .gap_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(div().text_sm().font_semibold().child(id))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(status),
                    ),
            )
            .child(
                Button::new(format!("provider-set-{start_id}"))
                    .small()
                    .outline()
                    .label(if provider.editable {
                        t!("settings.set_key").to_string()
                    } else {
                        t!("settings.unavailable").to_string()
                    })
                    .disabled(!provider.editable)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.start_key(start_id.clone(), window, cx);
                    })),
            )
            .child(
                Button::new(format!("provider-remove-{remove_id}"))
                    .small()
                    .ghost()
                    .label(t!("settings.remove").to_string())
                    .disabled(!provider.editable)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.confirm_remove_key(remove_id.clone(), window, cx);
                    })),
            )
            .into_any_element()
    }
}
