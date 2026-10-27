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
    let mut out: Vec<SessionInfo> = paths.iter().filter_map(|p| scan_session(p)).collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.modified_ms));
    out.dedup_by(|a, b| a.id == b.id);
    out
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
mod tests {
    use super::*;
    use crate::writer::{IdGen, SessionWriter};
    use std::sync::Arc;
    use timo_ai::user_text;
    use timo_config::FixedClock;

    fn setup(name: &str) -> (PathBuf, Arc<IdGen>) {
        let d = std::env::temp_dir().join(format!("timo-sess-read-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        (d, Arc::new(IdGen::new()))
    }

    #[test]
    fn roundtrip_and_projection() {
        let (dir, idgen) = setup("rt");
        let mut w =
            SessionWriter::create(&dir, "D:/demo", None, Arc::new(FixedClock::new(1)), idgen)
                .unwrap();
        w.append_raw(
            kinds::MESSAGE,
            serde_json::to_value(user_text("hello", 5)).unwrap(),
        )
        .unwrap();
        w.append_raw("custom.demo", serde_json::json!({"x": 1}))
            .unwrap();
        w.append_raw(
            kinds::MESSAGE,
            serde_json::to_value(user_text("again", 6)).unwrap(),
        )
        .unwrap();
        let entries = read_entries(w.path()).unwrap();
        assert_eq!(entries.len(), 4);
        let msgs = project_context(&entries);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].timestamp(), 5);
    }

    #[test]
    fn projection_uses_the_latest_compaction_checkpoint() {
        let (dir, idgen) = setup("compaction");
        let mut w =
            SessionWriter::create(&dir, "D:/demo", None, Arc::new(FixedClock::new(1)), idgen)
                .unwrap();
        for (i, text) in ["one", "two", "three", "four", "five"].iter().enumerate() {
            w.append_raw(
                kinds::MESSAGE,
                serde_json::to_value(user_text(text, i as i64)).unwrap(),
            )
            .unwrap();
        }
        w.append_raw(
            kinds::COMPACTION,
            serde_json::json!({"summary": "checkpoint", "keep_recent_messages": 2}),
        )
        .unwrap();
        w.append_raw(
            kinds::MESSAGE,
            serde_json::to_value(user_text("six", 6)).unwrap(),
        )
        .unwrap();

        let msgs = project_context(&read_entries(w.path()).unwrap());
        assert_eq!(msgs.len(), 4);
        assert_eq!(crate::extract_summary(&msgs[0]), Some("checkpoint"));
        assert_eq!(msgs[1].timestamp(), 3);
        assert_eq!(msgs[2].timestamp(), 4);
        assert_eq!(msgs[3].timestamp(), 6);
    }

