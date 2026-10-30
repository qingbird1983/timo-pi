//! 已安装插件页：分组列表、启停、挡位与卸载（设计 01 §5、§7）。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _, WindowExt as _,
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use timo_kernel::plugins::{PermissionTier, PluginEntry, PluginKind, PluginState};

use crate::app::DesktopApp;
use crate::bridge::BridgeCommand;

use super::diagnostics::diagnostics_card;

impl DesktopApp {
    pub(crate) fn installed_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let actions = vec![
            Button::new("plugins-refresh")
                .ghost()
                .small()
                .icon(IconName::Globe)
                .label(t!("plugins.refresh").to_string())
                .on_click(cx.listener(|this, _, _, _| {
                    let _ = this.commands.send(BridgeCommand::ListPlugins);
                }))
                .into_any_element(),
            Button::new("plugins-add")
                .primary()
                .small()
                .icon(IconName::Plus)
                .label(t!("plugins.add").to_string())
                .on_click(cx.listener(|this, _, _, cx| this.pick_plugin_dir(cx)))
                .into_any_element(),
            Button::new("plugins-preview")
                .ghost()
                .small()
                .icon(IconName::Eye)
                .label(t!("plugins.preview").to_string())
                .on_click(cx.listener(|this, _, window, cx| this.open_nsh_page(window, cx)))
                .into_any_element(),
            Button::new("plugins-close")
                .ghost()
                .small()
                .icon(IconName::Delete)
                .label(t!("plugins.close").to_string())
                .on_click(cx.listener(|this, _, window, cx| this.close_plugins(window, cx)))
                .into_any_element(),
        ];
        let mut body = vec![self.plugins_head(
            t!("plugins.title").to_string(),
            t!("plugins.subtitle").to_string(),
            actions,
            cx,
        )];
        body.push(self.installed_tabs(cx));
        if self.pending_install.is_some() {
            body.push(self.install_wizard(cx));
        }
        if let Some(error) = &self.plugins_error {
            body.push(banner(error.clone(), cx));
        }
        if let Some(note) = &self.notice {
            body.push(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(note.clone())
                    .into_any_element(),
            );
        }
        body.push(self.installed_groups(cx));
        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .p_6()
            .gap_4()
            .children(body)
            .into_any_element()
    }

    fn installed_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .gap_1()
            .child(
                Button::new("plugins-tab-installed")
                    .primary()
                    .small()
                    .label(t!("plugins.installed_tab").to_string()),
            )
            .child(
                Button::new("plugins-tab-market")
                    .ghost()
                    .small()
                    .label(t!("plugins.market_tab").to_string())
                    .on_click(cx.listener(|this, _, _, cx| this.open_market(cx))),
            )
            .child(
                Button::new("plugins-tab-discover")
                    .ghost()
                    .small()
                    .label(t!("plugins.discover_tab").to_string())
                    .on_click(cx.listener(|this, _, _, cx| this.open_market(cx))),
            )
            .into_any_element()
    }

    fn installed_groups(&self, cx: &mut Context<Self>) -> AnyElement {
        let installed: Vec<&PluginEntry> = self.plugins.iter().collect();
        v_flex()
            .id("plugins-groups")
            .flex_1()
            .min_h_0()
            .gap_2()
            .overflow_y_scroll()
            .child(group_title(t!("plugins.group_builtin").to_string(), cx))
            .child(empty_row(t!("plugins.empty_builtin").to_string(), cx))
            .child(group_title(
                t!("plugins.group_experimental").to_string(),
                cx,
            ))
            .child(empty_row(t!("plugins.empty_experimental").to_string(), cx))
            .child(group_title(t!("plugins.group_installed").to_string(), cx))
            .when(installed.is_empty(), |this| {
                this.child(
                    v_flex()
                        .px_3()
                        .py_4()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("plugins.empty_installed").to_string()),
                        )
                        .child(
                            Button::new("plugins-empty-market")
                                .ghost()
                                .small()
                                .label(t!("plugins.go_market").to_string())
                                .on_click(cx.listener(|this, _, _, cx| this.open_market(cx))),
                        ),
                )
            })
            .children({
                let mut rows = Vec::new();
                for entry in installed {
                    rows.push(self.installed_row(entry, cx));
                }
                rows
            })
            .into_any_element()
    }

    fn installed_row(&self, entry: &PluginEntry, cx: &mut Context<Self>) -> AnyElement {
        let id = entry.id.clone();
        let checked = entry.enabled;
        let weak = cx.entity().downgrade();
        let toggle_id = id.clone();
        let row = h_flex()
            .id(format!("plugin-row-{}", id))
            .px_3()
            .py_2()
            .gap_3()
            .items_center()
            .rounded_md()
            .hover(|this| this.bg(cx.theme().muted))
            .child(
                div()
                    .size(px(28.))
                    .rounded_md()
                    .bg(cx.theme().muted)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .child(if entry.kind == PluginKind::Gui {
                        "B"
                    } else {
                        "A"
                    }),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().text_sm().font_semibold().child(entry.name.clone()))
                            .child(badge(
                                if entry.kind == PluginKind::Gui {
                                    t!("plugins.badge_b").to_string()
                                } else {
                                    t!("plugins.badge_a").to_string()
                                },
                                cx,
                            ))
                            .child(badge(tier_label(entry.tier), cx)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(subtitle(entry)),
                    ),
            )
            .child(
                Checkbox::new(format!("plugin-toggle-{}", id))
                    .checked(checked)
                    .on_click(move |checked, _, cx| {
                        if let Some(app) = weak.upgrade() {
                            app.update(cx, |this, cx| {
                                let _ = this.commands.send(BridgeCommand::SetPluginEnabled {
                                    id: toggle_id.clone(),
                                    enabled: *checked,
                                });
                                let _ = cx;
                            });
                        }
                    }),
            )
            .child(
                Button::new(format!("plugin-detail-{}", id))
                    .ghost()
                    .xsmall()
                    .label(t!("plugins.detail").to_string())
                    .on_click(cx.listener({
                        let id = id.clone();
                        move |this, _, _, cx| {
                            this.plugins_selected =
                                if this.plugins_selected.as_deref() == Some(id.as_str()) {
                                    None
                                } else {
                                    Some(id.clone())
                                };
                            cx.notify();
                        }
                    })),
            );
        if self.plugins_selected.as_deref() != Some(id.as_str()) {
            return row.into_any_element();
        }
        v_flex()
            .gap_1()
            .child(row)
            .child(self.plugin_detail(entry, cx))
            .into_any_element()
    }

    fn plugin_detail(&self, entry: &PluginEntry, cx: &mut Context<Self>) -> AnyElement {
        let id = entry.id.clone();
        let mut tiers = Vec::new();
        for tier in [
            PermissionTier::ReadOnly,
            PermissionTier::Workspace,
            PermissionTier::Trusted,
        ] {
            let allowed = tier <= entry.tier;
            tiers.push(
                Button::new(format!("plugin-tier-{}-{tier}", entry.id))
                    .ghost()
                    .when(tier == entry.tier, |button| button.primary())
                    .xsmall()
                    .label(tier_label(tier))
                    .disabled(!allowed || tier == entry.tier)
                    .on_click(cx.listener({
                        let id = id.clone();
                        move |this, _, _, _| {
                            let _ = this.commands.send(BridgeCommand::SetPluginTier {
                                id: id.clone(),
                                tier,
                            });
                        }
                    }))
                    .into_any_element(),
            );
        }
        v_flex()
            .mx_6()
            .mb_2()
            .p_3()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(capabilities_line(entry)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("plugins.path", path = entry.path.display().to_string()).to_string()),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("plugins.tier_hint").to_string()),
                    )
                    .children(tiers),
            )
            .child(diagnostics_card(entry, cx))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new(format!("plugin-revoke-{}", entry.id))
                            .ghost()
                            .xsmall()
                            .label(t!("plugins.revoke").to_string())
                            .on_click(cx.listener({
                                let id = id.clone();
                                move |this, _, _, _| {
                                    let _ = this
                                        .commands
                                        .send(BridgeCommand::RevokePlugin { id: id.clone() });
                                }
                            })),
                    )
                    .child(
                        Button::new(format!("plugin-uninstall-{}", entry.id))
                            .danger()
                            .xsmall()
                            .label(t!("plugins.uninstall").to_string())
                            .on_click(cx.listener({
                                let id = id.clone();
                                let name = entry.name.clone();
                                move |this, _, window, cx| {
                                    this.confirm_uninstall(id.clone(), name.clone(), window, cx);
                                }
                            })),
                    ),
            )
            .into_any_element()
    }

    fn confirm_uninstall(
        &mut self,
        id: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let commands = self.commands.clone();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let id = id.clone();
            let commands = commands.clone();
            alert
                .title(t!("plugins.uninstall_title", name = name.clone()).to_string())
                .description(t!("plugins.uninstall_body").to_string())
                .confirm()
                .ok_text(t!("plugins.uninstall").to_string())
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .on_ok(move |_, _, _| {
                    let _ = commands.send(BridgeCommand::UninstallPlugin { id: id.clone() });
                    true
                })
        });
    }
}

