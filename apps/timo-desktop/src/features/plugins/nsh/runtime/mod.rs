//! B 轨 slot 运行时：装载、健康门、看门狗与主题同步（设计 05 §1~§4）。
//!
//! 每个 `panel` 实例一个 `AppView`。装载先验签、再进 1s 健康门，无错才
//! `accept_package`；运行期错误与看门狗失败都移除实例并记诊断。
//! 容器绘制留给 P1-d，本模块只管理实例与宿主副作用。
//!
//! 装载/排列在 [`load`]，会话广播与崩溃计数在 [`events`]。

mod events;
mod load;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Context, Entity, Window};
use nsh_engine::HostChannel;
use nsh_platform_gpui::package::TrustStore;
use nsh_platform_gpui::{AppView, LoadedPackage, accept_package};
use serde_json::Value;
use timo_kernel::plugins::{PluginDiagnostic, PluginEntry, PluginState};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio_util::sync::CancellationToken;

use crate::app::DesktopApp;
use crate::bridge::{BridgeCommand, PluginRequest};

use super::host_api::{HostApi, HostUiEvent};
use super::runtime_theme;
use super::runtime_watch::{Beat, Watchdog};
use load::desired_panels;

/// 同时显示的 panel 上限（设计 03 §1）。
const PANEL_LIMIT: usize = 5;
/// `sessions.changed` 节流（设计 04 §5）。
const SESSION_THROTTLE: Duration = Duration::from_millis(500);
/// 自动禁用的崩溃窗口（设计 05 §3）。
const CRASH_WINDOW: Duration = Duration::from_secs(60);
/// 每个插件保留的诊断条数（设计 03 §5）。
const DIAGNOSTIC_LIMIT: usize = 50;

/// 运行期要求主窗口执行的一次副作用。
#[derive(Debug, Clone)]
pub(crate) enum RuntimeEffect {
    /// 通知栏文本（已本地化）。
    Notice(String),
    /// 切换到已有会话。
    OpenSession(String),
    /// 用系统默认应用打开（已校验作用域）。
    OpenPath(String),
    /// 插入或提交提示词。
    SendPrompt {
        /// 文本。
        text: String,
        /// 是否直接提交。
        submit: bool,
    },
    /// 打开插件详情（面板容器到 P1-d 才渲染）。
    OpenPanel {
        /// 插件 id。
        plugin: String,
    },
    /// 写剪贴板。
    Clipboard(String),
}

/// 一个期望出现的 panel（排序后取前 [`PANEL_LIMIT`] 个）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct DesiredPanel {
    id: String,
    version: String,
    title: String,
    order: i32,
}

/// 实例阶段（设计 05 §2）。
enum Phase {
    /// 已装载，等待 1s 健康门。
    Pending {
        /// 健康门到点时间。
        ready_at: Instant,
    },
    /// 健康门通过，看门狗运行中。
    Running,
}

/// 一个 slot 实例及其宿主面。
struct PanelSlot {
    plugin: String,
    version: String,
    view: Entity<AppView>,
    channel: HostChannel,
    host: HostApi,
    error: Arc<Mutex<Option<String>>>,
    loaded: LoadedPackage,
    phase: Phase,
    watchdog: Watchdog,
    cancel: CancellationToken,
}

/// 一条按插件归并的诊断。
struct Diagnostic {
    code: String,
    message: String,
    count: u32,
}

/// 全部 panel 实例与宿主状态的持有者。
pub(crate) struct PluginRuntime {
    data_dir: PathBuf,
    work_dir: PathBuf,
    sessions_dir: PathBuf,
    commands: UnboundedSender<BridgeCommand>,
    kernel: UnboundedSender<PluginRequest>,
    ui_tx: UnboundedSender<HostUiEvent>,
    ui_rx: UnboundedReceiver<HostUiEvent>,
    trust: Option<TrustStore>,
    trust_error: Option<String>,
    entries: Vec<PluginEntry>,
    dirty: bool,
    slots: Vec<PanelSlot>,
    failed: BTreeMap<String, String>,
    overflow: BTreeSet<String>,
    diagnostics: BTreeMap<String, VecDeque<Diagnostic>>,
    crashes: BTreeMap<String, VecDeque<Instant>>,
    auto_disabled: BTreeSet<String>,
    patches: BTreeMap<String, BTreeMap<String, String>>,
    theme_tokens: BTreeMap<String, String>,
    theme_snapshot: Value,
    dark: bool,
    sessions: Option<(Instant, BTreeSet<String>)>,
    known_sessions: Option<BTreeSet<String>>,
    pending: Vec<RuntimeEffect>,
}

