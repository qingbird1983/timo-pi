//! 会话广播、槽位数据推送、主题补丁与崩溃计数（设计 03 §1.1、05 §3、§5）。

use std::collections::BTreeSet;
use std::time::Instant;

use gpui_kit::Context;
use serde_json::{Value, json};
use timo_kernel::plugins::host::NotifyLevel;

use crate::app::DesktopApp;
use crate::bridge::BridgeCommand;
use crate::features::chat::ToolCallSnapshot;

use super::super::host_api::HostUiEvent;
use super::super::runtime_theme;
use super::data;
use super::desired::SlotKind;
use super::{CRASH_WINDOW, PluginRuntime, RuntimeEffect, SESSION_THROTTLE};

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

    /// 侧栏会话 `(id, 标题)`：新增/删除广播 `sessions.changed`，标题变化推 `slot.changed`。
    pub(crate) fn sync_sessions(&mut self, rows: Vec<(String, String)>) {
        let ids: BTreeSet<String> = rows.iter().map(|(id, _)| id.clone()).collect();
        if let Some(previous) = self.known_sessions.replace(ids.clone()) {
            let mut changed: BTreeSet<String> = ids.difference(&previous).cloned().collect();
            changed.extend(previous.difference(&ids).cloned());
            if !changed.is_empty() {
                match &mut self.sessions {
                    Some((_, pending)) => pending.extend(changed),
                    None => self.sessions = Some((Instant::now() + SESSION_THROTTLE, changed)),
                }
            }
        }
        let retitled: Vec<String> = rows
            .iter()
            .filter(|(id, title)| {
                self.session_rows
                    .iter()
                    .any(|(old_id, old_title)| old_id == id && old_title != title)
            })
            .map(|(id, _)| id.clone())
            .collect();
        // 会话集合或顺序变化都要重建 entryExtra 实例。
        if self.session_rows != rows {
            self.dirty = true;
        }
        self.session_rows = rows;
        for id in retitled {
            self.publish(SlotKind::EntryExtra, Some(&id));
        }
    }

    /// 当前会话的工具调用与输入文本快照；变化时给对应实例推 `slot.changed`。
    pub(crate) fn sync_chat(
        &mut self,
        session: Option<&str>,
        calls: Vec<ToolCallSnapshot>,
        composer: &str,
    ) {
        if self.session_id.as_deref() != session || self.calls != calls {
            self.session_id = session.map(str::to_string);
            self.calls = calls;
            for tool in self.tool_bindings() {
                self.publish(SlotKind::ToolCard, Some(&tool));
            }
        }
        if self.composer_text != composer {
            self.composer_text = composer.to_string();
            for slot in &self.slots {
                slot.host.set_composer_text(composer);
            }
            self.publish(SlotKind::ComposerTrigger, None);
        }
    }

    /// 某个实例的槽位数据（启动参数与 `slot.changed` 同形）。
    pub(super) fn slot_data(&self, kind: SlotKind, binding: Option<&str>) -> Value {
        match kind {
            SlotKind::Panel => data::panel_data(),
            SlotKind::ToolCard => {
                let tool = binding.unwrap_or_default();
                let call = self.calls.iter().find(|call| call.name == tool);
                data::tool_card_data(tool, call)
            }
            SlotKind::ComposerTrigger => data::composer_data(&self.composer_text),
            SlotKind::EntryExtra => {
                let id = binding.unwrap_or_default();
                let title = self
                    .session_rows
                    .iter()
                    .find(|(session, _)| session == id)
                    .map(|(_, title)| title.as_str())
                    .unwrap_or_default();
                data::entry_extra_data(id, title)
            }
        }
    }

    fn tool_bindings(&self) -> BTreeSet<String> {
        self.slots
            .iter()
            .filter(|slot| slot.kind == SlotKind::ToolCard)
            .filter_map(|slot| slot.binding.clone())
            .collect()
    }

    /// 推送一次槽位数据；只发给该 kind+binding 的实例，内容未变则跳过。
    fn publish(&mut self, kind: SlotKind, binding: Option<&str>) {
        let data = self.slot_data(kind, binding);
        for slot in self
            .slots
            .iter_mut()
            .filter(|slot| slot.kind == kind && slot.binding.as_deref() == binding)
        {
            if slot.sent == data {
                continue;
            }
            slot.sent = data.clone();
            let _ = slot.channel.send(
                "slot.changed",
                json!({ "instanceId": slot.instance_id, "data": data }),
            );
        }
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
