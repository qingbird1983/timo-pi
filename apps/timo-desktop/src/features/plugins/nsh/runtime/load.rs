//! 实例装载与排列：把期望集变成 `AppView` 实例（设计 05 §1~§3）。
//!
//! 清单变化只置脏；真正装载在渲染帧之后执行，避免在 `render` 栈里创建实体。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use gpui_kit::{AppContext as _, Context, Window};
use nsh_engine::HostChannel;
use nsh_platform_gpui::package::TrustStore;
use nsh_platform_gpui::{AppOptions, AppView, Appearance, verify_package};
use timo_kernel::plugins::GuiManifest;
use timo_kernel::plugins::nsh::{assets_root, dev_dir, plugin_dir, trust_dir};
use timo_kernel::{PermissionStore, permissions_path};
use tokio_util::sync::CancellationToken;

use crate::app::DesktopApp;

use super::super::host_api::{HostApi, HostConfig};
use super::super::runtime_theme;
use super::super::runtime_watch::Watchdog;
use super::super::{HEALTH_DELAY, open_trust};
use super::desired::DesiredSlot;
use super::{Phase, PluginRuntime, SlotInstance, stop_slot};

impl PluginRuntime {
    pub(super) fn reconcile_slots(
        &mut self,
        desired: &[DesiredSlot],
        cx: &mut Context<DesktopApp>,
    ) {
        let wanted: BTreeMap<String, &DesiredSlot> =
            desired.iter().map(|slot| (slot.key(), slot)).collect();
        let live: BTreeSet<&str> = desired.iter().map(|slot| slot.plugin.as_str()).collect();
        let mut keep = Vec::new();
        for mut slot in std::mem::take(&mut self.slots) {
            let Some(target) = wanted.get(slot.key.as_str()) else {
                stop_slot(&mut slot, cx);
                continue;
            };
            let superseded =
                target.version != slot.version && matches!(slot.phase, Phase::Pending { .. });
            if superseded {
                stop_slot(&mut slot, cx);
            } else {
                keep.push(slot);
            }
        }
        self.slots = keep;
        self.patches
            .retain(|plugin, _| live.contains(plugin.as_str()));
    }

    pub(super) fn ensure_slot(
        &mut self,
        wanted: &DesiredSlot,
        now: Instant,
        window: &mut Window,
        cx: &mut Context<DesktopApp>,
    ) {
        let key = wanted.key();
        if self
            .slots
            .iter()
            .any(|slot| slot.key == key && slot.version == wanted.version)
        {
            return;
        }
        if self
            .failed
            .get(&wanted.plugin)
            .is_some_and(|version| version == &wanted.version)
        {
            return;
        }
        match self.load_slot(wanted, now, window, cx) {
            Ok(slot) => {
                self.failed.remove(&wanted.plugin);
                self.slots.push(slot);
            }
            Err(message) => {
                self.failed
                    .insert(wanted.plugin.clone(), wanted.version.clone());
                self.note(&wanted.plugin, "E742", message);
            }
        }
    }

    fn load_slot(
        &mut self,
        wanted: &DesiredSlot,
        now: Instant,
        window: &mut Window,
        cx: &mut Context<DesktopApp>,
    ) -> Result<SlotInstance, String> {
        let (entry_version, entry_path, tier, source_hash, queries, actions, tools) = {
            let entry = self
                .entries
                .iter()
                .find(|entry| entry.id == wanted.plugin)
                .ok_or_else(|| "the plugin is no longer listed".to_string())?;
            (
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
        if manifest.id != wanted.plugin || manifest.version != wanted.version {
            return Err(format!(
                "manifest is {}/{} but the ledger says {}/{}",
                manifest.id, manifest.version, wanted.plugin, wanted.version
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
        let read_scopes = self.read_scopes(&wanted.plugin, &source_hash);
        let channel = HostChannel::new();
        let cancel = CancellationToken::new();
        let patch = self.patches.get(&wanted.plugin).cloned();
        let snapshot = runtime_theme::snapshot(self.dark, &self.theme_tokens, patch.as_ref());
        let css = runtime_theme::css_vars(&self.theme_tokens, patch.as_ref());
        let host = HostApi::start(HostConfig {
            plugin_id: wanted.plugin.clone(),
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
        let instance_id = super::instance_id(wanted.kind, wanted.binding.as_deref());
        let data = self.slot_data(wanted.kind, wanted.binding.as_deref());
        host.set_composer_text(&self.composer_text);
        channel.set_params(host.params(
            wanted.kind.as_str(),
            &instance_id,
            data.clone(),
            &crate::i18n::current(),
        ));
        let error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let sink = error.clone();
        let app_dir = plugin_dir(&data_dir, &wanted.plugin).join("nsh");
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
        Ok(SlotInstance {
            key: wanted.key(),
            plugin: wanted.plugin.clone(),
            version: entry_version,
            kind: wanted.kind,
            binding: wanted.binding.clone(),
            title: wanted.title.clone(),
            order: wanted.order,
            instance_id,
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
            sent: data,
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
