//! 会话读取：entry 流恢复、上下文投影、列表扫描（设计 04 §4~5）。

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use timo_ai::Message;

use crate::Result;
use crate::entry::{Entry, SessionHeader, kinds};
use crate::error::SessionError;

/// 列表扫描时单文件头部读取上限（architecture/05 §2）。
const HEADER_SCAN_CAP: u64 = 1024 * 1024;

/// 读取全部完整 entry；**末行截断容忍丢弃**，中间行损坏报错（设计 04 §5）。
///
/// # Errors
/// 中间行非法 JSON → [`SessionError::CorruptEntry`]；IO 错误 → [`SessionError::Io`]。
pub fn read_entries(path: &Path) -> Result<Vec<Entry>> {
    let raw = std::fs::read_to_string(path).map_err(|e| SessionError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    let mut out = vec![];
    let lines = raw.lines().enumerate();
    let total = raw.lines().count();
    for (i, line) in lines {
        if line.trim().is_empty() {
            continue;
        }
        let is_last_data_line = i + 1 == total;
        match serde_json::from_str::<Entry>(line) {
            Ok(e) => out.push(e),
            Err(reason) if is_last_data_line && !raw.ends_with('\n') => {
                // 半条写入（kill -9）：丢弃且不报错
                let _ = reason;
                break;
            }
            Err(e) => {
                return Err(SessionError::CorruptEntry {
                    path: path.to_path_buf(),
                    line: i + 1,
                    reason: e.to_string(),
                });
            }
        }
    }
    Ok(out)
}

/// 沿 leaf 链投影 LLM 上下文（设计 04：树形结构 MVP 只写线性链，投影按通用树实现）。
///
/// 只取 `message` entry 的 payload；最新 `session.compaction` 生效：
/// 摘要消息 + 压缩点之前最近 `keep_recent_messages` 条消息 + 压缩点之后的消息。
#[must_use]
pub fn project_context(entries: &[Entry]) -> Vec<Message> {
    let chain = leaf_chain(entries);
    let compaction = chain.iter().rposition(|e| e.kind == kinds::COMPACTION);
    let Some(compaction_index) = compaction else {
        return messages_of(&chain[..]);
    };
    let entry = chain[compaction_index];
    let summary = entry
        .payload
        .get("summary")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.trim().is_empty());
    let Some(summary) = summary else {
        // 摘要损坏：忽略该检查点，退回原始消息链。
        return messages_of(&chain[..]);
    };
    let keep = entry
        .payload
        .get("keep_recent_messages")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize;

    let before = &chain[..compaction_index];
    let mut start = if keep == 0 { before.len() } else { 0 };
    let mut seen = 0usize;
    for (i, e) in before.iter().enumerate().rev() {
        if e.kind == kinds::MESSAGE {
            seen += 1;
            if seen == keep {
                start = i;
                break;
            }
        }
    }

    let mut out = vec![crate::entry::summary_message(
        summary,
        entry_timestamp_ms(entry),
    )];
    out.extend(messages_of(&chain[start..]));
    out
}

fn messages_of(chain: &[&Entry]) -> Vec<Message> {
    chain
        .iter()
        .filter(|e| e.kind == kinds::MESSAGE)
        .filter_map(|e| serde_json::from_value::<Message>(e.payload.clone()).ok())
        .collect()
}

fn entry_timestamp_ms(entry: &Entry) -> i64 {
    time::OffsetDateTime::parse(&entry.ts, &time::format_description::well_known::Rfc3339)
        .map(|t| (t.unix_timestamp_nanos() / 1_000_000) as i64)
        .unwrap_or(0)
}

/// 从 leaf（未被任何 entry 引用为 parent 的 entry）回溯父链，返回根→leaf 顺序。
fn leaf_chain(entries: &[Entry]) -> Vec<&Entry> {
    let by_id: std::collections::HashMap<&str, &Entry> =
        entries.iter().map(|e| (e.id.as_str(), e)).collect();
    let referenced: HashSet<&str> = entries
        .iter()
        .filter_map(|e| e.parent_id.as_deref())
        .collect();
    let leaf = entries
        .iter()
        .rev()
        .find(|e| !referenced.contains(e.id.as_str()));
    let mut chain: Vec<&Entry> = vec![];
    let mut cur = leaf;
    let mut guard = 0usize;
    while let Some(e) = cur {
        chain.push(e);
        guard += 1;
        if guard > entries.len() {
            break; // 防环（理论不可达：追加时 parent 取当前 leaf）
        }
        cur = e.parent_id.as_deref().and_then(|p| by_id.get(p).copied());
    }
    chain.reverse();
    chain
}

