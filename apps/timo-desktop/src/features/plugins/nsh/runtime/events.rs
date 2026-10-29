//! 会话广播、主题补丁与崩溃计数（设计 05 §3、§5）。

use std::time::Instant;

use gpui_kit::Context;
use serde_json::json;
use timo_kernel::plugins::host::NotifyLevel;

use crate::app::DesktopApp;
use crate::bridge::BridgeCommand;

use super::super::host_api::HostUiEvent;
use super::super::runtime_theme;
use super::{CRASH_WINDOW, PluginRuntime, RuntimeEffect};

impl PluginRuntime {
    /// 取出宿主线程产生的 UI 副作用，并把主题补丁应用到实例。
    pub(crate) fn drain_events(&mut self, cx: &mut Context<DesktopApp>) -> Vec<RuntimeEffect> {
        let mut effects = std::mem::take(&mut self.pending);
        while let Ok(event) = self.ui_rx.try_recv() {
            match event {
                HostUiEvent::Notify {
                    plugin,
                    level,
                    message,
                } => {
                    let level = match level {
                        NotifyLevel::Info => "info",
                        NotifyLevel::Warn => "warn",
                        NotifyLevel::Error => "error",
                    };
                    effects.push(RuntimeEffect::Notice(
                        t!(
                            "plugins.notify",
                            plugin = plugin,
                            level = level,
                            message = message
                        )
                        .to_string(),
                    ));
                }
                HostUiEvent::OpenSession(id) => effects.push(RuntimeEffect::OpenSession(id)),
                HostUiEvent::OpenPath(path) => effects.push(RuntimeEffect::OpenPath(path)),
                HostUiEvent::SendPrompt { text, submit } => {
                    effects.push(RuntimeEffect::SendPrompt { text, submit });
                }
                HostUiEvent::OpenPanel { plugin } => {
                    effects.push(RuntimeEffect::OpenPanel { plugin });
                }
                HostUiEvent::ClipboardWrite(text) => {
                    effects.push(RuntimeEffect::Clipboard(text));
                }
                HostUiEvent::Theme { plugin, tokens } => {
                    self.patches.insert(plugin.clone(), tokens);
                    self.apply_patch(&plugin, cx);
                }
                HostUiEvent::Diagnostic {
                    plugin,
                    code,
                    message,
                } => self.note(&plugin, &code, message),
            }
        }
        effects
    }

    pub(super) fn apply_patch(&mut self, plugin: &str, cx: &mut Context<DesktopApp>) {
        let patch = self.patches.get(plugin).cloned();
        for slot in self.slots.iter_mut().filter(|slot| slot.plugin == plugin) {
            let snapshot = runtime_theme::snapshot(self.dark, &self.theme_tokens, patch.as_ref());
            let css = runtime_theme::css_vars(&self.theme_tokens, patch.as_ref());
            slot.host.set_theme(snapshot.clone());
            let _ = slot.channel.send("theme.changed", snapshot);
            slot.view.update(cx, |view, cx| {
                let _ = view.set_theme_json(&css, cx);
            });
        }
    }

    pub(super) fn flush_sessions(&mut self, now: Instant) {
        let Some((due, _)) = &self.sessions else {
            return;
        };
        if now < *due {
            return;
        }
        let Some((_, ids)) = self.sessions.take() else {
            return;
        };
        let ids: Vec<String> = ids.into_iter().collect();
        for slot in &self.slots {
            let _ = slot.channel.send("sessions.changed", json!({ "ids": ids }));
        }
    }

    pub(super) fn record_crash(&mut self, plugin: &str, now: Instant) {
        let crashes = self.crashes.entry(plugin.to_string()).or_default();
        while crashes
            .front()
            .is_some_and(|at| now.duration_since(*at) > CRASH_WINDOW)
        {
            crashes.pop_front();
        }
        crashes.push_back(now);
        if crashes.len() >= 2 && self.auto_disabled.insert(plugin.to_string()) {
            let _ = self.commands.send(BridgeCommand::SetPluginEnabled {
                id: plugin.to_string(),
                enabled: false,
            });
            self.pending.push(RuntimeEffect::Notice(
                t!("plugins.auto_disabled", plugin = plugin).to_string(),
            ));
        }
    }
}
