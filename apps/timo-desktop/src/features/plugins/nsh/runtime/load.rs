//! 实例装载、排列与调度：把可用 panel 变成 `AppView` 实例（设计 05 §1~§3）。
//!
//! 清单变化只置脏；真正装载在渲染帧之后执行，避免在 `render` 栈里创建实体。

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use gpui_kit::{AppContext as _, Context, Window};
use nsh_engine::HostChannel;
use nsh_platform_gpui::package::TrustStore;
use nsh_platform_gpui::{AppOptions, AppView, Appearance, verify_package};
use serde_json::json;
use timo_kernel::plugins::nsh::{assets_root, dev_dir, plugin_dir, trust_dir};
use timo_kernel::plugins::{GuiManifest, PluginEntry, PluginKind, PluginOrigin, PluginState};
use timo_kernel::{PermissionStore, permissions_path};
use tokio_util::sync::CancellationToken;

use crate::app::DesktopApp;

use super::super::host_api::{HostApi, HostConfig};
use super::super::runtime_theme;
use super::super::runtime_watch::Watchdog;
use super::super::{HEALTH_DELAY, open_trust};
use super::{DesiredPanel, PANEL_LIMIT, PanelSlot, Phase, PluginRuntime, stop_slot};

impl PluginRuntime {
    pub(super) fn reconcile_slots(
        &mut self,
        desired: &[DesiredPanel],
        cx: &mut Context<DesktopApp>,
    ) {
        let mut keep = Vec::new();
        for mut slot in std::mem::take(&mut self.slots) {
            let wanted = desired.iter().find(|panel| panel.id == slot.plugin);
            let superseded = wanted.is_some_and(|panel| panel.version != slot.version)
                && matches!(slot.phase, Phase::Pending { .. });
            if wanted.is_none() || superseded {
                stop_slot(&mut slot, cx);
                if wanted.is_none() {
                    self.patches.remove(&slot.plugin);
                }
            } else {
                keep.push(slot);
            }
        }
        self.slots = keep;
    }

    pub(super) fn ensure_slot(
        &mut self,
        wanted: &DesiredPanel,
        now: Instant,
        window: &mut Window,
        cx: &mut Context<DesktopApp>,
    ) {
        let matching =
            |slot: &PanelSlot| slot.plugin == wanted.id && slot.version == wanted.version;
        if self.slots.iter().any(&matching) {
            return;
        }
        if self
            .failed
            .get(&wanted.id)
            .is_some_and(|version| version == &wanted.version)
        {
            return;
        }
        match self.load_slot(wanted, now, window, cx) {
            Ok(slot) => {
                self.failed.remove(&wanted.id);
                self.slots.push(slot);
            }
            Err(message) => {
                self.failed
                    .insert(wanted.id.clone(), wanted.version.clone());
                self.note(&wanted.id, "E742", message);
            }
        }
    }

    fn load_slot(
        &mut self,
        wanted: &DesiredPanel,
        now: Instant,
        window: &mut Window,
        cx: &mut Context<DesktopApp>,
    ) -> Result<PanelSlot, String> {
        let (entry_id, entry_version, entry_path, tier, source_hash, queries, actions, tools) = {
            let entry = self
                .entries
                .iter()
                .find(|entry| entry.id == wanted.id)
                .ok_or_else(|| "the plugin is no longer listed".to_string())?;
            (
                entry.id.clone(),
                entry.version.clone(),
                entry.path.clone(),
                entry.tier,
                entry.source_hash.clone(),
                entry.declared.queries.clone(),
                entry.declared.actions.clone(),
                entry.declared.tools.clone(),
            )
        };
        let manifest_path = entry_path.join("manifest.json");
        let manifest = GuiManifest::load(&manifest_path)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "manifest.json is missing".to_string())?;
        if manifest.id != entry_id || manifest.version != wanted.version {
            return Err(format!(
                "manifest is {}/{} but the ledger says {}/{}",
                manifest.id, manifest.version, entry_id, wanted.version
            ));
        }
        let nsh_id = manifest
            .nsh_id
            .clone()
            .ok_or_else(|| "nsh_id is missing".to_string())?;
        let bundle_path = manifest.bundle_path(&entry_path);
        let bytes = std::fs::read(&bundle_path)
            .map_err(|error| format!("cannot read {}: {error}", bundle_path.display()))?;
        let data_dir = self.data_dir.clone();
        let assets = assets_root(&data_dir);
        let loaded = verify_package(self.trust_store()?, &bytes, &assets)
            .map_err(|error| format!("E711: {error}"))?;
        if loaded.app.id != nsh_id || loaded.app.version != manifest.version {
            return Err(format!(
                "E713: package is {}/{}",
                loaded.app.id, loaded.app.version
            ));
        }
        let read_scopes = self.read_scopes(&entry_id, &source_hash);
        let channel = HostChannel::new();
        let cancel = CancellationToken::new();
        let patch = self.patches.get(&entry_id).cloned();
        let snapshot = runtime_theme::snapshot(self.dark, &self.theme_tokens, patch.as_ref());
        let css = runtime_theme::css_vars(&self.theme_tokens, patch.as_ref());
        let host = HostApi::start(HostConfig {
            plugin_id: entry_id.clone(),
            version: entry_version.clone(),
            tier,
            queries,
            actions,
            tools,
            slots: manifest
                .slots
                .iter()
                .map(|slot| slot.kind.clone())
                .collect(),
            storage_root: data_dir.clone(),
            sessions_dir: self.sessions_dir.clone(),
            work_dir: self.work_dir.clone(),
            read_scopes,
            theme: snapshot,
            ui: self.ui_tx.clone(),
            kernel: self.kernel.clone(),
            cancel: cancel.clone(),
        })?;
        host.register(&channel);
        channel.set_params(host.params(
            "panel",
            "panel",
            json!({ "panelId": "panel", "width": 248 }),
            &crate::i18n::current(),
        ));
        let error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let sink = error.clone();
        let app_dir = plugin_dir(&data_dir, &entry_id).join("nsh");
        let options = AppOptions {
            app_id: nsh_id,
            title: wanted.title.clone(),
            data_dir: Some(app_dir),
            appearance: appearance(self.dark),
            host: channel.clone(),
            ..Default::default()
        };
        let view = cx.new(|cx| AppView::new(options, window, cx));
        view.read(cx).on_app_error(move |error| {
            if let Ok(mut slot) = sink.lock() {
                *slot = Some(error.message.clone());
            }
        });
        view.read(cx).run_package(&loaded);
        view.update(cx, |view, cx| {
            view.set_appearance(appearance(self.dark), cx);
            let _ = view.set_theme_json(&css, cx);
        });
        Ok(PanelSlot {
            plugin: entry_id,
            version: entry_version,
            view,
            channel,
            host,
            error,
            loaded,
            phase: Phase::Pending {
                ready_at: now + HEALTH_DELAY,
            },
            watchdog: Watchdog::new(now),
            cancel,
        })
    }

    fn trust_store(&mut self) -> Result<&TrustStore, String> {
        if self.trust.is_none() && self.trust_error.is_none() {
            let mut dirs = vec![trust_dir(&self.data_dir)];
            if cfg!(debug_assertions) {
                dirs.push(dev_dir(&self.data_dir));
            }
            match open_trust(&dirs) {
                Ok(trust) => self.trust = Some(trust),
                Err(error) => self.trust_error = Some(error),
            }
        }
        self.trust.as_ref().ok_or_else(|| {
            self.trust_error
                .clone()
                .unwrap_or_else(|| t!("nsh.no_root").to_string())
        })
    }

    fn read_scopes(&self, id: &str, source_hash: &str) -> Vec<String> {
        PermissionStore::load(&permissions_path(&self.data_dir))
            .map(|store| store.effective(id, source_hash).0.fs.read)
            .unwrap_or_default()
    }
}

