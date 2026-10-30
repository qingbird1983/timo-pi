//! 工具 trace 的数据面：行状态、调用快照与当前会话的最近调用（设计 01 §6、03 §1.1）。
//!
//! 聊天行渲染只读这里的数据；插件 `toolCard` 拿到的也是这份快照（再经宿主脱敏）。

use std::collections::BTreeMap;

use super::ChatState;

/// 工具 trace 行状态（设计 01 §6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceStatus {
    /// 已请求、尚未开始。
    Queued,
    /// 等待用户审阅。
    Review,
    /// 执行中。
    Running,
    /// 成功完成。
    Done,
    /// 审阅被拒绝（无副作用）。
    Blocked,
    /// 失败（工具结果 is_error）。
    Failed,
    /// 恢复会话时缺少配对结果。
    Interrupted,
}

impl TraceStatus {
    /// 事件/数据面使用的稳定状态词（不翻译）。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Review => "review",
            Self::Running => "running",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

/// 一次工具调用在 trace 中的一行。
#[derive(Debug, Clone)]
pub struct TraceEntry {
    /// 调用 id。
    pub call_id: String,
    /// 工具名（技术标识，不翻译）。
    pub name: String,
    /// 关键参数摘要。
    pub summary: String,
    /// 参数 JSON（`toolCard` 数据源，已按 8 KiB 字符截断）。
    pub input: String,
    /// 状态。
    pub status: TraceStatus,
    /// 结果正文（折叠展示）。
    pub output: Option<String>,
    /// 执行时长。
    pub duration_ms: Option<u64>,
}

/// 某个工具最近一次调用的快照（插件 `toolCard` 的输入；设计 03 §1.1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallSnapshot {
    /// 工具名。
    pub name: String,
    /// 状态词（`queued`/`running`/…）。
    pub status: &'static str,
    /// 参数 JSON。
    pub input: String,
    /// 结果正文（可能为空）。
    pub output: String,
}

impl ChatState {
    /// 每个工具在当前会话里最近一次调用，按工具名升序（`toolCard` 数据源）。
    #[must_use]
    pub fn latest_tool_calls(&self) -> Vec<ToolCallSnapshot> {
        let mut latest: BTreeMap<&str, &TraceEntry> = BTreeMap::new();
        for row in &self.rows {
            let super::RowKind::Assistant(assistant) = &row.kind else {
                continue;
            };
            for entry in &assistant.trace {
                latest.insert(entry.name.as_str(), entry);
            }
        }
        latest
            .into_iter()
            .map(|(name, entry)| ToolCallSnapshot {
                name: name.to_string(),
                status: entry.status.as_str(),
                input: entry.input.clone(),
                output: entry.output.clone().unwrap_or_default(),
            })
            .collect()
    }
}
