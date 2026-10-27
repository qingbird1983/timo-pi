//! 会话生命周期 API（resume / 草稿 / 历史 / 删除）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use timo_session::{
    SessionError, SessionInfo, SessionWriter, list_sessions, project_context, read_entries,
};

use super::Kernel;
use crate::Result;
use crate::error::KernelError;

impl Kernel {
    /// 计算会话目录（与 [`Kernel::new`] 同一推导；`sessions` 命令无需模型凭证）。
    ///
    /// # Errors
    /// 平台数据目录不可得 → [`KernelError::Config`]。
    pub fn sessions_dir(data_dir: Option<&Path>) -> Result<PathBuf> {
        match data_dir {
            Some(d) => Ok(d.join("sessions")),
            None => Ok(timo_config::sessions_dir(None)?),
        }
    }

    /// 扫描会话目录（不装配内核；修改时间倒序）。
    #[must_use]
    pub fn list_sessions_at(dir: &Path) -> Vec<SessionInfo> {
        list_sessions(dir)
    }
    /// 会话列表（修改时间倒序）。
    #[must_use]
    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        list_sessions(&self.sessions_dir)
    }

    /// 恢复会话（投影历史；后续 run 追加到该文件）。
    ///
    /// # Errors
    /// 会话不存在/损坏 → [`KernelError::Session`]。
    pub async fn resume(&mut self, id: &str) -> Result<()> {
        self.ensure_ready().await;
        let path = self.sessions_dir.join(format!("{id}.jsonl"));
        let entries = read_entries(&path)?;
        let writer = SessionWriter::open(
            &path,
            Arc::clone(&self.clock),
            Arc::new(timo_session::IdGen::new()),
        )?;
        self.session_id = Some(id.to_string());
        self.history = Some(project_context(&entries));
        self.session = Some(Arc::new(tokio::sync::Mutex::new(writer)));
        if let Some(host) = self.ext.as_ref().and_then(|runtime| runtime.host.as_ref()) {
            host.notify_event("session_start", serde_json::json!({ "reason": "resume" }));
        }
        Ok(())
    }

    /// 开始一个未落盘的会话草稿；首次 [`Kernel::run`] 才创建会话文件。
    ///
    /// 调用后当前会话 id 与历史都被清空（GUI“新建会话”）。
    pub fn new_session(&mut self) {
        self.session = None;
        self.session_id = None;
        self.history = None;
    }

    /// 当前会话的投影上下文；草稿或尚无历史时为 `None`。
    #[must_use]
    pub fn history(&self) -> Option<&[timo_ai::Message]> {
        self.history.as_deref()
    }

    /// 删除一个已落盘的会话文件；删除当前会话时回到草稿态。
    ///
    /// id 只接受会话 id 字符（ASCII 字母数字、`-`、`_`），防止路径穿越。
    ///
    /// # Errors
    /// id 非法、文件不存在或删除失败 → [`KernelError::Session`]。
    pub fn delete_session(&mut self, id: &str) -> Result<()> {
        let valid = !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !valid {
            return Err(KernelError::Session(SessionError::NotFound(id.to_string())));
        }
        let path = self.sessions_dir.join(format!("{id}.jsonl"));
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Err(KernelError::Session(SessionError::NotFound(id.to_string())));
            }
            Err(source) => {
                return Err(KernelError::Session(SessionError::Io { path, source }));
            }
        }
        if self.session_id.as_deref() == Some(id) {
            self.new_session();
        }
        Ok(())
    }
}
