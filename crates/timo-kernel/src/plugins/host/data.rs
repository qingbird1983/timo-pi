//! 只读数据快照（设计 04 §2）：会话元数据与模型目录，全部脱敏后交给插件。
//!
//! 不返回消息正文、绝对路径、系统提示或密钥；`cwdLabel` 只保留最后一级目录名。

use std::path::Path;

use serde::Serialize;
use timo_ai::Catalog;
use timo_session::SessionInfo;

/// 列表行：会话元数据（无正文）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    /// 会话 id。
    pub id: String,
    /// 首条用户消息的首行（≤ 80 字符），可为空。
    pub title: String,
    /// 最后修改时间（unix 毫秒）。
    pub updated_at: i64,
    /// 工作目录最后一级名称。
    pub cwd_label: String,
    /// message entry 数。
    pub message_count: usize,
}

/// 模型目录行（不含密钥与端点）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSummary {
    /// 供应商 id。
    pub provider: String,
    /// 模型 id。
    pub id: String,
    /// 展示名。
    pub label: String,
    /// 上下文窗口（token）。
    pub context_window: u64,
}

/// 按修改时间倒序分页（调用方已校验 `limit`/`offset`）。
#[must_use]
pub fn list_sessions(dir: &Path, limit: usize, offset: usize) -> Vec<SessionSummary> {
    timo_session::list_sessions(dir)
        .iter()
        .skip(offset)
        .take(limit)
        .map(summary)
        .collect()
}

/// 按 id、标题或目录名做大小写不敏感匹配。
#[must_use]
pub fn search_sessions(dir: &Path, query: &str, limit: usize) -> Vec<SessionSummary> {
    let needle = query.to_lowercase();
    timo_session::list_sessions(dir)
        .iter()
        .filter(|info| {
            summary(info).id.to_lowercase().contains(&needle)
                || title_of(info).to_lowercase().contains(&needle)
                || cwd_label(&info.cwd).to_lowercase().contains(&needle)
        })
        .take(limit)
        .map(summary)
        .collect()
}

/// 单个会话的元数据；不存在返回 `None`。
#[must_use]
pub fn get_session(dir: &Path, id: &str) -> Option<SessionSummary> {
    timo_session::list_sessions(dir)
        .iter()
        .find(|info| info.id == id)
        .map(summary)
}

/// 目录快照，保持目录序。
#[must_use]
pub fn list_models(catalog: &Catalog) -> Vec<ModelSummary> {
    catalog
        .all()
        .into_iter()
        .map(|model| ModelSummary {
            provider: model.provider.clone(),
            id: model.id.clone(),
            label: model.name.clone(),
            context_window: model.context_window,
        })
        .collect()
}

fn summary(info: &SessionInfo) -> SessionSummary {
    SessionSummary {
        id: info.id.clone(),
        title: title_of(info),
        updated_at: info.modified_ms,
        cwd_label: cwd_label(&info.cwd),
        message_count: info.message_count,
    }
}

fn title_of(info: &SessionInfo) -> String {
    info.first_message
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .chars()
        .take(80)
        .collect()
}

fn cwd_label(cwd: &str) -> String {
    cwd.split(['/', '\\'])
        .rev()
        .find(|part| !part.is_empty())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use serde_json::Value;
    use timo_ai::Catalog;
    use timo_session::{Entry, SessionHeader, kinds};

    use super::*;

    fn line(id: &str, parent: Option<&str>, kind: &str, payload: Value) -> String {
        serde_json::to_string(&Entry {
            v: 1,
            id: id.to_string(),
            ts: "2026-01-01T00:00:00.000Z".to_string(),
            parent_id: parent.map(str::to_string),
            kind: kind.to_string(),
            payload,
        })
        .unwrap()
    }

    fn session(
        dir: &Path,
        id: &str,
        cwd: &str,
        text: &str,
        message_count: usize,
    ) -> std::path::PathBuf {
        let header = SessionHeader {
            format_version: 1,
            session_id: id.to_string(),
            created_ms: 1_700_000_000_000,
            cwd: cwd.to_string(),
            parent_session_id: None,
            initial_model: None,
        };
        let mut body = line(
            "01A",
            None,
            kinds::HEADER,
            serde_json::to_value(header).unwrap(),
        );
        body.push('\n');
        let mut parent = "01A".to_string();
        for index in 0..message_count {
            let entry_id = format!("01B{index}");
            let message = timo_ai::user_text(&format!("{text} #{index}"), 1_700_000_000_000);
            body.push_str(&line(
                &entry_id,
                Some(&parent),
                kinds::MESSAGE,
                serde_json::to_value(message).unwrap(),
            ));
            body.push('\n');
            parent = entry_id;
        }
        let path = dir.join(format!("{id}.jsonl"));
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn summaries_hide_paths_and_bodies() {
        let dir = tempfile::tempdir().unwrap();
        session(dir.path(), "01A", "C:\\work\\timo", "Fix the   parser", 3);
        let rows = list_sessions(dir.path(), 10, 0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "01A");
        assert_eq!(rows[0].title, "Fix the   parser #0");
        assert_eq!(rows[0].cwd_label, "timo");
        assert_eq!(rows[0].message_count, 3);
        let json = serde_json::to_value(&rows[0]).unwrap();
        assert!(json.get("cwdLabel").is_some());
        assert!(json.get("path").is_none());
        assert!(!json.to_string().contains("work"));
    }

    #[test]
    fn paging_and_search_are_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        session(dir.path(), "01A", "/work/alpha", "Parser bug", 1);
        session(dir.path(), "01B", "/work/beta", "Renderer polish", 2);
        assert_eq!(list_sessions(dir.path(), 1, 1).len(), 1);
        assert_eq!(search_sessions(dir.path(), "PARSER", 10)[0].id, "01A");
        assert_eq!(search_sessions(dir.path(), "beta", 10)[0].id, "01B");
        assert!(search_sessions(dir.path(), "missing", 10).is_empty());
        let found = get_session(dir.path(), "01B").unwrap();
        assert_eq!(found.cwd_label, "beta");
        assert!(get_session(dir.path(), "nope").is_none());
    }

    #[test]
    fn models_expose_no_endpoints_or_headers() {
        let rows = list_models(&Catalog::builtin());
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|row| !row.provider.is_empty()));
        let json = serde_json::to_value(&rows[0]).unwrap();
        assert!(json.get("contextWindow").is_some());
        assert!(json.get("baseUrl").is_none());
        assert!(json.get("headers").is_none());
        assert!(list_models(&Catalog::default()).is_empty());
    }
}
