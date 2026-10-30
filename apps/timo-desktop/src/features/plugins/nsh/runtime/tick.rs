//! 运行期轮询：健康门、错误回收与看门狗（设计 05 §2~§3）。

use std::time::Instant;

use gpui_kit::Context;
use nsh_platform_gpui::accept_package;

use crate::app::DesktopApp;

use super::super::runtime_watch::{Beat, Watchdog};
use super::{Phase, PluginRuntime, stop_slot};

impl PluginRuntime {
    /// 健康门、错误与看门狗；实例增减或阶段变化时返回 true（界面需重绘）。
    pub(crate) fn tick(&mut self, now: Instant, cx: &mut Context<DesktopApp>) -> bool {
        let mut changed = false;
        self.flush_sessions(now);

        let failures: Vec<(usize, String)> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let message = slot.error.lock().ok().and_then(|mut error| error.take())?;
                Some((index, message))
            })
            .collect();
        for (index, message) in failures.into_iter().rev() {
            let mut slot = self.slots.remove(index);
            let plugin = slot.plugin.clone();
            let version = slot.version.clone();
            let pending = matches!(slot.phase, Phase::Pending { .. });
            stop_slot(&mut slot, cx);
            self.note(
                &plugin,
                "E742",
                if pending {
                    format!("startup failed: {message}")
                } else {
                    message
                },
            );
            self.failed.insert(plugin.clone(), version);
            if !pending {
                self.record_crash(&plugin, now);
            }
            changed = true;
        }

        let due: Vec<(usize, String, String, String)> = self
            .slots
            .iter()
            .enumerate()
            .filter(
                |(_, slot)| matches!(slot.phase, Phase::Pending { ready_at } if ready_at <= now),
            )
            .map(|(index, slot)| {
                (
                    index,
                    slot.key.clone(),
                    slot.version.clone(),
                    slot.plugin.clone(),
                )
            })
            .collect();
        let mut accepted: Vec<(String, String)> = vec![];
        let mut rejected: Vec<(String, String, String, String)> = vec![];
        for (index, key, version, plugin) in due {
            let outcome = match self.trust.as_mut() {
                Some(trust) => accept_package(trust, &self.slots[index].loaded),
                None => Err(self.trust_error.clone().unwrap_or_default()),
            };
            match outcome {
                Ok(()) => accepted.push((key, version)),
                Err(error) => rejected.push((plugin, key, version, error)),
            }
        }
        for (key, version) in &accepted {
            if let Some(slot) = self
                .slots
                .iter_mut()
                .find(|slot| &slot.key == key && &slot.version == version)
            {
                slot.phase = Phase::Running;
                slot.watchdog = Watchdog::new(now);
                changed = true;
            }
        }
        for (plugin, key, version, error) in rejected {
            if let Some(index) = self
                .slots
                .iter()
                .position(|slot| slot.key == key && slot.version == version)
            {
                let mut slot = self.slots.remove(index);
                stop_slot(&mut slot, cx);
            }
            self.failed.insert(plugin.clone(), version);
            self.note(&plugin, "E711", format!("accept failed: {error}"));
            changed = true;
        }
        // 新版本健康后与旧实例交换：同一 key 其它版本的 Running 实例下线。
        let stale: Vec<usize> = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| {
                matches!(slot.phase, Phase::Running)
                    && accepted
                        .iter()
                        .any(|(key, version)| key == &slot.key && version != &slot.version)
            })
            .map(|(index, _)| index)
            .collect();
        for index in stale.into_iter().rev() {
            let mut old = self.slots.remove(index);
            stop_slot(&mut old, cx);
            changed = true;
        }

        let mut stops: Vec<(usize, String)> = vec![];
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if !matches!(slot.phase, Phase::Running) {
                continue;
            }
            match slot.watchdog.tick(now, slot.host.pong_count()) {
                Beat::Ping(seq) => slot.host.ping(&slot.channel, seq),
                Beat::Stop => {
                    stops.push((index, "unresponsive: no __pong from the plugin".to_string()));
                }
                Beat::Idle | Beat::Waiting => {}
            }
        }
        changed |= !stops.is_empty();
        for (index, message) in stops.into_iter().rev() {
            let mut slot = self.slots.remove(index);
            let plugin = slot.plugin.clone();
            let version = slot.version.clone();
            stop_slot(&mut slot, cx);
            self.note(&plugin, "E742", message);
            self.failed.insert(plugin.clone(), version);
            self.record_crash(&plugin, now);
        }
        changed
    }
}
