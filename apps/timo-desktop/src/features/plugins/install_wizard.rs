//! 本地安装向导：选目录 → 验签 → 确认挡位 → 内核落盘（设计 01 §6、02 §4）。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use timo_kernel::plugins::PermissionTier;

use crate::app::DesktopApp;
use crate::bridge::BridgeCommand;

use super::nsh::install::verify_dir;

/// 三档挡位与它们的按钮 id 片段、文案键。
const TIERS: [(PermissionTier, &str, &str); 3] = [
    (PermissionTier::ReadOnly, "read-only", "plugins.tier_read"),
    (
        PermissionTier::Workspace,
        "workspace",
        "plugins.tier_workspace",
    ),
    (PermissionTier::Trusted, "trusted", "plugins.tier_trusted"),
];

impl DesktopApp {
    /// 「添加插件」第一步：打开原生目录选择框，选中后立刻验签。
    pub(crate) fn pick_plugin_dir(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t!("plugins.pick_dir").to_string().into()),
        });
        cx.spawn(async move |this, cx| {
            let picked = match receiver.await {
                Ok(Ok(Some(mut paths))) => paths.pop(),
                _ => None,
            };
            let Some(dir) = picked else {
                return;
            };
            let outcome = timo_kernel::data_dir()
                .map_err(|error| error.to_string())
                .and_then(|data_dir| verify_dir(&data_dir, &dir));
            let _ = this.update(cx, |this, cx| {
                match outcome {
                    Ok(candidate) => {
                        this.plugins_error = None;
                        this.pending_install = Some(candidate);
                    }
                    Err(message) => {
                        this.pending_install = None;
                        this.plugins_error = Some(message);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 取消安装：丢掉候选，不落任何文件（设计 01 §6「取消任意一步 → 不留文件」）。
    pub(crate) fn cancel_install(&mut self, cx: &mut Context<Self>) {
        self.pending_install = None;
        cx.notify();
    }

    /// 第二步：按选定挡位落盘；内核负责回滚，这里只报告结果。
    pub(crate) fn confirm_install(&mut self, tier: PermissionTier, cx: &mut Context<Self>) {
        let Some(candidate) = self.pending_install.take() else {
            return;
        };
        let outcome = timo_kernel::data_dir()
            .map_err(|error| error.to_string())
            .and_then(|data_dir| candidate.commit(&data_dir, tier));
        match outcome {
            Ok(id) => {
                self.plugins_error = None;
                self.notice = Some(t!("plugins.installed", id = id).to_string());
            }
            Err(error) => {
                self.plugins_error = Some(error);
                self.notice = None;
            }
        }
        let _ = self.commands.send(BridgeCommand::ListPlugins);
        cx.notify();
    }

    /// 确认卡：包身份 + 挡位按钮（高于清单申请档位的按钮禁用）。
    pub(crate) fn install_wizard(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(candidate) = &self.pending_install else {
            return v_flex().into_any_element();
        };
        let manifest = &candidate.manifest;
        let mut buttons = Vec::new();
        for (tier, slug, tier_key) in TIERS {
            let locked = tier > candidate.max_tier();
            buttons.push(
                Button::new(format!("plugin-install-tier-{slug}"))
                    .primary()
                    .xsmall()
                    .label(t!("plugins.install_as", tier = t!(tier_key).to_string()).to_string())
                    .disabled(locked)
                    .when(locked, |button| {
                        button.tooltip(
                            t!(
                                "plugins.tier_locked",
                                tier = t!(tier_key_of(candidate.max_tier())).to_string()
                            )
                            .to_string(),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.confirm_install(tier, cx);
                    })),
            );
        }
        v_flex()
            .id("plugin-install-wizard")
            .gap_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().primary)
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .child(t!("plugins.wizard_title").to_string()),
            )
            .child(
                div().text_xs().child(
                    t!(
                        "plugins.wizard_identity",
                        name = manifest.name.clone(),
                        version = manifest.version.clone(),
                        build = candidate.receipt.build,
                        sha = short_sha(&candidate.receipt.sha256),
                    )
                    .to_string(),
                ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("plugins.wizard_hint").to_string()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        t!(
                            "plugins.wizard_dir",
                            path = candidate.dir.display().to_string()
                        )
                        .to_string(),
                    ),
            )
            .child(h_flex().gap_2().children(buttons))
            .child(
                Button::new("plugin-install-cancel")
                    .ghost()
                    .xsmall()
                    .label(t!("plugins.cancel").to_string())
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_install(cx))),
            )
            .into_any_element()
    }
}

/// 挡位对应的文案键（与 TIERS 一致）。
fn tier_key_of(tier: PermissionTier) -> &'static str {
    match tier {
        PermissionTier::ReadOnly => "plugins.tier_read",
        PermissionTier::Workspace => "plugins.tier_workspace",
        PermissionTier::Trusted => "plugins.tier_trusted",
    }
}

fn short_sha(sha: &str) -> String {
    sha.chars().take(8).collect()
}
