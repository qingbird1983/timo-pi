//! B 轨 slot 运行时：装载、健康门、看门狗与数据推送（设计 05 §1~§4）。
//!
//! 每个 slot 实例一个 `AppView`。装载先验签、再进 1s 健康门，无错才
//! `accept_package`；运行期错误与看门狗失败都移除实例并记诊断。容器只读取
//! 健康通过的实例（`visible_*`）。
//!
//! 期望集在 [`desired`]，装载/排列在 [`load`]，会话广播与槽位数据在 [`events`]。

mod data;
mod desired;
mod events;
mod load;
mod tick;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Context, Entity, Window};
use nsh_engine::HostChannel;
use nsh_platform_gpui::package::TrustStore;
use nsh_platform_gpui::{AppView, LoadedPackage};
use serde_json::Value;
use timo_kernel::plugins::{PluginDiagnostic, PluginEntry, PluginState};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio_util::sync::CancellationToken;

use crate::app::DesktopApp;
use crate::bridge::{BridgeCommand, PluginRequest};
use crate::features::chat::ToolCallSnapshot;

use super::host_api::{HostApi, HostUiEvent};
use super::runtime_theme;
use super::runtime_watch::Watchdog;
use desired::{INSTANCE_LIMIT, PANEL_LIMIT, SlotKind, desired_slots};

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
    /// 打开插件详情。
    OpenPanel {
        /// 插件 id。
        plugin: String,
    },
    /// 写剪贴板。
    Clipboard(String),
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

/// 宿主容器要显示的一个实例（只含健康通过的实例）。
pub(crate) struct VisibleSlot {
    /// 插件 id。
    pub(crate) plugin: String,
    /// 工具名（`toolCard`）或会话 id（`entryExtra`）。
    pub(crate) binding: Option<String>,
    /// 容器标题。
    pub(crate) title: String,
    /// 同类内排序（`panel` 用）。
    pub(crate) order: i32,
    /// 插件绘制内容。
    pub(crate) view: Entity<AppView>,
}

/// 一个 slot 实例及其宿主面。
struct SlotInstance {
    key: String,
    plugin: String,
    version: String,
    kind: SlotKind,
    binding: Option<String>,
    /// 传给插件的实例 id（`slot.instanceId`）。
    instance_id: String,
    title: String,
    order: i32,
    view: Entity<AppView>,
    channel: HostChannel,
    host: HostApi,
    error: Arc<Mutex<Option<String>>>,
    loaded: LoadedPackage,
    phase: Phase,
    watchdog: Watchdog,
    cancel: CancellationToken,
    /// 最近一次推给插件的槽位数据（去重）。
    sent: Value,
}

/// 传给插件的实例 id：`panel`/`trigger` 用类别名，其余用绑定值。
fn instance_id(kind: SlotKind, binding: Option<&str>) -> String {
    binding.map(str::to_string).unwrap_or_else(|| match kind {
        SlotKind::Panel => "panel".to_string(),
        SlotKind::ComposerTrigger => "trigger".to_string(),
        SlotKind::ToolCard | SlotKind::EntryExtra => kind.as_str().to_string(),
    })
}

/// 一条按插件归并的诊断。
struct Diagnostic {
    code: String,
    message: String,
    count: u32,
}

/// 全部 slot 实例与宿主状态的持有者。
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
    slots: Vec<SlotInstance>,
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
    /// 侧栏会话 `(id, 标题)`，按显示顺序。
    session_rows: Vec<(String, String)>,
    /// 当前会话 id（`toolCard` 只看当前会话）。
    session_id: Option<String>,
    /// 当前会话每个工具最近一次调用。
    calls: Vec<ToolCallSnapshot>,
    /// 输入框文本快照。
    composer_text: String,
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
            session_rows: vec![],
            session_id: None,
            calls: vec![],
            composer_text: String::new(),
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

    /// 本轮渲染是否需要先协调实例。
    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 清单/会话变化后按需装载/卸载实例。
    pub(crate) fn reconcile_if_dirty(&mut self, window: &mut Window, cx: &mut Context<DesktopApp>) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let (desired, overflow) = desired_slots(&self.entries, &self.session_rows);
        self.reconcile_slots(&desired, cx);
        for item in &overflow {
            if self.overflow.contains(&item.key) {
                continue;
            }
            let reason = if item.panel_cap {
                format!("more than {PANEL_LIMIT} panels; this panel is not displayed")
            } else {
                format!(
                    "more than {INSTANCE_LIMIT} slots; this {} instance is not displayed",
                    item.kind.as_str()
                )
            };
            self.note(&item.plugin, "E751", reason);
        }
        self.overflow = overflow.into_iter().map(|item| item.key).collect();
        let now = Instant::now();
        for wanted in &desired {
            self.ensure_slot(wanted, now, window, cx);
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

    /// 侧栏要显示的 panel：按 `order`/标题/插件 id 排序。
    pub(crate) fn visible_panels(&self) -> Vec<VisibleSlot> {
        let mut slots = self.visible(SlotKind::Panel);
        slots.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
                .then_with(|| left.plugin.cmp(&right.plugin))
        });
        slots
    }

    /// 聊天内工具卡：工具名 → 实例。
    pub(crate) fn visible_tool_cards(&self) -> Vec<VisibleSlot> {
        self.visible(SlotKind::ToolCard)
    }

    /// 会话条目附加区：会话 id → 实例。
    pub(crate) fn visible_entry_extras(&self) -> Vec<VisibleSlot> {
        self.visible(SlotKind::EntryExtra)
    }

    /// 输入区上方触发器（每插件至多一个实例）。
    pub(crate) fn visible_composer_trigger(&self) -> Option<VisibleSlot> {
        self.visible(SlotKind::ComposerTrigger).into_iter().next()
    }

    fn visible(&self, kind: SlotKind) -> Vec<VisibleSlot> {
        self.slots
            .iter()
            .filter(|slot| slot.kind == kind && matches!(slot.phase, Phase::Running))
            .map(|slot| VisibleSlot {
                plugin: slot.plugin.clone(),
                binding: slot.binding.clone(),
                title: slot.title.clone(),
                order: slot.order,
                view: slot.view.clone(),
            })
            .collect()
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

    /// 是否需要对会话数据做一次快照比对（没有相关插件时跳过）。
    pub(crate) fn wants_chat_sync(&self) -> bool {
        self.entries.iter().any(|entry| {
            entry.declared.slots.iter().any(|slot| {
                matches!(
                    slot.kind.as_str(),
                    "toolCard" | "composerTrigger" | "entryExtra"
                )
            }) || entry
                .declared
                .queries
                .iter()
                .any(|query| query == "composer.text")
        })
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

fn stop_slot(slot: &mut SlotInstance, cx: &mut App) {
    slot.cancel.cancel();
    slot.view.read(cx).stop();
}
