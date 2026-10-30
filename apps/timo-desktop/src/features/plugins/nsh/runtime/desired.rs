//! slot 期望集：由清单与当前会话算出每个插件要挂的实例（设计 03 §1）。
//!
//! 单插件实例上限 4，按 `panel` → `composerTrigger` → `toolCard`（声明顺序）
//! → `entryExtra`（会话顺序）取舍；`panel` 另有全局 5 个的上限。

use std::collections::BTreeSet;

use timo_kernel::plugins::{PluginEntry, PluginKind, PluginOrigin, PluginState, SlotDecl};

/// 同时显示的 panel 上限（设计 03 §1）。
pub(super) const PANEL_LIMIT: usize = 5;
/// 单插件实例总数上限（设计 03 §1）。
pub(super) const INSTANCE_LIMIT: usize = 4;

/// 期望实例的类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum SlotKind {
    Panel,
    ToolCard,
    ComposerTrigger,
    EntryExtra,
}

impl SlotKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Panel => "panel",
            Self::ToolCard => "toolCard",
            Self::ComposerTrigger => "composerTrigger",
            Self::EntryExtra => "entryExtra",
        }
    }
}

/// 一个期望存在的实例。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DesiredSlot {
    pub(super) plugin: String,
    pub(super) version: String,
    pub(super) kind: SlotKind,
    /// `toolCard` 的工具名或 `entryExtra` 的会话 id。
    pub(super) binding: Option<String>,
    /// 容器标题。
    pub(super) title: String,
    /// 同类内排序（`panel` 用）。
    pub(super) order: i32,
}

impl DesiredSlot {
    pub(super) fn key(&self) -> String {
        key_of(&self.plugin, self.kind, self.binding.as_deref())
    }
}

/// 被上限舍弃的实例（每个只记一次 E751）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SlotOverflow {
    pub(super) plugin: String,
    pub(super) key: String,
    pub(super) kind: SlotKind,
    /// 因 panel 全局上限被舍弃（否则是单插件实例上限）。
    pub(super) panel_cap: bool,
}

pub(super) fn key_of(plugin: &str, kind: SlotKind, binding: Option<&str>) -> String {
    format!("{plugin}:{}:{}", kind.as_str(), binding.unwrap_or(""))
}

pub(super) fn desired_slots(
    entries: &[PluginEntry],
    sessions: &[(String, String)],
) -> (Vec<DesiredSlot>, Vec<SlotOverflow>) {
    let mut overflow = vec![];
    let mut panels: Vec<(&PluginEntry, &SlotDecl)> = entries
        .iter()
        .filter(|entry| usable(entry))
        .filter_map(|entry| decl(entry, "panel").map(|slot| (entry, slot)))
        .collect();
    panels.sort_by(|(left_entry, left), (right_entry, right)| {
        left.order
            .unwrap_or(0)
            .cmp(&right.order.unwrap_or(0))
            .then_with(|| {
                title_of(left_entry, left)
                    .to_lowercase()
                    .cmp(&title_of(right_entry, right).to_lowercase())
            })
            .then_with(|| left_entry.id.cmp(&right_entry.id))
    });
    for (entry, _) in panels.iter().skip(PANEL_LIMIT) {
        overflow.push(SlotOverflow {
            plugin: entry.id.clone(),
            key: key_of(&entry.id, SlotKind::Panel, None),
            kind: SlotKind::Panel,
            panel_cap: true,
        });
    }
    panels.truncate(PANEL_LIMIT);
    let shown: BTreeSet<&str> = panels.iter().map(|(entry, _)| entry.id.as_str()).collect();

    let mut desired = vec![];
    for entry in entries.iter().filter(|entry| usable(entry)) {
        let mut items = vec![];
        if shown.contains(entry.id.as_str())
            && let Some(slot) = decl(entry, "panel")
        {
            items.push(slot_of(
                entry,
                SlotKind::Panel,
                None,
                title_of(entry, slot),
                slot,
            ));
        }
        if let Some(slot) = decl(entry, "composerTrigger") {
            items.push(slot_of(
                entry,
                SlotKind::ComposerTrigger,
                None,
                title_of(entry, slot),
                slot,
            ));
        }
        let mut seen = BTreeSet::new();
        for slot in entry
            .declared
            .slots
            .iter()
            .filter(|slot| slot.kind == "toolCard")
        {
            for tool in &slot.tools {
                if !seen.insert(tool.clone()) {
                    continue;
                }
                items.push(slot_of(
                    entry,
                    SlotKind::ToolCard,
                    Some(tool.clone()),
                    tool.clone(),
                    slot,
                ));
            }
        }
        if let Some(slot) = decl(entry, "entryExtra") {
            for (id, title) in sessions {
                items.push(slot_of(
                    entry,
                    SlotKind::EntryExtra,
                    Some(id.clone()),
                    title.clone(),
                    slot,
                ));
            }
        }
        for extra in items.iter().skip(INSTANCE_LIMIT) {
            overflow.push(SlotOverflow {
                plugin: entry.id.clone(),
                key: extra.key(),
                kind: extra.kind,
                panel_cap: false,
            });
        }
        items.truncate(INSTANCE_LIMIT);
        desired.extend(items);
    }
    (desired, overflow)
}

fn slot_of(
    entry: &PluginEntry,
    kind: SlotKind,
    binding: Option<String>,
    title: String,
    slot: &SlotDecl,
) -> DesiredSlot {
    DesiredSlot {
        plugin: entry.id.clone(),
        version: entry.version.clone(),
        kind,
        binding,
        title,
        order: slot.order.unwrap_or(0),
    }
}

