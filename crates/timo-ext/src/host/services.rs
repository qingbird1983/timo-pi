//! 默认宿主服务：子进程、stdin 对话框、设置快照（设计 01 §5、03 §5）。

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value as JsonValue, json};

use crate::host::{ExecOutput, Grants, HostServices};

/// CLI/测试用的基础服务实现。
pub struct BasicServices {
    cwd: PathBuf,
    has_ui: bool,
    grants: Grants,
    settings: JsonValue,
    model: JsonValue,
    thinking: String,
    aborted: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

impl BasicServices {
    /// 构造（默认无 UI、全拒能力）。
    #[must_use]
    pub fn new(cwd: impl Into<PathBuf>, grants: Grants, settings: JsonValue) -> Self {
        Self {
            cwd: cwd.into(),
            has_ui: false,
            grants,
            settings,
            model: JsonValue::Null,
            thinking: "off".to_string(),
            aborted: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 开启 TTY 对话框（stdin 行交互）。
    #[must_use]
    pub fn with_ui(mut self, has_ui: bool) -> Self {
        self.has_ui = has_ui;
        self
    }

    /// 设置当前模型快照。
    #[must_use]
    pub fn with_model(mut self, model: JsonValue) -> Self {
        self.model = model;
        self
    }

    /// 取消标志（CLI 的 abort 与扩展共享）。
    #[must_use]
    pub fn abort_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.aborted)
    }
}

impl HostServices for BasicServices {
    fn exec(
        &self,
        command: &str,
        args: &[String],
        _options: &JsonValue,
    ) -> std::result::Result<ExecOutput, String> {
        if !self.grants.exec {
            return Err("E401 exec capability is not granted".to_string());
        }
        let output = std::process::Command::new(command)
            .args(args)
            .current_dir(&self.cwd)
            .output()
            .map_err(|error| format!("cannot run `{command}`: {error}"))?;
        Ok(ExecOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            code: output.status.code().unwrap_or(-1),
        })
    }

    fn notify(&self, message: &str, kind: &str) {
        eprintln!("[ext:{kind}] {message}");
    }

    fn select(&self, title: &str, options: &[String]) -> Option<String> {
        if !self.has_ui {
            return None;
        }
        eprintln!("{title}");
        for (index, option) in options.iter().enumerate() {
            eprintln!("  {}) {option}", index + 1);
        }
        eprint!("choice> ");
        let _ = io::stderr().flush();
        let line = read_line()?;
        let trimmed = line.trim();
        if let Ok(index) = trimmed.parse::<usize>()
            && index >= 1
            && index <= options.len()
        {
            return Some(options[index - 1].clone());
        }
        options
            .iter()
            .find(|option| option.as_str() == trimmed)
            .cloned()
    }

    fn confirm(&self, title: &str, message: &str) -> bool {
        if !self.has_ui {
            return false;
        }
        eprintln!("{title}\n{message}");
        eprint!("confirm [y/N]> ");
        let _ = io::stderr().flush();
        read_line()
            .map(|line| matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
            .unwrap_or(false)
    }

    fn input(&self, title: &str, placeholder: &str) -> Option<String> {
        if !self.has_ui {
            return None;
        }
        eprintln!("{title}");
        if !placeholder.is_empty() {
            eprintln!("({placeholder})");
        }
        eprint!("value> ");
        let _ = io::stderr().flush();
        let line = read_line()?;
        if line.trim().is_empty() {
            None
        } else {
            Some(line.trim_end().to_string())
        }
    }

    fn action(&self, name: &str, _payload: &JsonValue) -> JsonValue {
        match name {
            "getSettings" => self.settings.clone(),
            "getThinkingLevel" => json!(self.thinking),
            "getActiveTools" | "getAllTools" | "getCommands" => json!([]),
            "setModel" => json!(false),
            _ => JsonValue::Null,
        }
    }

    fn config(&self) -> JsonValue {
        json!({
            "hasUI": self.has_ui,
            "cwd": self.cwd.display().to_string(),
            "model": self.model,
            "thinkingLevel": self.thinking,
            "systemPrompt": "",
        })
    }

    fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
    }

    fn abort(&self) {
        self.aborted.store(true, Ordering::Relaxed);
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

fn read_line() -> Option<String> {
    let mut line = String::new();
    let read = io::stdin().read_line(&mut line).ok()?;
    if read == 0 {
        return None;
    }
    Some(line)
}

impl std::fmt::Debug for BasicServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BasicServices")
            .field("cwd", &self.cwd)
            .field("has_ui", &self.has_ui)
            .finish_non_exhaustive()
    }
}
