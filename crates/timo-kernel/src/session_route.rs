//! 会话分支上的路由状态与选择恢复（设计 10 §4.2；对齐蓝本 virtual-models.ts）。
//!
//! 纯读函数：entry 流进、模型出；不碰磁盘、不碰凭证。

use serde_json::Value;
use timo_ai::{Message, Model, StopReason};
use timo_session::{Entry, kinds};

/// 分支上最后一条 `model.change` 的 `(provider, model_id)`。
pub(crate) fn last_model_change(entries: &[Entry]) -> Option<(String, String)> {
    entries
        .iter()
        .rev()
        .find(|entry| entry.kind == kinds::MODEL_CHANGE)
        .and_then(|entry| pair(&entry.payload))
}

/// 分支上某个虚拟模型最近一次路由状态。
pub(crate) fn virtual_model_state(
    entries: &[Entry],
    provider: &str,
    model_id: &str,
) -> Option<Value> {
    entries.iter().rev().find_map(|entry| {
        if entry.kind != kinds::VIRTUAL_MODEL_STATE {
            return None;
        }
        let (entry_provider, entry_model) = pair(&entry.payload)?;
        if entry_provider != provider || entry_model != model_id {
            return None;
        }
        entry.payload.get("state").cloned()
    })
}

/// 恢复选择：最后一条 `model.change` 指向仍注册的虚拟模型时保持它，
/// 否则退回最后一条成功响应记录的物理模型（设计 10 §4.2）。
pub(crate) fn branch_selection(
    entries: &[Entry],
    is_registered_virtual: impl Fn(&str, &str) -> bool,
    resolve: impl Fn(&str, &str) -> Option<Model>,
) -> Option<Model> {
    if let Some((provider, model_id)) = last_model_change(entries)
        && is_registered_virtual(&provider, &model_id)
        && let Some(model) = resolve(&provider, &model_id)
        && model.is_virtual()
    {
        return Some(model);
    }
    let (provider, model_id) = last_physical_response(entries)?;
    resolve(&provider, &model_id).filter(|model| !model.is_virtual())
}

/// 最后一条成功（非错误/中止）assistant 消息的 `(provider, model)`。
fn last_physical_response(entries: &[Entry]) -> Option<(String, String)> {
    entries.iter().rev().find_map(|entry| {
        if entry.kind != kinds::MESSAGE {
            return None;
        }
        let Message::Assistant(message) =
            serde_json::from_value::<Message>(entry.payload.clone()).ok()?
        else {
            return None;
        };
        if matches!(message.stop_reason, StopReason::Error | StopReason::Aborted) {
            return None;
        }
        Some((message.provider, message.model))
    })
}

fn pair(payload: &Value) -> Option<(String, String)> {
    Some((
        payload.get("provider")?.as_str()?.to_string(),
        payload.get("model_id")?.as_str()?.to_string(),
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use timo_ai::{ApiKind, AssistantMessage};
    use timo_session::Entry;

    fn entry(kind: &str, payload: Value) -> Entry {
        Entry {
            v: timo_session::ENTRY_VERSION,
            id: "01TEST".into(),
            ts: "2026-10-27T00:00:00.000Z".into(),
            parent_id: None,
            kind: kind.to_string(),
            payload,
        }
    }

    fn change(provider: &str, id: &str) -> Entry {
        entry(
            kinds::MODEL_CHANGE,
            serde_json::json!({ "provider": provider, "model_id": id }),
        )
    }

    fn state(provider: &str, id: &str, value: Value) -> Entry {
        entry(
            kinds::VIRTUAL_MODEL_STATE,
            serde_json::json!({ "provider": provider, "model_id": id, "state": value }),
        )
    }

    fn response(provider: &str, id: &str, stop: StopReason) -> Entry {
        let mut message = AssistantMessage::pending("openai-completions", provider, id, 1);
        message.stop_reason = stop;
        entry(
            kinds::MESSAGE,
            serde_json::to_value(Message::Assistant(message)).unwrap(),
        )
    }

    fn physical(provider: &str, id: &str) -> Model {
        Model::descriptor(provider, id, ApiKind::OpenaiCompletions)
    }

    fn catalog(provider: &str, id: &str) -> Option<Model> {
        if provider == "router" && id == "auto" {
            Some(Model::virtual_entry("router", "auto", "Auto"))
        } else {
            Some(physical(provider, id))
        }
    }

    #[test]
    fn state_lookup_keeps_the_latest_matching_entry() {
        let entries = vec![
            state("router", "auto", serde_json::json!({ "phase": "plan" })),
            state("other", "x", serde_json::json!({ "phase": "ignored" })),
            state("router", "auto", serde_json::json!({ "phase": "build" })),
        ];
        assert_eq!(
            virtual_model_state(&entries, "router", "auto"),
            Some(serde_json::json!({ "phase": "build" }))
        );
        assert_eq!(virtual_model_state(&entries, "router", "missing"), None);
        assert_eq!(last_model_change(&entries), None);
    }

    #[test]
    fn a_registered_virtual_change_holds_the_selection() {
        let entries = vec![
            response("mock", "chat", StopReason::Stop),
            change("router", "auto"),
            response("mock", "chat", StopReason::ToolUse),
        ];
        let picked = branch_selection(&entries, |p, m| p == "router" && m == "auto", catalog)
            .expect("selection");
        assert!(picked.is_virtual());
    }

    #[test]
    fn an_unregistered_virtual_change_falls_back_to_the_last_response() {
        let entries = vec![
            response("mock", "old", StopReason::Stop),
            change("router", "auto"),
            response("mock", "chat", StopReason::ToolUse),
            response("mock", "failed", StopReason::Error),
        ];
        let picked = branch_selection(&entries, |_, _| false, catalog).expect("fallback");
        assert_eq!(
            (picked.provider.as_str(), picked.id.as_str()),
            ("mock", "chat")
        );
        assert!(!picked.is_virtual());
    }

    #[test]
    fn a_physical_change_does_not_hold() {
        let entries = vec![
            change("mock", "small"),
            response("mock", "chat", StopReason::Stop),
        ];
        let picked = branch_selection(&entries, |_, _| true, catalog).expect("fallback");
        assert_eq!(picked.id, "chat");
    }
}