impl PluginRuntime {
    /// 建立运行期；数据目录不可用时返回原因（插件不启动，主应用照常）。
    ///
    /// # Errors
    /// 平台数据/会话目录不可得 → 人读文本。
    pub(crate) fn new(
        commands: UnboundedSender<BridgeCommand>,
        kernel: UnboundedSender<PluginRequest>,
        dark: bool,
        theme_tokens: BTreeMap<String, String>,
    ) -> Result<Self, String> {
        let data_dir = timo_kernel::data_dir().map_err(|error| error.to_string())?;
        let sessions_dir =
            timo_kernel::Kernel::sessions_dir(None).map_err(|error| error.to_string())?;
        let work_dir = std::env::current_dir().unwrap_or_else(|_| data_dir.clone());
        let (ui_tx, ui_rx) = unbounded_channel();
        let theme_snapshot = runtime_theme::snapshot(dark, &theme_tokens, None);
        Ok(Self {
            data_dir,
            work_dir,
            sessions_dir,
            commands,
            kernel,
            ui_tx,
            ui_rx,
            trust: None,
            trust_error: None,
            entries: vec![],
            dirty: false,
            slots: vec![],
            failed: BTreeMap::new(),
            overflow: BTreeSet::new(),
            diagnostics: BTreeMap::new(),
            crashes: BTreeMap::new(),
            auto_disabled: BTreeSet::new(),
            patches: BTreeMap::new(),
            theme_tokens,
            theme_snapshot,
            dark,
            sessions: None,
            known_sessions: None,
            pending: vec![],
        })
    }

    /// 接收插件清单；返回附带了运行期诊断的副本给界面。
    pub(crate) fn set_entries(&mut self, entries: Vec<PluginEntry>) -> Vec<PluginEntry> {
        self.dirty = true;
        for entry in &entries {
            if !entry.enabled || entry.state == PluginState::Error {
                self.auto_disabled.remove(&entry.id);
                self.crashes.remove(&entry.id);
                self.failed.remove(&entry.id);
            }
            if self
                .failed
                .get(&entry.id)
                .is_some_and(|version| version != &entry.version)
            {
                self.failed.remove(&entry.id);
            }
        }
        self.entries = entries;
        self.decorated()
    }

    /// 标记会话 id 变化；节流后广播 `sessions.changed`。
    pub(crate) fn mark_sessions(&mut self, ids: Vec<String>) {
        let next: BTreeSet<String> = ids.into_iter().collect();
        let Some(previous) = self.known_sessions.replace(next.clone()) else {
            return;
        };
        let mut changed: BTreeSet<String> = next.difference(&previous).cloned().collect();
        changed.extend(previous.difference(&next).cloned());
        if changed.is_empty() {
            return;
        }
        match &mut self.sessions {
            Some((_, pending)) => pending.extend(changed),
            None => self.sessions = Some((Instant::now() + SESSION_THROTTLE, changed)),
        }
    }