/// 会话摘要（列表展示用）。
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// 会话 id。
    pub id: String,
    /// 文件路径。
    pub path: PathBuf,
    /// 工作目录。
    pub cwd: String,
    /// 创建时间（unix 毫秒）。
    pub created_ms: i64,
    /// 文件修改时间（unix 毫秒；取不到为 0）。
    pub modified_ms: i64,
    /// message entry 数。
    pub message_count: usize,
    /// 首条 user 消息文本（截 120 字符）。
    pub first_message: String,
}

/// 扫描会话根目录与一级项目分区内的全部 `*.jsonl`（读首条 header，≤1MB cap），
/// 按修改时间倒序；同一会话 id 只保留最新的一份（phase-3.5/07）。
#[must_use]
pub fn list_sessions(dir: &Path) -> Vec<SessionInfo> {
    let mut paths = vec![];
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    for item in rd.flatten() {
        let path = item.path();
        if path.is_dir() {
            if let Ok(inner) = std::fs::read_dir(&path) {
                for entry in inner.flatten() {
                    let candidate = entry.path();
                    if candidate.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                        paths.push(candidate);
                    }
                }
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            paths.push(path);
        }
    }
    let mut best: std::collections::BTreeMap<String, SessionInfo> =
        std::collections::BTreeMap::new();
    for info in paths.iter().filter_map(|p| scan_session(p)) {
        match best.get(&info.id) {
            // 同一 id 同时出现在分区目录与旧的平铺位置时，只保留一份。
            // 优先分区副本（现行布局），其次修改时间新的；两者都与 readdir 顺序无关。
            Some(current) if prefer(current, &info, dir) => {}
            _ => {
                best.insert(info.id.clone(), info);
            }
        }
    }
    let mut out: Vec<SessionInfo> = best.into_values().collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.modified_ms));
    out
}

/// 已有副本 `kept` 是否比候选 `other` 更该保留。
fn prefer(kept: &SessionInfo, other: &SessionInfo, dir: &Path) -> bool {
    let nested = |info: &SessionInfo| info.path.parent().is_some_and(|parent| parent != dir);
    match (nested(kept), nested(other)) {
        (true, false) => true,
        (false, true) => false,
        _ => kept.modified_ms >= other.modified_ms,
    }
}

/// 把 leaf 链（header → 当前叶）序列化为 JSONL；entry 原样输出（phase-3.5/07）。
///
/// # Errors
/// entry 序列化失败 → [`SessionError::CorruptEntry`]。
pub fn serialize_export(entries: &[Entry]) -> Result<String> {
    let mut out = String::new();
    for entry in leaf_chain(entries) {
        let line = serde_json::to_string(entry).map_err(|reason| SessionError::CorruptEntry {
            path: PathBuf::from("<export>"),
            line: 0,
            reason: format!("serialize: {reason}"),
        })?;
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

fn scan_session(path: &Path) -> Option<SessionInfo> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = String::new();
    (&mut file)
        .take(HEADER_SCAN_CAP)
        .read_to_string(&mut buf)
        .ok()?;
    let first_line = buf.lines().next()?;
    let header: Entry = serde_json::from_str(first_line).ok()?;
    if header.kind != kinds::HEADER {
        return None;
    }
    let h: SessionHeader = serde_json::from_value(header.payload.clone()).ok()?;
    let message_count = buf
        .lines()
        .filter_map(|l| serde_json::from_str::<Entry>(l).ok())
        .filter(|e| e.kind == kinds::MESSAGE)
        .count();
    let first_message = buf
        .lines()
        .filter_map(|l| serde_json::from_str::<Entry>(l).ok())
        .find(|e| e.kind == kinds::MESSAGE)
        .and_then(|e| {
            serde_json::from_value::<Message>(e.payload.clone())
                .ok()
                .and_then(first_user_text)
        })
        .unwrap_or_default();
    let modified_ms = file
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Some(SessionInfo {
        id: h.session_id,
        path: path.to_path_buf(),
        cwd: h.cwd,
        created_ms: h.created_ms,
        modified_ms,
        message_count,
        first_message,
    })
}

fn first_user_text(m: Message) -> Option<String> {
    if let Message::User(u) = &m {
        for c in &u.content {
            if let timo_ai::Content::Text { text } = c {
                let t: String = text.chars().take(120).collect();
                return Some(t);
            }
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
