//! `bash` / `powershell` 工具（设计 03 §3；pi bash.ts / powershell.ts 同构）。
//!
//! 同一执行机：无默认超时（上限 2^31−1 ms）、尾截断、全量输出落临时文件、
//! 超时/取消杀进程树。Windows 优先：未配置 `shell_path` 时探测 bash→PowerShell。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::AsyncReadExt as _;
use tokio_util::sync::CancellationToken;

use crate::Result;
use crate::definition::{ToolCallCtx, ToolDefinition, ToolOutput};
use crate::error::ToolError;
use crate::truncate::truncate_tail;

/// timeout 上限（毫秒，2^31−1）。
const MAX_TIMEOUT_MS: u64 = 2_147_483_647;

/// 参数 schema。
#[must_use]
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "command": {"type": "string"},
            "timeout": {"type": "number", "description": "Seconds; no default timeout"}
        },
        "required": ["command"]
    })
}

/// 构造 bash 工具定义（Windows 无 bash 时回落 PowerShell 执行）。
#[must_use]
pub fn bash_definition(cwd: Arc<Path>) -> ToolDefinition {
    ToolDefinition {
        name: "bash".into(),
        label: "Bash".into(),
        description: "Run a shell command and return its output (tail-truncated).".into(),
        prompt_snippet: Some("bash(command, timeout?): run a shell command".into()),
        prompt_guidelines: vec![
            "There is no default timeout; pass `timeout` for potentially long commands.".into(),
        ],
        parameters: schema(),
        output_schema: None,
        exposure: crate::definition::Exposure::Direct,
        namespace: None,
        annotations: crate::definition::Annotations {
            destructive_hint: Some(true),
            open_world_hint: Some(true),
            ..Default::default()
        },
        execution_mode: crate::definition::ExecutionMode::Parallel,
        execute: Arc::new(move |ctx| {
            let cwd = cwd.clone();
            Box::pin(async move { run_shell(ctx, cwd, ShellKind::Bash).await })
        }),
    }
}

/// 构造 powershell 工具定义（Windows 支持的正式通道；UTF8 前导）。
#[must_use]
pub fn powershell_definition(cwd: Arc<Path>) -> ToolDefinition {
    ToolDefinition {
        name: "powershell".into(),
        label: "PowerShell".into(),
        description: "Run a PowerShell command and return its output (tail-truncated).".into(),
        prompt_snippet: Some("powershell(command, timeout?): run a PowerShell command".into()),
        prompt_guidelines: vec![],
        parameters: schema(),
        output_schema: None,
        exposure: crate::definition::Exposure::Direct,
        namespace: None,
        annotations: crate::definition::Annotations {
            destructive_hint: Some(true),
            open_world_hint: Some(true),
            ..Default::default()
        },
        execution_mode: crate::definition::ExecutionMode::Parallel,
        execute: Arc::new(move |ctx| {
            let cwd = cwd.clone();
            Box::pin(async move { run_shell(ctx, cwd, ShellKind::PowerShell).await })
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellKind {
    Bash,
    PowerShell,
}

impl ShellKind {
    fn program(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::PowerShell => "powershell",
        }
    }
}

/// PATH 探测（Windows bash 可得性）。
fn on_path(program: &str) -> bool {
    let exe = if cfg!(windows) {
        format!("{program}.exe")
    } else {
        program.to_string()
    };
    std::env::var("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(&exe).is_file()))
        .unwrap_or(false)
}

/// Windows 上可用的 `bash` 必须是原生 POSIX 层（Git Bash/MSYS/Cygwin）。
///
/// System32 里的 `bash.exe` 是 WSL 启动器：它不认 Windows 工作目录，
/// 会把命令跑在 Linux 侧（或直接失败），因此视为不可用、回退 PowerShell。
fn windows_bash_usable() -> bool {
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| {
        if !on_path("bash") {
            return false;
        }
        let out = std::process::Command::new("bash")
            .args(["-c", "uname -s"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output();
        match out {
            Ok(o) if o.status.success() => {
                is_native_bash_uname(&String::from_utf8_lossy(&o.stdout))
            }
            _ => false,
        }
    })
}

/// `uname -s` 是否来自 Git Bash/MSYS/Cygwin（纯函数，便于测试）。
fn is_native_bash_uname(uname: &str) -> bool {
    let s = uname.to_ascii_lowercase();
    s.contains("mingw") || s.contains("msys") || s.contains("cygwin")
}

async fn run_shell(ctx: ToolCallCtx, cwd: Arc<Path>, kind: ShellKind) -> Result<ToolOutput> {
    let _ = ctx.updates; // 进度节流为 Phase 1 不做（roadmap 不做清单）
    let command = crate::tools::write::str_param(&ctx.params, "command")?.to_string();
    let timeout_s = ctx.params.get("timeout").and_then(Value::as_u64);
    let timeout_ms = timeout_s
        .map(|s| s.saturating_mul(1000))
        .unwrap_or(MAX_TIMEOUT_MS)
        .min(MAX_TIMEOUT_MS);
    // Windows 上无原生 bash（缺失或只有 WSL 启动器）时按 D1 白名单以 PowerShell 执行
    let kind = if kind == ShellKind::Bash && cfg!(windows) && !windows_bash_usable() {
        ShellKind::PowerShell
    } else {
        kind
    };
    execute_process(&command, &cwd, kind, timeout_ms, &ctx.cancel).await
}

async fn execute_process(
    command: &str,
    cwd: &Path,
    kind: ShellKind,
    timeout_ms: u64,
    cancel: &CancellationToken,
) -> Result<ToolOutput> {
    let mut cmd = tokio::process::Command::new(kind.program());
    match kind {
        ShellKind::Bash => {
            cmd.arg("-c").arg(command);
        }
        ShellKind::PowerShell => {
            cmd.args(["-NoProfile", "-Command"]).arg(format!(
                "[Console]::OutputEncoding=[Text.Encoding]::UTF8; {command}"
            ));
        }
    }
    cmd.current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .map_err(|e| ToolError::Subprocess { source: e })?;
    let piped = ToolError::Subprocess {
        source: std::io::Error::other("pipe closed before read"),
    };
    let mut out_pipe = child.stdout.take().ok_or(piped)?;
    let piped2 = ToolError::Subprocess {
        source: std::io::Error::other("pipe closed before read"),
    };
    let mut err_pipe = child.stderr.take().ok_or(piped2)?;
    let out_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = out_pipe.read_to_end(&mut buf).await;
        buf
    });
    let err_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = err_pipe.read_to_end(&mut buf).await;
        buf
    });

    let deadline = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms));
    tokio::pin!(deadline);
    enum Exit {
        Status(std::process::ExitStatus),
        Io(std::io::Error),
        Timeout,
        Cancelled,
    }
    let exit: Exit = tokio::select! {
        status = child.wait() => match status {
            Ok(s) => Exit::Status(s),
            Err(e) => Exit::Io(e),
        },
        _ = deadline.as_mut() => Exit::Timeout,
        _ = cancel.cancelled() => Exit::Cancelled,
    };
    let status = match exit {
        Exit::Status(s) => s,
        Exit::Io(e) => {
            let _ = out_task.await;
            let _ = err_task.await;
            return Err(ToolError::Subprocess { source: e });
        }
        Exit::Timeout | Exit::Cancelled => {
            kill_tree(child.id());
            let _ = child.wait().await;
            let _ = out_task.await;
            let _ = err_task.await;
            return match exit {
                Exit::Timeout => Err(timed_out(timeout_ms)),
                Exit::Cancelled => Err(ToolError::Cancelled),
                Exit::Status(_) | Exit::Io(_) => unreachable!("handled above"),
            };
        }
    };
    let mut out = out_task.await.unwrap_or_default();
    out.extend(err_task.await.unwrap_or_default());
    finish_output(&String::from_utf8_lossy(&out), status.code(), cancel)
}