    /// 本轮渲染是否需要先协调实例。
    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 清单变化后按需装载/卸载实例。
    pub(crate) fn reconcile_if_dirty(&mut self, window: &mut Window, cx: &mut Context<DesktopApp>) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let (desired, overflow) = desired_panels(&self.entries);
        self.reconcile_slots(&desired, cx);
        let fresh: Vec<String> = overflow.difference(&self.overflow).cloned().collect();
        for id in fresh {
            self.note(
                &id,
                "E751",
                format!("more than {PANEL_LIMIT} panels; this panel is not displayed"),
            );
        }
        self.overflow = overflow;
        let now = Instant::now();
        for wanted in desired {
            self.ensure_slot(&wanted, now, window, cx);
        }
    }

    /// 同步主应用明暗/主题；只在整个 token 表变化时下发。
    pub(crate) fn sync_theme(&mut self, dark: bool, cx: &mut Context<DesktopApp>) {
        let tokens = runtime_theme::tokens(cx.theme());
        let snapshot = runtime_theme::snapshot(dark, &tokens, None);
        if self.dark == dark && self.theme_snapshot == snapshot {
            return;
        }
        self.dark = dark;
        self.theme_tokens = tokens;
        self.theme_snapshot = snapshot;
        let plugins: Vec<String> = self
            .slots
            .iter()
            .map(|slot| slot.plugin.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        for plugin in plugins {
            self.apply_patch(&plugin, cx);
        }
    }

    /// 健康门、错误与看门狗。
    pub(crate) fn tick(&mut self, now: Instant, cx: &mut Context<DesktopApp>) {
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
        }

        let due: Vec<(usize, String, String)> = self
            .slots
            .iter()
            .enumerate()
            .filter(
                |(_, slot)| matches!(slot.phase, Phase::Pending { ready_at } if ready_at <= now),
            )
            .map(|(index, slot)| (index, slot.plugin.clone(), slot.version.clone()))
            .collect();
        let mut accepted: Vec<(String, String)> = vec![];
        let mut rejected: Vec<(String, String, String)> = vec![];
        for (index, plugin, version) in due {
            let outcome = match self.trust.as_mut() {
                Some(trust) => accept_package(trust, &self.slots[index].loaded),
                None => Err(self.trust_error.clone().unwrap_or_default()),
            };
            match outcome {
                Ok(()) => accepted.push((plugin, version)),
                Err(error) => rejected.push((plugin, version, error)),
            }
        }
        for (plugin, version) in &accepted {
            if let Some(slot) = self
                .slots
                .iter_mut()
                .find(|slot| &slot.plugin == plugin && &slot.version == version)
            {
                slot.phase = Phase::Running;
                slot.watchdog = Watchdog::new(now);
            }
        }
        for (plugin, version, error) in rejected {
            if let Some(index) = self
                .slots
                .iter()
                .position(|slot| slot.plugin == plugin && slot.version == version)
            {
                let mut slot = self.slots.remove(index);
                stop_slot(&mut slot, cx);
            }
            self.failed.insert(plugin.clone(), version);
            self.note(&plugin, "E711", format!("accept failed: {error}"));
        }
        // 新版本健康后与旧实例交换：同一插件其它版本的 Running 实例下线。
        let stale: Vec<usize> = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| {
                matches!(slot.phase, Phase::Running)
                    && accepted
                        .iter()
                        .any(|(plugin, version)| plugin == &slot.plugin && version != &slot.version)
            })
            .map(|(index, _)| index)
            .collect();
        for index in stale.into_iter().rev() {
            let mut old = self.slots.remove(index);
            stop_slot(&mut old, cx);
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
        for (index, message) in stops.into_iter().rev() {
            let mut slot = self.slots.remove(index);
            let plugin = slot.plugin.clone();
            let version = slot.version.clone();
            stop_slot(&mut slot, cx);
            self.note(&plugin, "E742", message);
            self.failed.insert(plugin.clone(), version);
            self.record_crash(&plugin, now);
        }
    }

    /// 把运行期诊断合并回界面条目；有变化返回 true。
    pub(crate) fn apply_diagnostics(&self, entries: &mut [PluginEntry]) -> bool {
        let mut changed = false;
        for entry in entries.iter_mut() {
            let mut diagnostics = self
                .entries
                .iter()
                .find(|base| base.id == entry.id)
                .map_or_else(Vec::new, |base| base.diagnostics.clone());
            if let Some(list) = self.diagnostics.get(&entry.id) {
                diagnostics.extend(list.iter().map(|item| PluginDiagnostic {
                    code: item.code.clone(),
                    message: if item.count > 1 {
                        format!("{} (x{})", item.message, item.count)
                    } else {
                        item.message.clone()
                    },
                }));
            }
            if diagnostics != entry.diagnostics {
                entry.diagnostics = diagnostics;
                changed = true;
            }
        }
        changed
    }

    /// 退出时停止全部实例（设计 05 §2）。
    pub(crate) fn shutdown(&mut self, cx: &mut App) {
        for mut slot in self.slots.drain(..) {
            stop_slot(&mut slot, cx);
        }
        self.sessions = None;
    }

    fn decorated(&self) -> Vec<PluginEntry> {
        let mut entries = self.entries.clone();
        let _ = self.apply_diagnostics(&mut entries);
        entries
    }

    fn note(&mut self, plugin: &str, code: &str, message: impl Into<String>) {
        let message = message.into();
        let list = self.diagnostics.entry(plugin.to_string()).or_default();
        if let Some(last) = list.back_mut()
            && last.code == code
            && last.message == message
        {
            last.count = last.count.saturating_add(1);
            return;
        }
        if list.len() >= DIAGNOSTIC_LIMIT {
            list.pop_front();
        }
        list.push_back(Diagnostic {
            code: code.to_string(),
            message,
            count: 1,
        });
    }
}

fn stop_slot(slot: &mut PanelSlot, cx: &mut App) {
    slot.cancel.cancel();
    slot.view.read(cx).stop();
}
