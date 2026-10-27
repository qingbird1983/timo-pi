//! 默认宿主服务：子进程、stdin 对话框、设置快照（设计 01 §5、03 §5）。

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value as JsonValue, json};

use crate::host::{DirItem, ExecOutput, ExecRequest, FetchOutput, Grants, HostServices};

/// `pi.execParallel` 的单批上限（设计 08 §4.4，与 pi parallel 模式同值）。
pub const MAX_PARALLEL_REQUESTS: usize = 8;
/// `pi.execParallel` 的并发上限。
pub const MAX_PARALLEL_CONCURRENCY: usize = 4;
/// kill 之后等管道收尾的宽限（孙进程可能长期握着写端，不能无限等）。
const PIPE_DRAIN_GRACE: Duration = Duration::from_millis(800);

/// 文件访问失败的统一文案（非安全事件，`E303`）。
fn io_error(path: &str, reason: &str) -> String {
    format!("E303 cannot access `{path}`: {reason}")
}

/// 单独跑一条子进程：取消或超时 kill，输出仍按已读到的部分返回。
fn run_one(cwd: &Path, request: &ExecRequest, aborted: &AtomicBool) -> ExecOutput {
    let mut command = std::process::Command::new(&request.command);
    command
        .args(&request.args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ExecOutput {
                stdout: String::new(),
                stderr: format!("cannot run `{}`: {error}", request.command),
                code: -1,
            };
        }
    };
    // 先起读线程：管道写满会让子进程阻塞，轮询等待期间必须持续消费。
    let stdout = child.stdout.take().map(read_pipe);
    let stderr = child.stderr.take().map(read_pipe);
    let deadline = request
        .timeout_ms
        .map(|ms| Instant::now() + Duration::from_millis(ms));
    let mut code = -1;
    let mut note = String::new();
    loop {
        if aborted.load(Ordering::SeqCst) {
            let _ = child.kill();
            note = "cancelled by host".to_string();
            break;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                code = status.code().unwrap_or(-1);
                break;
            }
            Ok(None) => {
                if deadline.is_some_and(|at| Instant::now() >= at) {
                    let _ = child.kill();
                    note = "timeout".to_string();
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                let _ = child.kill();
                note = format!("wait failed: {error}");
                break;
            }
        }
    }
    // kill 路径也要 reap 直接子进程，否则它变成僵尸且孙进程更晚才被清理。
    let _ = child.wait();
    let stdout = join_pipe(stdout);
    let mut stderr = join_pipe(stderr);
    if !note.is_empty() {
        if !stderr.is_empty() {
            stderr.push('\n');
        }
        stderr.push_str(&note);
    }
    ExecOutput {
        stdout,
        stderr,
        code,
    }
}

fn read_pipe<R: Read + Send + 'static>(pipe: R) -> std::sync::mpsc::Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = pipe
            .take(crate::fs_scope::READ_CAP_BYTES as u64)
            .read_to_end(&mut buffer);
        let _ = tx.send(String::from_utf8_lossy(&buffer).into_owned());
    });
    rx
}