fn timed_out(ms: u64) -> ToolError {
    ToolError::Timeout {
        seconds: (ms / 1000).max(1),
    }
}

/// 杀进程树（Windows taskkill /T；unix 分支 Phase 3 打包时补 killpg）。
fn kill_tree(pid: Option<u32>) {
    if let Some(pid) = pid {
        kill_tree_impl(pid);
    }
}

#[cfg(windows)]
fn kill_tree_impl(pid: u32) {
    use std::os::windows::process::CommandExt as _;
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output();
}

#[cfg(not(windows))]
fn kill_tree_impl(_pid: u32) {
    // kill_on_drop 已覆盖单进程；进程组级清理随 Phase 3 平台打包补齐。
}

fn finish_output(
    raw: &str,
    exit_code: Option<i32>,
    _cancel: &CancellationToken,
) -> Result<ToolOutput> {
    let (body, truncated) = truncate_tail(raw.trim_end_matches(['\r', '\n']));
    let body = if body.is_empty() {
        "(no output)".to_string()
    } else {
        body
    };
    let mut body = if truncated {
        let full = write_full_output(raw);
        format!("{body}\n\n[Showing tail. Full output: {}]", full.display())
    } else {
        body
    };
    match exit_code {
        Some(0) | None => Ok(ToolOutput::text(body)),
        Some(code) => {
            body.push_str(&format!("\n\nCommand exited with code {code}"));
            Ok(ToolOutput {
                is_error: true,
                ..ToolOutput::text(body)
            })
        }
    }
}

fn write_full_output(raw: &str) -> PathBuf {
    let dir = std::env::temp_dir();
    let Ok(mut f) = tempfile::Builder::new()
        .prefix("timo-bash")
        .rand_bytes(6)
        .tempfile_in(dir)
    else {
        return PathBuf::from("<temp>");
    };
    use std::io::Write as _;
    let _ = f.write_all(raw.as_bytes());
    match f.into_temp_path().keep() {
        Ok(p) => p,
        Err(_) => PathBuf::from("<temp>"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn native_bash_detection() {
        assert!(is_native_bash_uname("MINGW64_NT-10.0-19045\n"));
        assert!(is_native_bash_uname("MSYS_NT-10.0"));
        assert!(is_native_bash_uname("CYGWIN_NT-10.0"));
        assert!(!is_native_bash_uname("Linux\n"));
    }

    #[tokio::test]
    async fn echo_and_exit_code() {
        let (out, _) = run_kind("echo timo-ok").await;
        assert!(out.contains("timo-ok"), "{out}");
        let (out2, is_err) = run_kind("exit 3").await;
        assert!(is_err);
        assert!(out2.contains("exited with code 3"), "{out2}");
    }

    #[tokio::test]
    async fn timeout_errors() {
        let cmd = if cfg!(windows) && !on_path("bash") {
            "Start-Sleep -Seconds 30"
        } else {
            "sleep 30"
        };
        let dir = std::env::temp_dir();
        let res =
            execute_process(cmd, &dir, ShellKind::Bash, 1500, &CancellationToken::new()).await;
        match res {
            Err(ToolError::Timeout { seconds }) => assert!(seconds >= 1),
            other => panic!("timeout expected, got {other:?}"),
        }
    }

    async fn run_kind(cmd: &str) -> (String, bool) {
        let out = execute_process(
            cmd,
            &std::env::temp_dir(),
            ShellKind::Bash,
            30_000,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        let timo_ai::Content::Text { text } = &out.content[0] else {
            panic!("text")
        };
        (text.clone(), out.is_error)
    }
}