fn usable(entry: &PluginEntry) -> bool {
    entry.kind == PluginKind::Gui
        && entry.origin == PluginOrigin::Installed
        && entry.enabled
        && entry.state != PluginState::Error
        && !entry.version.is_empty()
}

fn decl<'a>(entry: &'a PluginEntry, kind: &str) -> Option<&'a SlotDecl> {
    entry.declared.slots.iter().find(|slot| slot.kind == kind)
}

fn title_of(entry: &PluginEntry, slot: &SlotDecl) -> String {
    slot.title.clone().unwrap_or_else(|| entry.name.clone())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::path::PathBuf;

    use timo_kernel::plugins::{Declared, PermissionTier, PluginSource};

    use super::*;

    fn entry(id: &str, slots: Vec<SlotDecl>) -> PluginEntry {
        PluginEntry {
            id: id.into(),
            kind: PluginKind::Gui,
            origin: PluginOrigin::Installed,
            name: id.into(),
            version: "1.0.0".into(),
            enabled: true,
            tier: PermissionTier::ReadOnly,
            declared: Declared {
                slots,
                ..Declared::default()
            },
            state: PluginState::Enabled,
            source: PluginSource::LocalFile,
            source_hash: format!("sha256:{}", "0".repeat(64)),
            configurable: false,
            diagnostics: vec![],
            path: PathBuf::new(),
        }
    }

    fn panel(order: i32) -> SlotDecl {
        SlotDecl {
            kind: "panel".into(),
            title: Some(format!("p{order}")),
            order: Some(order),
            icon: None,
            tools: vec![],
        }
    }

    fn sessions(count: usize) -> Vec<(String, String)> {
        (0..count)
            .map(|index| (format!("s{index}"), format!("会话 {index}")))
            .collect()
    }

    #[test]
    fn panels_are_ordered_and_capped() {
        let entries: Vec<PluginEntry> = (0..7)
            .map(|index| entry(&format!("plugin-{index}"), vec![panel(index)]))
            .collect();
        let (desired, overflow) = desired_slots(&entries, &[]);
        assert_eq!(desired.len(), PANEL_LIMIT);
        assert_eq!(desired[0].plugin, "plugin-0");
        assert_eq!(overflow.len(), 2);
        assert!(overflow.iter().all(|item| item.panel_cap));
        assert!(overflow.iter().any(|item| item.plugin == "plugin-5"));
    }

    #[test]
    fn disabled_and_broken_plugins_are_skipped() {
        let mut broken = entry("broken", vec![panel(0)]);
        broken.state = PluginState::Error;
        let mut disabled = entry("disabled", vec![panel(1)]);
        disabled.enabled = false;
        let (desired, overflow) =
            desired_slots(&[disabled, broken, entry("ok", vec![panel(2)])], &[]);
        assert!(overflow.is_empty());
        assert_eq!(desired.len(), 1);
        assert_eq!(desired[0].plugin, "ok");
    }

    #[test]
    fn tool_card_expands_per_bound_tool() {
        let slots = vec![SlotDecl {
            kind: "toolCard".into(),
            title: Some("卡片".into()),
            order: None,
            icon: None,
            tools: vec!["grep".into(), "read".into()],
        }];
        let (desired, overflow) = desired_slots(&[entry("demo", slots)], &[]);
        assert!(overflow.is_empty());
        let bindings: Vec<_> = desired.iter().map(|slot| slot.binding.clone()).collect();
        assert_eq!(
            bindings,
            [Some("grep".to_string()), Some("read".to_string())]
        );
        assert!(desired.iter().all(|slot| slot.kind == SlotKind::ToolCard));
    }

    #[test]
    fn instance_limit_prefers_panel_then_trigger_then_cards() {
        let slots = vec![
            SlotDecl {
                kind: "entryExtra".into(),
                title: None,
                order: None,
                icon: None,
                tools: vec![],
            },
            SlotDecl {
                kind: "toolCard".into(),
                title: None,
                order: None,
                icon: None,
                tools: vec!["a".into(), "b".into()],
            },
            SlotDecl {
                kind: "composerTrigger".into(),
                title: None,
                order: None,
                icon: None,
                tools: vec![],
            },
            panel(0),
        ];
        let (desired, overflow) = desired_slots(&[entry("demo", slots)], &sessions(3));
        let kinds: Vec<_> = desired.iter().map(|slot| slot.kind).collect();
        assert_eq!(
            kinds,
            [
                SlotKind::Panel,
                SlotKind::ComposerTrigger,
                SlotKind::ToolCard,
                SlotKind::ToolCard
            ]
        );
        assert_eq!(overflow.len(), 3);
        assert!(
            overflow
                .iter()
                .all(|item| item.kind == SlotKind::EntryExtra)
        );
    }

    #[test]
    fn entry_extra_follows_session_order() {
        let slots = vec![SlotDecl {
            kind: "entryExtra".into(),
            title: None,
            order: Some(9),
            icon: None,
            tools: vec![],
        }];
        let (desired, _) = desired_slots(&[entry("demo", slots)], &sessions(2));
        let bindings: Vec<_> = desired.iter().map(|slot| slot.binding.clone()).collect();
        assert_eq!(bindings, [Some("s0".to_string()), Some("s1".to_string())]);
    }
}