fn tier_label(tier: PermissionTier) -> String {
    match tier {
        PermissionTier::ReadOnly => t!("plugins.tier_read").to_string(),
        PermissionTier::Workspace => t!("plugins.tier_workspace").to_string(),
        PermissionTier::Trusted => t!("plugins.tier_trusted").to_string(),
    }
}

fn subtitle(entry: &PluginEntry) -> String {
    let mut parts = Vec::new();
    if !entry.version.is_empty() {
        parts.push(format!("v{}", entry.version));
    }
    if entry.state == PluginState::Disabled {
        parts.push(t!("plugins.state_disabled").to_string());
    }
    if entry.state == PluginState::Error {
        parts.push(t!("plugins.state_error").to_string());
    }
    if parts.is_empty() {
        t!("plugins.state_enabled").to_string()
    } else {
        parts.join(" · ")
    }
}

fn capabilities_line(entry: &PluginEntry) -> String {
    let declared = &entry.declared;
    let mut parts = Vec::new();
    if declared.capabilities.exec {
        parts.push("exec".to_string());
    }
    if !declared.capabilities.net.domains.is_empty() {
        parts.push(format!("net({})", declared.capabilities.net.domains.len()));
    }
    if !declared.capabilities.fs.read.is_empty() {
        parts.push(format!("fs.read({})", declared.capabilities.fs.read.len()));
    }
    if !declared.capabilities.fs.write.is_empty() {
        parts.push(format!(
            "fs.write({})",
            declared.capabilities.fs.write.len()
        ));
    }
    if !declared.capabilities.fs.delete.is_empty() {
        parts.push(format!(
            "fs.delete({})",
            declared.capabilities.fs.delete.len()
        ));
    }
    if !declared.queries.is_empty() {
        parts.push(format!("queries({})", declared.queries.len()));
    }
    if !declared.actions.is_empty() {
        parts.push(format!("actions({})", declared.actions.len()));
    }
    if parts.is_empty() {
        t!("plugins.no_caps").to_string()
    } else {
        format!("{}: {}", t!("plugins.declared"), parts.join(" · "))
    }
}

fn group_title(title: String, cx: &mut Context<DesktopApp>) -> AnyElement {
    div()
        .pt_2()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(title)
        .into_any_element()
}

fn empty_row(text: String, cx: &mut Context<DesktopApp>) -> AnyElement {
    div()
        .px_3()
        .py_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}

fn badge(text: String, cx: &mut Context<DesktopApp>) -> AnyElement {
    div()
        .px_1p5()
        .py_0p5()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}

fn banner(text: String, cx: &mut Context<DesktopApp>) -> AnyElement {
    div()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().danger)
        .text_xs()
        .text_color(cx.theme().danger)
        .child(text)
        .into_any_element()
}