fn appearance(dark: bool) -> Appearance {
    if dark {
        Appearance::Dark
    } else {
        Appearance::Light
    }
}

/// 可显示的 panel：B 轨、已启用、无错误、声明了 `panel`；按 order/标题/id 排序取前 5。
pub(super) fn desired_panels(entries: &[PluginEntry]) -> (Vec<DesiredPanel>, BTreeSet<String>) {
    let mut panels = vec![];
    for entry in entries {
        if entry.kind != PluginKind::Gui
            || entry.origin != PluginOrigin::Installed
            || !entry.enabled
            || entry.state == PluginState::Error
            || entry.version.is_empty()
        {
            continue;
        }
        let Some(slot) = entry
            .declared
            .slots
            .iter()
            .find(|slot| slot.kind == "panel")
        else {
            continue;
        };
        panels.push(DesiredPanel {
            id: entry.id.clone(),
            version: entry.version.clone(),
            title: slot.title.clone().unwrap_or_else(|| entry.name.clone()),
            order: slot.order.unwrap_or(0),
        });
    }
    panels.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });
    let overflow = panels
        .iter()
        .skip(PANEL_LIMIT)
        .map(|panel| panel.id.clone())
        .collect();
    panels.truncate(PANEL_LIMIT);
    (panels, overflow)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use timo_kernel::plugins::{Declared, PermissionTier, PluginSource};

    fn entry(id: &str, tier: PermissionTier, order: i32, enabled: bool) -> PluginEntry {
        let declared = Declared {
            slots: vec![timo_kernel::plugins::SlotDecl {
                kind: "panel".into(),
                title: Some(id.to_uppercase()),
                order: Some(order),
                icon: None,
            }],
            ..Declared::default()
        };
        PluginEntry {
            id: id.into(),
            kind: PluginKind::Gui,
            origin: PluginOrigin::Installed,
            name: id.into(),
            version: "1.0.0".into(),
            enabled,
            tier,
            declared,
            state: if enabled {
                PluginState::Enabled
            } else {
                PluginState::Disabled
            },
            source: PluginSource::LocalFile,
            source_hash: format!("sha256:{}", "0".repeat(64)),
            configurable: false,
            diagnostics: vec![],
            path: PathBuf::new(),
        }
    }

    #[test]
    fn panels_are_ordered_and_capped() {
        let entries: Vec<PluginEntry> = (0..7)
            .map(|index| {
                entry(
                    &format!("plugin-{index}"),
                    PermissionTier::ReadOnly,
                    index,
                    true,
                )
            })
            .collect();
        let (desired, overflow) = desired_panels(&entries);
        assert_eq!(desired.len(), PANEL_LIMIT);
        assert_eq!(desired[0].id, "plugin-0");
        assert_eq!(overflow.len(), 2);
        assert!(overflow.contains("plugin-5") && overflow.contains("plugin-6"));
    }

    #[test]
    fn disabled_and_broken_plugins_are_skipped() {
        let mut broken = entry("broken", PermissionTier::ReadOnly, 0, true);
        broken.state = PluginState::Error;
        let entries = vec![
            entry("disabled", PermissionTier::ReadOnly, 1, false),
            broken,
            entry("ok", PermissionTier::ReadOnly, 2, true),
        ];
        let (desired, overflow) = desired_panels(&entries);
        assert!(overflow.is_empty());
        assert_eq!(desired.len(), 1);
        assert_eq!(desired[0].id, "ok");
    }
}
