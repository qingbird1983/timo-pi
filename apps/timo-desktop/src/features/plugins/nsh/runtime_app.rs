//! 运行期与主窗口的接缝：启动 tick、渲染帧协调与宿主副作用落地（设计 05）。
//!
//! `AppView` 的侧栏容器在 `features::plugins::nsh::panel`；这里保证实例装载、
//! 健康门、看门狗与宿主动作（会话/提示词/剪贴板/通知）在主窗口里有确定的去处。

use std::path::Path;
use std::time::{Duration, Instant};

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use timo_kernel::SessionInfo;

use crate::app::DesktopApp;
use crate::bridge::BridgeCommand;

use super::runtime::{PluginRuntime, RuntimeEffect};
use super::runtime_theme;

/// 运行期轮询间隔：保证 1s 的 pong 期限至少有 4 次判定机会。
const TICK: Duration = Duration::from_millis(250);

impl DesktopApp {
    /// 建立运行期并向内核要一次插件清单；失败只提示，不影响主应用。
    pub(crate) fn start_plugin_runtime(&mut self, cx: &mut Context<Self>) {
        if self.plugin_runtime.is_some() {
            return;
        }
        let tokens = runtime_theme::tokens(cx.theme());
        let runtime = PluginRuntime::new(
            self.commands.clone(),
            self.plugin_requests.clone(),
            self.dark,
            tokens,
        );
        match runtime {
            Ok(runtime) => {
                self.plugin_runtime = Some(runtime);
                let _ = self.commands.send(BridgeCommand::ListPlugins);
                self._runtime_ticks = cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(TICK).await;
                        if this.update(cx, |this, cx| this.runtime_tick(cx)).is_err() {
                            break;
                        }
                    }
                });
                self._subscriptions
                    .push(cx.on_release(|this, cx| this.shutdown_plugin_runtime(cx)));
            }
            Err(message) => {
                self.notice = Some(t!("plugins.runtime_failed", message = message).to_string());
            }
        }
    }

    /// 会话列表变化时给插件广播 `sessions.changed`（节流在内核侧）。
    pub(crate) fn sync_sessions(&mut self, sessions: Vec<SessionInfo>) {
        if let Some(runtime) = self.plugin_runtime.as_mut() {
            let rows = sessions
                .iter()
                .map(|session| (session.id.clone(), session_title(session)))
                .collect();
            runtime.sync_sessions(rows);
        }
        self.sessions = sessions;
    }

    /// 渲染帧之前同步会话快照并协调实例（`cx.new` 不能在 `render` 栈里直接跑）。
    pub(crate) fn runtime_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.runtime_prompt.take() {
            self.input
                .update(cx, |input, cx| input.set_value(text, window, cx));
        }
        let wants_chat = self
            .plugin_runtime
            .as_ref()
            .is_some_and(PluginRuntime::wants_chat_sync);
        if wants_chat {
            let session = self.session_id.clone();
            let calls = self.chat.read(cx).latest_tool_calls();
            let composer = self.input.read(cx).value().to_string();
            if let Some(runtime) = self.plugin_runtime.as_mut() {
                runtime.sync_chat(session.as_deref(), calls, &composer);
            }
        }
        let dirty = self
            .plugin_runtime
            .as_ref()
            .is_some_and(PluginRuntime::is_dirty);
        if let Some(runtime) = self.plugin_runtime.as_mut() {
            runtime.reconcile_if_dirty(window, cx);
        }
        if dirty {
            cx.notify();
        }
    }

    /// 一次定时轮询：健康门、看门狗、主题同步与宿主副作用。
    pub(crate) fn runtime_tick(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let (effects, changed) = {
            let Some(runtime) = self.plugin_runtime.as_mut() else {
                return;
            };
            let mut changed = runtime.tick(now, cx);
            runtime.sync_theme(self.dark, cx);
            let effects = runtime.drain_events(cx);
            changed |= runtime.apply_diagnostics(&mut self.plugins);
            changed |= runtime.is_dirty();
            (effects, changed)
        };
        let acted = !effects.is_empty();
        for effect in effects {
            self.apply_runtime_effect(effect, cx);
        }
        if acted || changed {
            cx.notify();
        }
    }

    /// 退出时停掉全部实例（`on_release` 与主窗口释放同一路径）。
    pub(crate) fn shutdown_plugin_runtime(&mut self, cx: &mut App) {
        if let Some(mut runtime) = self.plugin_runtime.take() {
            runtime.shutdown(cx);
        }
    }

    fn apply_runtime_effect(&mut self, effect: RuntimeEffect, cx: &mut Context<Self>) {
        match effect {
            RuntimeEffect::Notice(text) => self.notice = Some(text),
            RuntimeEffect::OpenSession(id) => self.open_session(id, cx),
            RuntimeEffect::OpenPath(path) => cx.open_with_system(Path::new(&path)),
            RuntimeEffect::SendPrompt { text, submit } => {
                if submit {
                    self.send_text(text, cx);
                } else {
                    self.runtime_prompt = Some(text);
                }
            }
            RuntimeEffect::OpenPanel { plugin } => {
                self.open_installed(cx);
                self.plugins_selected = Some(plugin);
            }
            RuntimeEffect::Clipboard(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text))
            }
        }
    }
}

/// 会话显示名（与侧栏一致）：首条消息，空会话用占位文案。
fn session_title(session: &SessionInfo) -> String {
    if session.first_message.trim().is_empty() {
        t!("chat.empty_session").to_string()
    } else {
        session.first_message.clone()
    }
}
