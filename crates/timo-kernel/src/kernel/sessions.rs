//! 会话生命周期 API（resume / 草稿 / 历史 / 删除）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use timo_session::{
    SessionError, SessionInfo, SessionWriter, list_sessions, project_context, read_entries,
    serialize_export,
};

use super::Kernel;
use crate::Result;
use crate::error::KernelError;

fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl Kernel {
    /// 查找会话文件：旧平铺 → 当前项目分区 → 其他一级分区（phase-3.5/07）。
    pub(super) fn find_session_path(&self, id: &str) -> Option<PathBuf> {
        if !valid_session_id(id) {
            return None;
        }
        let name = format!("{id}.jsonl");
        let direct = self.sessions_dir.join(&name);
        if direct.is_file() {
            return Some(direct);
        }
        let partition =
            timo_config::session_dir_for(&self.sessions_dir, &self.work_dir).join(&name);
        if partition.is_file() {
            return Some(partition);
        }
        let entries = std::fs::read_dir(&self.sessions_dir).ok()?;
        for item in entries.flatten() {
            if !item.path().is_dir() {
                continue;
            }
            let candidate = item.path().join(&name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }
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
        let path = self
            .find_session_path(id)
            .ok_or_else(|| KernelError::Session(SessionError::NotFound(id.to_string())))?;
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
        let path = self
            .find_session_path(id)
            .ok_or_else(|| KernelError::Session(SessionError::NotFound(id.to_string())))?;
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

    /// 导出当前会话的 leaf 链为 JSONL（phase-3.5/07）。
    ///
    /// `path` 为空时写到 `<work_dir>/session-YYYYMMDD-HHMMSS.jsonl`；同名文件不覆盖。
    ///
    /// # Errors
    /// 草稿会话、读取/写入失败、目标已存在 → [`KernelError::Session`]。
    pub fn export_session(&self, path: Option<&Path>) -> Result<PathBuf> {
        let id = self
            .session_id
            .clone()
            .ok_or_else(|| KernelError::Session(SessionError::NotFound("<draft>".into())))?;
        let source = self
            .find_session_path(&id)
            .ok_or_else(|| KernelError::Session(SessionError::NotFound(id.clone())))?;
        let body = serialize_export(&read_entries(&source)?)?;
        let target = match path {
            Some(path) => path.to_path_buf(),
            None => self.default_export_path()?,
        };
        if target.exists() {
            return Err(KernelError::Session(SessionError::InvalidFile {
                path: target,
                reason: "export target already exists".into(),
            }));
        }
        let tmp = target.with_extension("jsonl.tmp");
        std::fs::write(&tmp, body).map_err(|source| SessionError::Io {
            path: tmp.clone(),
            source,
        })?;
        if let Err(source) = std::fs::rename(&tmp, &target) {
            let _ = std::fs::remove_file(&tmp);
            return Err(KernelError::Session(SessionError::Io {
                path: target,
                source,
            }));
        }
        Ok(target)
    }

    fn default_export_path(&self) -> Result<PathBuf> {
        let ts = timo_session::Entry::format_ts(self.clock.now_ms())?;
        let digits: String = ts.chars().filter(char::is_ascii_digit).collect();
        let stem = format!("session-{}-{}", &digits[..8], &digits[8..14]);
        let mut candidate = self.work_dir.join(format!("{stem}.jsonl"));
        for suffix in 1..=9 {
            if !candidate.exists() {
                return Ok(candidate);
            }
            candidate = self.work_dir.join(format!("{stem}-{suffix}.jsonl"));
        }
        Ok(candidate)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::kernel::KernelOptions;
    use timo_ai::user_text;
    use timo_config::FixedClock;

    async fn kernel_at(data: &Path) -> Kernel {
        let work = data.join("work");
        std::fs::create_dir_all(&work).unwrap();
        Kernel::new(KernelOptions {
            data_dir: Some(data.to_path_buf()),
            work_dir: Some(work),
            model_override: Some(("anthropic".into(), "claude-sonnet-4-5".into())),
            api_key: Some("test-key".into()),
            ..Default::default()
        })
        .await
        .expect("assemble")
    }

    #[tokio::test]
    async fn sessions_live_in_the_cwd_partition_and_export() {
        let data = std::env::temp_dir().join(format!("timo-kernel-sess-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let work = data.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let mut kernel = kernel_at(&data).await;

        let dir = timo_config::session_dir_for(&data.join("sessions"), &work);
        let mut writer = SessionWriter::create(
            &dir,
            &work.display().to_string(),
            None,
            Arc::new(FixedClock::new(3)),
            Arc::new(timo_session::IdGen::new()),
        )
        .unwrap();
        let id = writer.session_id().to_string();
        writer
            .append_raw(
                timo_session::kinds::MESSAGE,
                serde_json::to_value(user_text("hello", 5)).unwrap(),
            )
            .unwrap();
        let mut assistant = timo_ai::AssistantMessage::pending(
            "openai-completions",
            "anthropic",
            "claude-sonnet-4-5",
            6,
        );
        assistant.stop_reason = timo_ai::StopReason::Stop;
        assistant.content = vec![timo_ai::Content::Text { text: "hi".into() }];
        assistant.usage.input = 7;
        assistant.usage.output = 3;
        assistant.usage.total_tokens = 10;
        writer
            .append_raw(
                timo_session::kinds::MESSAGE,
                serde_json::to_value(timo_ai::Message::Assistant(assistant)).unwrap(),
            )
            .unwrap();
        drop(writer);

        assert_eq!(kernel.list_sessions().len(), 1);
        kernel.resume(&id).await.unwrap();
        assert_eq!(kernel.history().unwrap().len(), 2);
        assert_eq!(kernel.usage_totals().input, 7);

        let target = data.join("out.jsonl");
        assert_eq!(kernel.export_session(Some(&target)).unwrap(), target);
        let text = std::fs::read_to_string(&target).unwrap();
        assert_eq!(text.lines().count(), 3);
        assert!(kernel.export_session(Some(&target)).is_err());

        let default = kernel.export_session(None).unwrap();
        assert!(default.exists());
        assert!(
            default
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("session-")
        );

        let _ = std::fs::remove_dir_all(&data);
    }
}
