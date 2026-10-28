//! 项目信任确认 sheet（设计 12 §3.4/§3.5；五项选择与 CLI 菜单同义）。
//!
//! 内核在装配前经 [`timo_kernel::trust::ProjectTrustPrompt`] 发问，桥接转成
//! [`crate::bridge::BridgeEvent::Trust`]；本 sheet 把选择经 oneshot 回传。
//! 关闭/遮罩/Esc = “不信任（本次）”，窗口整体关闭时桥接 drop 通道，内核同样按
//! 不信任处理。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::sheet::Sheet;
use gpui_kit::component::{ActiveTheme as _, StyledExt as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;
use timo_kernel::trust::TrustRequest;
use timo_kernel::{TrustChoice, TrustScopeChoice};
use tokio::sync::oneshot;

use crate::app::DesktopApp;

/// 一条等待回答的信任询问。
pub struct PendingTrust {
    /// 工作目录、触发的资源与已保存的决定。
    pub request: TrustRequest,
    /// 回传通道；`None` = 用户取消。
    pub respond: oneshot::Sender<Option<TrustChoice>>,
}

/// 信任 sheet 的状态。
pub struct TrustState {
    /// 当前待答请求（同一时刻只有一条：发生在 `Kernel::new` 内）。
    pub pending: Option<PendingTrust>,
    /// sheet 是否已打开。
    pub open: bool,
    /// 开合时承接快捷键的焦点。
    pub focus: FocusHandle,
}

impl TrustState {
    /// 创建空状态。
    pub fn new(cx: &mut App) -> Self {
        Self {
            pending: None,
            open: false,
            focus: cx.focus_handle(),
        }
    }
}

impl DesktopApp {
    /// 信任询问到达：记下请求，由 render 打开 sheet。
    pub(crate) fn handle_trust_request(
        &mut self,
        request: TrustRequest,
        respond: oneshot::Sender<Option<TrustChoice>>,
        cx: &mut Context<Self>,
    ) {
        self.trust.pending = Some(PendingTrust { request, respond });
        cx.notify();
    }

    /// 打开信任 sheet（若尚未打开）。
    pub(crate) fn open_trust_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.trust.pending.is_none() || self.trust.open {
            return;
        }
        self.trust.open = true;
        let weak = cx.entity().downgrade();
        window.open_sheet(cx, move |sheet, window, cx| match weak.upgrade() {
            Some(app) => app.update(cx, |this, cx| this.trust_sheet(sheet, window, cx)),
            None => sheet,
        });
        let focus = self.trust.focus.clone();
        cx.defer_in(window, move |_, window, cx| {
            window.focus(&focus, cx);
        });
    }

    /// 信任（保存到当前目录）。
    pub(crate) fn trust_accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_trust(
            Some(TrustChoice {
                trusted: true,
                scope: TrustScopeChoice::Directory,
            }),
            window,
            cx,
        );
    }

    /// 信任父目录（保存到上级并清除本目录条目）。
    pub(crate) fn trust_accept_parent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_trust(
            Some(TrustChoice {
                trusted: true,
                scope: TrustScopeChoice::Parent,
            }),
            window,
            cx,
        );
    }

    /// 仅本次信任（不落盘）。
    pub(crate) fn trust_accept_once(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_trust(
            Some(TrustChoice {
                trusted: true,
                scope: TrustScopeChoice::Session,
            }),
            window,
            cx,
        );
    }

    /// 不信任（保存到当前目录）。
    pub(crate) fn trust_deny(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_trust(
            Some(TrustChoice {
                trusted: false,
                scope: TrustScopeChoice::Directory,
            }),
            window,
            cx,
        );
    }

    /// 仅本次不信任；关闭 sheet / Esc 走同一条。
    pub(crate) fn trust_deny_once(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_trust(
            Some(TrustChoice {
                trusted: false,
                scope: TrustScopeChoice::Session,
            }),
            window,
            cx,
        );
    }

    fn finish_trust(
        &mut self,
        choice: Option<TrustChoice>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.trust.pending.take() else {
            self.trust.open = false;
            return;
        };
        let choice = choice.or(Some(TrustChoice {
            trusted: false,
            scope: TrustScopeChoice::Session,
        }));
        let _ = pending.respond.send(choice);
        self.trust.open = false;
        window.close_sheet(cx);
        cx.notify();
    }

    /// 构建信任 sheet：资源清单 + 五个选择。
    fn trust_sheet(&mut self, sheet: Sheet, _window: &mut Window, cx: &mut Context<Self>) -> Sheet {
        let Some(pending) = self.trust.pending.as_ref() else {
            return sheet;
        };
        let cwd = pending.request.cwd.display().to_string();
        let resources: Vec<String> = pending
            .request
            .resources
            .iter()
            .map(|resource| format!("{}: {}", resource.kind.label(), resource.path.display()))
            .collect();
        let parent = pending
            .request
            .cwd
            .parent()
            .map(|path| path.display().to_string());
        let saved = pending.request.saved.as_ref().map(|entry| {
            (
                if entry.decision {
                    t!("trust.saved_trusted").to_string()
                } else {
                    t!("trust.saved_untrusted").to_string()
                },
                entry.path.display().to_string(),
            )
        });

        let weak_accept = cx.entity().downgrade();
        let weak_parent = cx.entity().downgrade();
        let weak_once = cx.entity().downgrade();
        let weak_deny = cx.entity().downgrade();
        let weak_deny_once = cx.entity().downgrade();
        let weak_close = cx.entity().downgrade();

        let mut body = v_flex()
            .gap_3()
            .w_full()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("trust.intro").to_string()),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("trust.cwd").to_string()),
                    )
                    .child(div().text_xs().font_semibold().child(cwd)),
            );
        if let Some((decision, path)) = saved {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{decision}: {path}")),
            );
        }
        let mut list = v_flex().gap_1();
        for resource in resources {
            list = list.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(resource),
            );
        }
        body = body.child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("trust.resources").to_string()),
                )
                .child(list),
        );

        let mut buttons = v_flex().gap_2();
        buttons = buttons.child(
            Button::new("trust-accept")
                .primary()
                .label(t!("trust.accept").to_string())
                .on_click(move |_, window, cx| {
                    if let Some(app) = weak_accept.upgrade() {
                        app.update(cx, |this, cx| this.trust_accept(window, cx));
                    }
                }),
        );
        if let Some(parent) = parent {
            buttons = buttons.child(
                Button::new("trust-accept-parent")
                    .outline()
                    .label(t!("trust.accept_parent", path = parent).to_string())
                    .on_click(move |_, window, cx| {
                        if let Some(app) = weak_parent.upgrade() {
                            app.update(cx, |this, cx| this.trust_accept_parent(window, cx));
                        }
                    }),
            );
        }
        buttons = buttons
            .child(
                Button::new("trust-accept-once")
                    .outline()
                    .label(t!("trust.accept_once").to_string())
                    .on_click(move |_, window, cx| {
                        if let Some(app) = weak_once.upgrade() {
                            app.update(cx, |this, cx| this.trust_accept_once(window, cx));
                        }
                    }),
            )
            .child(
                Button::new("trust-deny")
                    .outline()
                    .label(t!("trust.deny").to_string())
                    .on_click(move |_, window, cx| {
                        if let Some(app) = weak_deny.upgrade() {
                            app.update(cx, |this, cx| this.trust_deny(window, cx));
                        }
                    }),
            )
            .child(
                Button::new("trust-deny-once")
                    .outline()
                    .label(t!("trust.deny_once").to_string())
                    .on_click(move |_, window, cx| {
                        if let Some(app) = weak_deny_once.upgrade() {
                            app.update(cx, |this, cx| this.trust_deny_once(window, cx));
                        }
                    }),
            );

        let heading = h_flex().gap_2().items_center().child(
            div()
                .text_sm()
                .font_semibold()
                .child(t!("trust.title").to_string()),
        );

        let body = v_flex()
            .gap_4()
            .w_full()
            .track_focus(&self.trust.focus)
            .key_context("TrustSheet")
            .child(body)
            .child(buttons);

        sheet
            .size(px(460.))
            .title(heading)
            .on_close(move |_, window, cx| {
                if let Some(app) = weak_close.upgrade() {
                    app.update(cx, |this, cx| this.trust_deny_once(window, cx));
                }
            })
            .child(body)
    }
}