/// 取管道读数。被 kill 的子进程可能留下孙进程仍握着管道写端，`read_to_end` 等不到 EOF，
/// 因此读取本身有界：超时即按"已读到的部分"返回，不让 kill 路径变成阻塞等待。
fn join_pipe(receiver: Option<std::sync::mpsc::Receiver<String>>) -> String {
    receiver
        .and_then(|rx| rx.recv_timeout(PIPE_DRAIN_GRACE).ok())
        .unwrap_or_default()
}

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
    flags: std::collections::BTreeMap<String, String>,
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
            flags: std::collections::BTreeMap::new(),
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

    /// 注入共享取消标志（内核 run 与所有扩展共用一个 epoch）。
    #[must_use]
    pub fn with_abort_flag(mut self, flag: Arc<AtomicBool>) -> Self {
        self.aborted = flag;
        self
    }

    /// 设置 run 是否进行中（`ctx.isIdle`）。
    #[must_use]
    pub fn with_running(mut self, running: Arc<AtomicBool>) -> Self {
        self.running = running;
        self
    }

    /// 注入 `--ext-flag NAME=VALUE`（`pi.getFlag`）。
    #[must_use]
    pub fn with_flags(mut self, flags: std::collections::BTreeMap<String, String>) -> Self {
        self.flags = flags;
        self
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

    fn read_file(&self, path: &str) -> std::result::Result<String, String> {
        let absolute = crate::fs_scope::check_read(&self.cwd, &self.grants.fs_read, path)?;
        let mut file =
            std::fs::File::open(&absolute).map_err(|error| io_error(path, &error.to_string()))?;
        let mut buffer = Vec::new();
        std::io::Read::read_to_end(
            &mut std::io::Read::take(&mut file, crate::fs_scope::READ_CAP_BYTES as u64 + 1),
            &mut buffer,
        )
        .map_err(|error| io_error(path, &error.to_string()))?;
        let text = String::from_utf8_lossy(&buffer).into_owned();
        Ok(crate::fs_scope::cap_text(&text))
    }

    fn list_dir(&self, path: &str) -> std::result::Result<Vec<DirItem>, String> {
        let absolute = crate::fs_scope::check_read(&self.cwd, &self.grants.fs_read, path)?;
        let entries =
            std::fs::read_dir(&absolute).map_err(|error| io_error(path, &error.to_string()))?;
        let mut items = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| io_error(path, &error.to_string()))?;
            items.push(DirItem {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: entry.path().is_dir(),
            });
        }
        items.sort_by_key(|a| a.name.to_lowercase());
        Ok(items)
    }

    fn exec_parallel(
        &self,
        requests: Vec<ExecRequest>,
    ) -> std::result::Result<Vec<ExecOutput>, String> {
        if !self.grants.exec {
            return Err("E401 exec capability is not granted".to_string());
        }
        if requests.len() > MAX_PARALLEL_REQUESTS {
            return Err(format!(
                "too many parallel commands (max {MAX_PARALLEL_REQUESTS})"
            ));
        }
        let mut outputs = Vec::with_capacity(requests.len());
        for chunk in requests.chunks(MAX_PARALLEL_CONCURRENCY) {
            let handles: Vec<_> = chunk
                .iter()
                .map(|request| {
                    let cwd = self.cwd.clone();
                    let aborted = Arc::clone(&self.aborted);
                    let request = request.clone();
                    std::thread::spawn(move || run_one(&cwd, &request, &aborted))
                })
                .collect();
            for handle in handles {
                outputs.push(handle.join().unwrap_or_else(|_| ExecOutput {
                    stdout: String::new(),
                    stderr: "worker panicked".to_string(),
                    code: -1,
                }));
            }
        }
        Ok(outputs)
    }

    fn notify(&self, message: &str, kind: &str) {
        eprintln!("[ext:{kind}] {message}");
    }

    fn fetch(&self, url: &str) -> std::result::Result<FetchOutput, String> {
        let parsed =
            reqwest::Url::parse(url).map_err(|error| format!("E401 invalid URL: {error}"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(format!("E401 scheme not allowed: {}", parsed.scheme()));
        }
        let host = parsed
            .host_str()
            .ok_or_else(|| "E401 URL has no host".to_string())?
            .to_string();
        let port = parsed.port_or_known_default().unwrap_or(0);
        if !crate::permissions::domain_allowed(&self.grants.net_domains, &host, port) {
            return Err(format!("E402 net denied: {host}"));
        }
        let patterns = self.grants.net_domains.clone();
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                let url = attempt.url();
                let host = url.host_str().unwrap_or_default().to_string();
                let port = url.port_or_known_default().unwrap_or(0);
                if attempt.previous().len() >= 5 {
                    attempt.error("too many redirects")
                } else if crate::permissions::domain_allowed(&patterns, &host, port) {
                    attempt.follow()
                } else {
                    attempt.error(format!("E402 redirect denied: {host}"))
                }
            }))
            .build()
            .map_err(|error| format!("E401 http client: {error}"))?;
        let response = client
            .get(parsed)
            .send()
            .map_err(|error| format!("E402 fetch failed: {error}"))?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_string(),
                    value.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        let body = response
            .text()
            .map_err(|error| format!("E402 read body: {error}"))?;
        Ok(FetchOutput {
            status,
            headers,
            body,
        })
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
            "flags": self.flags,
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
