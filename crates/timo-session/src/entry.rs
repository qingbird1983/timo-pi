//! Entry 信封与类型（设计 04 §2~3；architecture/05 定稿）。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 当前格式版本（architecture/05 §3：破坏性变更 → v2+迁移器）。
pub const ENTRY_VERSION: u32 = 1;

/// entry 类型名（`type` 字段；未知类型读取端必须跳过，architecture/05 §1.3）。
pub mod kinds {
    /// 会话首条。
    pub const HEADER: &str = "session.header";
    /// 完整消息（role 判别）。
    pub const MESSAGE: &str = "message";
    /// 模型切换。
    pub const MODEL_CHANGE: &str = "model.change";
    /// 思考档位切换。
    pub const THINKING_CHANGE: &str = "model.thinking_level_change";
    /// 轮次记账快照。
    pub const USAGE: &str = "usage";
    /// 扩展私有状态（不入 LLM 上下文，Phase 2 起用）。
    pub const CUSTOM: &str = "custom";
    /// 扩展消息（入 LLM 上下文，Phase 2 起用）。
    pub const CUSTOM_MESSAGE: &str = "custom_message";
    /// 上下文压缩检查点（phase-3.5/04）。
    pub const COMPACTION: &str = "session.compaction";
    /// 虚拟模型路由状态（phase-3.5/10 §4.2；只增不改，`Direct` 不写）。
    pub const VIRTUAL_MODEL_STATE: &str = "session.virtual_model_state";
}

/// 压缩摘要消息的前缀（读端投影与写端共用；phase-3.5/04 §5）。
pub const COMPACTION_SUMMARY_PREFIX: &str = "The conversation history before this point was compacted into the following summary:\n\n<summary>\n";

/// 压缩摘要消息的后缀。
pub const COMPACTION_SUMMARY_SUFFIX: &str = "\n</summary>";

/// 构造注入上下文的摘要消息（user 角色，与 pi 的 compactionSummary 投影一致）。
#[must_use]
pub fn summary_message(summary: &str, timestamp: i64) -> timo_ai::Message {
    timo_ai::user_text(
        &format!("{COMPACTION_SUMMARY_PREFIX}{summary}{COMPACTION_SUMMARY_SUFFIX}"),
        timestamp,
    )
}

/// 从消息中提取压缩摘要正文（非摘要消息返回 None）。
#[must_use]
pub fn extract_summary(message: &timo_ai::Message) -> Option<&str> {
    let timo_ai::Message::User(user) = message else {
        return None;
    };
    let text = user.content.iter().find_map(|c| match c {
        timo_ai::Content::Text { text } => Some(text.as_str()),
        _ => None,
    })?;
    text.strip_prefix(COMPACTION_SUMMARY_PREFIX)?
        .strip_suffix(COMPACTION_SUMMARY_SUFFIX)
}

/// 会话首条 payload（每文件第一条，parent_id = null）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionHeader {
    /// 格式版本（与信封 `v` 一致）。
    pub format_version: u32,
    /// 会话 id（ULID；= 文件名主干）。
    pub session_id: String,
    /// 创建时间（unix 毫秒）。
    pub created_ms: i64,
    /// 会话工作目录。
    pub cwd: String,
    /// 分叉来源会话（预留）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    /// 初始模型（`provider/id`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_model: Option<String>,
}

/// 会话 entry 信封（architecture/05 §3）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    /// 格式版本。
    pub v: u32,
    /// entry id（ULID 字典序=时间序；进程内单调工厂防时钟回拨）。
    pub id: String,
    /// RFC3339（毫秒）UTC 时间戳。
    pub ts: String,
    /// 父 entry id；根为 null。
    pub parent_id: Option<String>,
    /// 类型名（[`kinds`] 常量或 `custom.*`）。
    #[serde(rename = "type")]
    pub kind: String,
    /// 类型自定义载荷；未知类型按原样保留。
    pub payload: Value,
}

impl Entry {
    /// 从 unix 毫秒格式化 RFC3339（毫秒）时间戳。
    ///
    /// # Errors
    /// 毫秒值超出可表示范围时返回 [`crate::SessionError::InvalidFile`]。
    pub fn format_ts(ms: i64) -> crate::Result<String> {
        use time::format_description;
        let desc = format_description::parse_borrowed::<2>(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z",
        )
        .map_err(|e| crate::SessionError::InvalidFile {
            path: PathBuf::from("<ts>"),
            reason: format!("ts format desc: {e}"),
        })?;
        time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
            .map_err(|e| crate::SessionError::InvalidFile {
                path: PathBuf::from("<ts>"),
                reason: format!("bad timestamp {ms}: {e}"),
            })?
            .to_offset(time::UtcOffset::UTC)
            .format(&desc)
            .map_err(|e| crate::SessionError::InvalidFile {
                path: PathBuf::from("<ts>"),
                reason: format!("format timestamp {ms}: {e}"),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_roundtrip() {
        let raw = r#"{"v":1,"id":"01ABC","ts":"2026-10-26T03:21:00.512Z","parent_id":null,"type":"message","payload":{"role":"user","content":[{"type":"text","text":"hi"}],"timestamp":1}}"#;
        let e: Entry = serde_json::from_str(raw).expect("parse");
        assert_eq!(e.kind, kinds::MESSAGE);
        assert_eq!(e.v, 1);
        let back = serde_json::to_string(&e).expect("ser");
        let e2: Entry = serde_json::from_str(&back).expect("reparse");
        assert_eq!(e, e2);
    }

    #[test]
    fn unknown_kind_preserved() {
        let raw =
            r#"{"v":1,"id":"X","ts":"t","parent_id":null,"type":"custom.demo","payload":{"n":1}}"#;
        let e: Entry = serde_json::from_str(raw).expect("parse");
        assert_eq!(e.kind, "custom.demo");
        assert_eq!(e.payload["n"], 1);
    }

    #[test]
    fn ts_formatting() {
        let s = Entry::format_ts(1_789_000_000_000).expect("fmt");
        assert!(s.ends_with('Z'));
        assert!(s.contains('.'));
    }
}
