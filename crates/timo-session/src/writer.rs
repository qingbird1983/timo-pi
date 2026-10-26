//! 会话写入器（设计 04 §4~5：append-only + 每行 flush）。

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use timo_config::Clock;

use crate::Result;
use crate::entry::{ENTRY_VERSION, Entry, SessionHeader, kinds};
use crate::error::SessionError;

/// ULID 生成器（进程内单调：同毫秒随机段自增，时钟回拨仍有序——设计 04 §6 Q1）。
#[derive(Debug, Default)]
pub struct IdGen {
    generator: std::sync::Mutex<ulid::Generator>,
}

impl IdGen {
    /// 新建生成器。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 产出下一个单调 ULID。
    ///
    /// # Errors
    /// 生成器饱和（理论不可能）时返回 [`SessionError::CorruptEntry`]。
    pub fn next(&self) -> Result<String> {
        let mut guard = self
            .generator
            .lock()
            .map_err(|_| SessionError::CorruptEntry {
                path: PathBuf::from("<idgen>"),
                line: 0,
                reason: "ulid generator poisoned".into(),
            })?;
        guard
            .generate()
            .map(|u| u.to_string())
            .map_err(|e| SessionError::CorruptEntry {
                path: PathBuf::from("<idgen>"),
                line: 0,
                reason: format!("ulid generator: {e}"),
            })
    }
}

/// 会话追加写入器：entry 流的唯一写入口。
pub struct SessionWriter {
    file: BufWriter<std::fs::File>,
    path: PathBuf,
    session_id: String,
    leaf: Option<String>,
    clock: Arc<dyn Clock>,
    id_gen: Arc<IdGen>,
}

impl SessionWriter {
    /// 新建会话：写 `session.header` 首条。
    ///
    /// # Errors
    /// 目录创建/文件写入失败 → [`SessionError::Io`]。
    pub fn create(
        dir: &Path,
        cwd: &str,
        initial_model: Option<&str>,
        clock: Arc<dyn Clock>,
        id_gen: Arc<IdGen>,
    ) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| SessionError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let session_id = id_gen.next()?;
        let path = dir.join(format!("{session_id}.jsonl"));
        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|e| SessionError::Io {
                path: path.clone(),
                source: e,
            })?;
        let mut w = Self {
            file: BufWriter::new(file),
            path,
            session_id: session_id.clone(),
            leaf: None,
            clock,
            id_gen,
        };
        let header = SessionHeader {
            format_version: ENTRY_VERSION,
            session_id,
            created_ms: w.clock.now_ms(),
            cwd: cwd.to_string(),
            parent_session_id: None,
            initial_model: initial_model.map(String::from),
        };
        w.append_raw(
            kinds::HEADER,
            serde_json::to_value(&header).unwrap_or(Value::Null),
        )?;
        Ok(w)
    }

    /// 打开既有会话续写（校验 header；leaf 定位到最后一条完整 entry）。
    ///
    /// # Errors
    /// 文件缺失/无 header → [`SessionError::InvalidFile`]；中间行损坏 →
    /// [`SessionError::CorruptEntry`]（末行截断容忍，见设计 04 §5）。
    pub fn open(path: &Path, clock: Arc<dyn Clock>, id_gen: Arc<IdGen>) -> Result<Self> {
        let entries = crate::reader::read_entries(path)?;
        let header = entries.first().ok_or_else(|| SessionError::InvalidFile {
            path: path.to_path_buf(),
            reason: "empty session file".into(),
        })?;
        if header.kind != kinds::HEADER {
            return Err(SessionError::InvalidFile {
                path: path.to_path_buf(),
                reason: format!("first entry is {}, not header", header.kind),
            });
        }
        let leaf = entries.last().map(|e| e.id.clone());
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(|e| SessionError::Io {
                path: path.to_path_buf(),
                source: e,
            })?;
        Ok(Self {
            file: BufWriter::new(file),
            path: path.to_path_buf(),
            session_id: header
                .payload
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            leaf,
            clock,
            id_gen,
        })
    }

    /// 追加一条指定类型的 entry（通用入口；便捷方法基于它）。
    ///
    /// # Errors
    /// 序列化/写入/flush 失败 → [`SessionError`]。
    pub fn append_raw(&mut self, kind: &str, payload: Value) -> Result<String> {
        let id = self.id_gen.next()?;
        let entry = Entry {
            v: ENTRY_VERSION,
            id: id.clone(),
            ts: Entry::format_ts(self.clock.now_ms())?,
            parent_id: self.leaf.clone(),
            kind: kind.to_string(),
            payload,
        };
        let mut line = serde_json::to_string(&entry).map_err(|e| SessionError::CorruptEntry {
            path: self.path.clone(),
            line: 0,
            reason: format!("serialize: {e}"),
        })?;
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .map_err(|e| SessionError::Io {
                path: self.path.clone(),
                source: e,
            })?;
        // 崩溃安全：每条 flush（进程被杀不丢已 flush 行；设计 04 §5）
        self.file.flush().map_err(|e| SessionError::Io {
            path: self.path.clone(),
            source: e,
        })?;
        self.leaf = Some(id.clone());
        Ok(id)
    }

    /// 会话 id。
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// 文件路径。
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 当前叶子 entry id（树形结构挂点）。
    #[must_use]
    pub fn leaf_id(&self) -> Option<&str> {
        self.leaf.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use timo_config::FixedClock;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("timo-sess-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn create_appends_parent_chain() {
        let clock = Arc::new(FixedClock::new(1_789_000_000_000));
        let idgen = Arc::new(IdGen::new());
        let mut w = SessionWriter::create(
            &dir("create"),
            "D:/demo",
            Some("anthropic/claude-x"),
            clock,
            idgen,
        )
        .expect("create");
        let id2 = w
            .append_raw(kinds::MESSAGE, serde_json::json!({"role": "user"}))
            .expect("append");
        assert_eq!(w.leaf_id(), Some(id2.as_str()));
        let first_line = std::fs::read_to_string(w.path()).expect("read");
        let first: Entry =
            serde_json::from_str(first_line.lines().next().expect("line")).expect("e");
        assert_eq!(first.kind, kinds::HEADER);
        assert_eq!(first.parent_id, None);
    }

    #[test]
    fn reopen_locates_leaf() {
        let d = dir("reopen");
        let clock = Arc::new(FixedClock::new(1));
        let idgen = Arc::new(IdGen::new());
        let mut w = SessionWriter::create(&d, "D:/demo", None, clock.clone(), idgen.clone())
            .expect("create");
        let sid = w.session_id().to_string();
        w.append_raw(kinds::MESSAGE, serde_json::json!({"n": 1}))
            .expect("a1");
        let last = w
            .append_raw(kinds::MESSAGE, serde_json::json!({"n": 2}))
            .expect("a2");
        drop(w);
        let w2 =
            SessionWriter::open(&d.join(format!("{sid}.jsonl")), clock, idgen).expect("reopen");
        assert_eq!(w2.leaf_id(), Some(last.as_str()));
    }
}