    #[test]
    fn projection_falls_back_when_the_summary_is_empty() {
        let (dir, idgen) = setup("compaction-bad");
        let mut w =
            SessionWriter::create(&dir, "D:/demo", None, Arc::new(FixedClock::new(1)), idgen)
                .unwrap();
        w.append_raw(
            kinds::MESSAGE,
            serde_json::to_value(user_text("one", 1)).unwrap(),
        )
        .unwrap();
        w.append_raw(kinds::COMPACTION, serde_json::json!({"summary": ""}))
            .unwrap();
        w.append_raw(
            kinds::MESSAGE,
            serde_json::to_value(user_text("two", 2)).unwrap(),
        )
        .unwrap();

        let msgs = project_context(&read_entries(w.path()).unwrap());
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].timestamp(), 1);
        assert_eq!(msgs[1].timestamp(), 2);
    }

    #[test]
    fn truncated_last_line_dropped() {
        let (dir, idgen) = setup("trunc");
        let mut w =
            SessionWriter::create(&dir, "D:/x", None, Arc::new(FixedClock::new(1)), idgen).unwrap();
        w.append_raw(kinds::MESSAGE, serde_json::json!({"role": "user"}))
            .unwrap();
        let path = w.path().to_path_buf();
        drop(w);
        let mut raw = std::fs::read_to_string(&path).unwrap();
        raw.push_str(
            r#"{"v":1,"id":"ZZ","ts":"t","parent_id":null,"type":"message","payload":{"rol"#,
        );
        std::fs::write(&path, raw).unwrap();
        let entries = read_entries(&path).unwrap();
        assert_eq!(entries.len(), 2); // header + 完整 message，半行被丢弃
    }

    #[test]
    fn mid_file_corruption_is_error() {
        let (dir, idgen) = setup("mid");
        let mut w =
            SessionWriter::create(&dir, "D:/x", None, Arc::new(FixedClock::new(1)), idgen).unwrap();
        w.append_raw(kinds::MESSAGE, serde_json::json!({"role": "user"}))
            .unwrap();
        w.append_raw(kinds::MESSAGE, serde_json::json!({"role": "user"}))
            .unwrap();
        let path = w.path().to_path_buf();
        drop(w);
        let raw = std::fs::read_to_string(&path).unwrap();
        let mut lines: Vec<String> = raw.lines().map(String::from).collect();
        lines.insert(1, "{corrupt".into());
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        let err = read_entries(&path).unwrap_err();
        assert!(matches!(err, SessionError::CorruptEntry { line: 2, .. }));
    }

    #[test]
    fn list_scans_headers() {
        let (dir, idgen) = setup("list");
        let mut w =
            SessionWriter::create(&dir, "D:/demo", None, Arc::new(FixedClock::new(1)), idgen)
                .unwrap();
        w.append_raw(
            kinds::MESSAGE,
            serde_json::to_value(user_text("first prompt here", 5)).unwrap(),
        )
        .unwrap();
        let infos = list_sessions(&dir);
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].cwd, "D:/demo");
        assert_eq!(infos[0].first_message, "first prompt here");
        assert_eq!(infos[0].message_count, 1);
    }

    #[test]
    fn list_scans_partitions_and_dedupes() {
        let (dir, idgen) = setup("list-partition");
        let partition = dir.join("--D--demo--");
        let mut w = SessionWriter::create(
            &partition,
            "D:/demo",
            None,
            Arc::new(FixedClock::new(1)),
            idgen,
        )
        .unwrap();
        w.append_raw(
            kinds::MESSAGE,
            serde_json::to_value(user_text("partition prompt", 5)).unwrap(),
        )
        .unwrap();
        let path = w.path().to_path_buf();
        drop(w);
        // 同一会话又出现在旧平铺位置：列表只保留一份
        std::fs::copy(&path, dir.join(path.file_name().unwrap())).unwrap();

        let infos = list_sessions(&dir);
        assert_eq!(infos.len(), 1);
        assert!(infos[0].path.starts_with(&partition));
    }

    #[test]
    fn export_serializes_the_leaf_chain() {
        let (dir, idgen) = setup("export");
        let mut w =
            SessionWriter::create(&dir, "D:/demo", None, Arc::new(FixedClock::new(7)), idgen)
                .unwrap();
        w.append_raw(
            kinds::MESSAGE,
            serde_json::to_value(user_text("one", 5)).unwrap(),
        )
        .unwrap();
        w.append_raw("custom.demo", serde_json::json!({"x": 1}))
            .unwrap();
        w.append_raw(
            kinds::MESSAGE,
            serde_json::to_value(user_text("two", 6)).unwrap(),
        )
        .unwrap();
        let entries = read_entries(w.path()).unwrap();
        let body = serialize_export(&entries).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), entries.len());
        let first: Entry = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first.kind, kinds::HEADER);
        let last: Entry = serde_json::from_str(lines[lines.len() - 1]).unwrap();
        assert_eq!(
            Some(last.id.as_str()),
            entries.last().map(|e| e.id.as_str())
        );
    }
}
