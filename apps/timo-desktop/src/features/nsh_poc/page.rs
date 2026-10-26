//! 插件预览页的界面与动作（表盘与按钮；渲染细节见 `mod.rs`）。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, IntoElement as _, ParentElement as _, Styled as _, Window, div,
};

use super::{HEALTH_DELAY, NshPreview, NshSource, short_sha};
use crate::app::DesktopApp;

impl DesktopApp {
    /// 侧栏入口（仅 `nsh-poc` feature 编译）。
    pub(crate) fn nsh_entry(&self, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .px_2()
            .pb_3()
            .child(
                Button::new("nsh-poc-open")
                    .ghost()
                    .small()
                    .icon(IconName::Bot)
                    .label(t!("nsh.entry").to_string())
                    .on_click(cx.listener(|this, _, window, cx| this.open_nsh_page(window, cx))),
            )
            .into_any_element()
    }

    /// 预览页是否占用中央区域（Esc 关闭用）。
    pub(crate) fn nsh_page_open(&self) -> bool {
        self.nsh_page
    }

    /// 中央区域：预览页优先，否则回到设置/对话。
    pub(crate) fn center_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.nsh_page {
            self.nsh_page(window, cx)
        } else {
            self.main_panel(window, cx)
        }
    }

    /// 打开 NSH 预览页（仅 `nsh-poc` feature）。
    pub(crate) fn open_nsh_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.nsh_page = true;
        self.settings_open = false;
        self.palette_open = false;
        if self.nsh_preview.is_none() {
            self.nsh_preview = Some(NshPreview::new(self.dark));
        }
        if let Some(preview) = &mut self.nsh_preview {
            preview.sync_appearance(self.dark, cx);
        }
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// 关闭预览页回到对话。
    pub(crate) fn close_nsh_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.nsh_page = false;
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// 加载 v1/v2 或尝试未签名/篡改包。
    pub(crate) fn nsh_load(
        &mut self,
        source: NshSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(preview) = self.nsh_preview.as_mut() else {
            return;
        };
        let path = preview.package_path(source);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                let text = t!(
                    "nsh.missing",
                    path = path.display().to_string(),
                    error = error
                )
                .to_string();
                preview.set_status(false, text);
                cx.notify();
                return;
            }
        };
        let mut schedule = false;
        if source.signed() {
            match preview.load_signed(&bytes, window, cx) {
                Ok(_) => schedule = true,
                Err(error) => preview.set_status(false, error),
            }
        } else {
            preview.expect_reject(&bytes, source);
        }
        if schedule {
            let weak = cx.entity().downgrade();
            cx.spawn_in(window, async move |_, cx| {
                cx.background_executor().timer(HEALTH_DELAY).await;
                let _ = weak.update(cx, |this, cx| {
                    if let Some(preview) = &mut this.nsh_preview {
                        preview.finish_pending();
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// 停止预览（保留信任根）。
    pub(crate) fn nsh_stop(&mut self, cx: &mut Context<Self>) {
        if let Some(preview) = &mut self.nsh_preview {
            preview.stop();
        }
        cx.notify();
    }

    /// 清空已接受的构建记录。
    pub(crate) fn nsh_reset_trust(&mut self, cx: &mut Context<Self>) {
        if let Some(preview) = &mut self.nsh_preview {
            preview.reset_trust();
        }
        cx.notify();
    }

    /// 预览页整体。
    pub(crate) fn nsh_page(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let Some(preview) = &mut self.nsh_preview {
            preview.sync_appearance(self.dark, cx);
            preview.poll_errors();
        }
        let (meta, sha, panel) = if let Some(preview) = &self.nsh_preview {
            let meta = preview
                .current()
                .map(|package| {
                    t!(
                        "nsh.current",
                        id = package.id.clone(),
                        version = package.version.clone(),
                        build = package.build
                    )
                    .to_string()
                })
                .unwrap_or_else(|| t!("nsh.no_package").to_string());
            let sha = preview
                .current()
                .map(|package| format!("sha256 {}", short_sha(&package.sha256)))
                .unwrap_or_default();
            (meta, sha, preview.render(cx))
        } else {
            let empty = v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(t!("nsh.empty").to_string())
                .into_any_element();
            (t!("nsh.no_package").to_string(), String::new(), empty)
        };
        let status = self
            .nsh_preview
            .as_ref()
            .and_then(|preview| preview.status().cloned());
        let mut actions = h_flex().gap_2().items_center();
        for (id, source, primary) in [
            ("nsh-load-v1", NshSource::V1, false),
            ("nsh-load-v2", NshSource::V2, true),
            ("nsh-load-crash", NshSource::Crash, false),
            ("nsh-load-unsigned", NshSource::Unsigned, false),
            ("nsh-load-tampered", NshSource::Tampered, false),
        ] {
            let label = match source {
                NshSource::V1 => t!("nsh.load_v1").to_string(),
                NshSource::V2 => t!("nsh.load_v2").to_string(),
                NshSource::Crash => t!("nsh.load_crash").to_string(),
                NshSource::Unsigned => t!("nsh.load_unsigned").to_string(),
                NshSource::Tampered => t!("nsh.load_tampered").to_string(),
            };
            let mut button = Button::new(id).small();
            button = if primary {
                button.primary()
            } else {
                button.outline()
            };
            actions = actions.child(button.label(label).on_click(
                cx.listener(move |this, _, window, cx| this.nsh_load(source, window, cx)),
            ));
        }
        actions = actions
            .child(
                Button::new("nsh-reset-trust")
                    .small()
                    .ghost()
                    .label(t!("nsh.reset_trust").to_string())
                    .on_click(cx.listener(|this, _, _, cx| this.nsh_reset_trust(cx))),
            )
            .child(
                Button::new("nsh-stop")
                    .small()
                    .ghost()
                    .label(t!("nsh.stop").to_string())
                    .on_click(cx.listener(|this, _, _, cx| this.nsh_stop(cx))),
            );
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
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
                            .child(t!("nsh.title").to_string()),
                    )
                    .child(
                        Button::new("nsh-close")
                            .ghost()
                            .label(t!("nsh.back").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_nsh_page(window, cx);
                            })),
                    ),
            )
            .child(actions)
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_sm().font_semibold().child(meta))
                    .when(!sha.is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(sha),
                        )
                    }),
            )
            .child(panel)
            .when_some(status, |this, (ok, text)| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(if ok {
                            cx.theme().success
                        } else {
                            cx.theme().danger
                        })
                        .child(text),
                )
            })
            .into_any_element()
    }
}
