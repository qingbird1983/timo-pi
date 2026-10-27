//! 冒烟壳：仅验收载体，交互不打磨（docs/design/phase-1/06 §3）。
//!
//! stdout 只出流式正文、`[tool]` 起止行与 usage 摘要；错误走 stderr。
//! 薄壳只依赖 `timo-kernel` 门面（architecture/01 原则 2）。

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, CommandFactory, Parser, Subcommand};

mod commands;
mod ext_cmd;
mod json_mode;
mod print;
use commands::{CommandOutcome, WARNED, handle_command, print_resource_warnings};
use timo_kernel::{
    AssistantStreamEvent, Content, Kernel, KernelEvent, KernelOptions, Message, ThinkingLevel,
    Usage,
};

#[derive(Parser)]
#[command(name = "timo", about = "Timo-pi non-interactive smoke CLI", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Headless print mode (`-p`); ignored when a subcommand is used.
    #[command(flatten)]
    print: print::PrintArgs,
}

#[derive(Subcommand)]
enum Command {
    /// Run one prompt in a new session.
    Run(RunArgs),
    /// Resume a session (`last` = most recently modified).
    Resume(ResumeArgs),
    /// List sessions.
    Sessions(SessionsArgs),
    /// Manage extensions.
    Ext(ext_cmd::ExtArgs),
}

#[derive(Args)]
pub(crate) struct Common {
    /// Provider id (must be paired with --model).
    #[arg(long)]
    provider: Option<String>,
    /// Model id (must be paired with --provider).
    #[arg(long)]
    model: Option<String>,
    /// off|minimal|low|medium|high|xhigh|max.
    #[arg(long)]
    thinking: Option<String>,
    /// Explicit API key (otherwise env/provider config).
    #[arg(long = "api-key")]
    api_key: Option<String>,
    /// Session directory override.
    #[arg(long = "session-dir")]
    session_dir: Option<PathBuf>,
    /// Working directory for tools and project settings.
    #[arg(long)]
    cwd: Option<PathBuf>,
    /// Load an extension from a file or directory (repeatable).
    #[arg(long = "extension", value_name = "PATH")]
    extension: Vec<PathBuf>,
    /// Disable all extensions for this run.
    #[arg(long = "no-extensions")]
    no_extensions: bool,
    /// Extension flag value (`NAME=VALUE`; repeatable).
    #[arg(long = "ext-flag", value_name = "NAME=VALUE")]
    ext_flag: Vec<String>,
}

#[derive(Args)]
struct RunArgs {
    /// Prompt text.
    prompt: String,
    #[command(flatten)]
    common: Common,
    /// Maximum model-request turns in this run.
    #[arg(long = "max-turns")]
    max_turns: Option<usize>,
}

#[derive(Args)]
struct ResumeArgs {
    /// Session id or `last`.
    session: String,
    /// Run one extra prompt, then exit.
    #[arg(long)]
    once: Option<String>,
    #[command(flatten)]
    common: Common,
}

#[derive(Args)]
struct SessionsArgs {
    /// Only show sessions created in this working directory.
    #[arg(long)]
    cwd: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Some(_) if !cli.print.prompts.is_empty() => {
            eprintln!("[error] -p/--print cannot be combined with a subcommand");
            2
        }
        Some(command) => match command {
            Command::Run(args) => run_command(args).await,
            Command::Resume(args) => resume_command(args).await,
            Command::Sessions(args) => sessions_command(args),
            Command::Ext(args) => ext_cmd::run(args).await,
        },
        None if cli.print.prompts.is_empty() => {
            eprintln!("[error] nothing to run: supply -p <prompt> or a subcommand\n");
            eprint!("{}", Cli::command().render_help());
            2
        }
        None => print::run(cli.print).await,
    };
    std::process::exit(code);
}

async fn run_command(args: RunArgs) -> i32 {
    let options = match build_options(&args.common, args.max_turns) {
        Ok(o) => o,
        Err(message) => {
            eprintln!("[error] {message}");
            return 2;
        }
    };
    let mut kernel = match Kernel::new(options).await {
        Ok(k) => k,
        Err(e) => {
            eprintln!("[error] {e}");
            return 1;
        }
    };
    run_prompt(&mut kernel, &args.prompt).await
}

async fn resume_command(args: ResumeArgs) -> i32 {
    let options = match build_options(&args.common, None) {
        Ok(o) => o,
        Err(message) => {
            eprintln!("[error] {message}");
            return 2;
        }
    };
    let mut kernel = match Kernel::new(options).await {
        Ok(k) => k,
        Err(e) => {
            eprintln!("[error] {e}");
            return 1;
        }
    };
    let id = if args.session == "last" {
        match kernel.list_sessions().first() {
            Some(s) => s.id.clone(),
            None => {
                eprintln!("[error] no sessions found");
                return 1;
            }
        }
    } else {
        args.session.clone()
    };
    if let Err(e) = kernel.resume(&id).await {
        eprintln!("[error] {e}");
        return 1;
    }
    if let Some(once) = args.once {
        return run_prompt(&mut kernel, &once).await;
    }
    let mut lines = spawn_stdin_lines();
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                eprintln!("[exit] interrupt");
                return 130;
            }
            line = lines.recv() => {
                let Some(line) = line else {
                    break;
                };
                let text = line.trim();
                if text.is_empty() {
                    continue;
                }
                match handle_command(&mut kernel, text).await {
                    CommandOutcome::Handled => {}
                    CommandOutcome::Quit => break,
                    CommandOutcome::Prompt => {
                        let _ = run_prompt(&mut kernel, text).await;
                    }
                }
            }
        }
    }
    0
}

fn sessions_command(args: SessionsArgs) -> i32 {
    let dir = match Kernel::sessions_dir(None) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[error] {e}");
            return 1;
        }
    };
    let filter = args.cwd.as_ref().and_then(|p| p.canonicalize().ok());
    for s in Kernel::list_sessions_at(&dir) {
        if let Some(f) = &filter {
            let matches = Path::new(&s.cwd)
                .canonicalize()
                .map(|c| &c == f)
                .unwrap_or(false);
            if !matches {
                continue;
            }
        }
        println!(
            "{}  {}  {:>4} msgs  {}",
            s.id,
            format_ms(s.modified_ms),
            s.message_count,
            s.first_message
        );
    }
    0
}

/// 一次 prompt：并发消费事件（流式正文/工具行/usage），Ctrl+C 两段语义
/// （第一次 abort、第二次退出进程）。
pub(crate) async fn run_prompt(kernel: &mut Kernel, prompt: &str) -> i32 {
    // 先装配 agent（幂等），以便在 run 借出 &mut 期间仍能拿中止句柄。
    let _ = kernel.tools().await;
    if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        print_resource_warnings(kernel);
    }
    let abort = kernel.abort_handle();
    let ext_abort = kernel.abort_flag();
    let mut events = kernel.events();
    let mut printer = Printer::new();
    let outcome = {
        let run = kernel.run(prompt);
        tokio::pin!(run);
        let mut aborted = false;
        loop {
            tokio::select! {
                event = events.recv() => {
                    match event {
                        Ok(event) => printer.handle(&event),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {}
                    }
                }
                result = &mut run => break result,
                _ = tokio::signal::ctrl_c() => {
                    if aborted {
                        eprintln!("[exit] second interrupt");
                        std::process::exit(130);
                    }
                    ext_abort.store(true, std::sync::atomic::Ordering::SeqCst);
                    if let Some(handle) = &abort {
                        handle.abort();
                    }
                    eprintln!("[abort] cancelling current run (Ctrl+C again to exit)");
                    aborted = true;
                }
            }
        }
    };
    // run 返回时收尾事件已同步发出；清空队列，保证 usage 行不丢。
    loop {
        match events.try_recv() {
            Ok(event) => printer.handle(&event),
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
            Err(_) => break,
        }
    }
    let code = match outcome {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("[error] {e}");
            1
        }
    };
    if let Some(report) = kernel.extension_report()
        && report
            .extensions
            .iter()
            .any(|ext| !ext.degraded.is_empty() || !ext.errors.is_empty())
    {
        eprint!("{}", report.render_human());
    }
    code
}

pub(crate) fn build_options(
    common: &Common,
    max_turns: Option<usize>,
) -> Result<KernelOptions, String> {
    let model_override = match (&common.provider, &common.model) {
        (None, None) => None,
        (Some(p), Some(m)) => Some((p.clone(), m.clone())),
        _ => return Err("--provider and --model must be supplied together".into()),
    };
    let thinking = match &common.thinking {
        None => None,
        Some(s) => Some(ThinkingLevel::parse(s).ok_or_else(|| {
            format!("unknown thinking level `{s}` (off|minimal|low|medium|high|xhigh|max)")
        })?),
    };
    let mut ext_flags = Vec::new();
    for flag in &common.ext_flag {
        let (name, value) = flag
            .split_once('=')
            .ok_or_else(|| format!("--ext-flag expects NAME=VALUE, got `{flag}`"))?;
        ext_flags.push((name.to_string(), value.to_string()));
    }
    Ok(KernelOptions {
        data_dir: None,
        work_dir: common.cwd.clone(),
        model_override,
        thinking,
        api_key: common.api_key.clone(),
        session_dir: common.session_dir.clone(),
        max_turns,
        extension_paths: common.extension.clone(),
        no_extensions: common.no_extensions,
        extensions_interactive: None,
        ext_flags,
        tool_review: None,
    })
}

/// stdin 逐行读取（独立线程，避免阻塞事件循环；EOF 关闭通道）。
fn spawn_stdin_lines() -> tokio::sync::mpsc::UnboundedReceiver<String> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    rx
}

/// 事件渲染（stdout 正文与工具行；错误走 stderr）。
struct Printer {
    at_line_start: bool,
}

impl Printer {
    fn new() -> Self {
        Self {
            at_line_start: true,
        }
    }

    fn handle(&mut self, event: &KernelEvent) {
        match event {
            KernelEvent::StreamEvent { event } => self.stream(event),
            KernelEvent::ToolCallStarted {
                tool_name, args, ..
            } => {
                self.ensure_newline();
                let mut summary = args.to_string();
                if summary.chars().count() > 120 {
                    summary = summary.chars().take(119).chain(['…']).collect();
                }
                println!("[tool] {tool_name}({summary})");
                let _ = std::io::stdout().flush();
            }
            KernelEvent::ToolCallFinished {
                tool_name,
                result,
                duration_ms,
                ..
            } => {
                let failed = matches!(result.as_ref(), Message::ToolResult(r) if r.is_error);
                println!(
                    "[tool] {tool_name} {} ({} ms)",
                    if failed { "failed" } else { "done" },
                    duration_ms.unwrap_or(0)
                );
            }
            KernelEvent::Error { context } => {
                self.ensure_newline();
                eprintln!("[error] {context}");
            }
            KernelEvent::AgentFinished { messages } => {
                self.ensure_newline();
                if let Some(usage) = last_usage(messages) {
                    println!(
                        "[usage] input={} output={} cache_read={} cache_write={}",
                        usage.input, usage.output, usage.cache_read, usage.cache_write
                    );
                }
            }
            _ => {}
        }
    }

    fn stream(&mut self, event: &AssistantStreamEvent) {
        match event {
            AssistantStreamEvent::Delta {
                index,
                delta,
                partial,
            } => {
                if matches!(partial.content.get(*index), Some(Content::Text { .. })) {
                    print!("{delta}");
                    let _ = std::io::stdout().flush();
                    self.at_line_start = false;
                }
            }
            AssistantStreamEvent::BlockEnd { index, partial } => {
                if matches!(partial.content.get(*index), Some(Content::Text { .. })) {
                    self.ensure_newline();
                }
            }
            _ => {}
        }
    }

    fn ensure_newline(&mut self) {
        if !self.at_line_start {
            println!();
            self.at_line_start = true;
        }
    }
}

fn last_usage(messages: &[Message]) -> Option<&Usage> {
    messages.iter().rev().find_map(|m| match m {
        Message::Assistant(a) => Some(&a.usage),
        _ => None,
    })
}

/// unix 毫秒 → `YYYY-MM-DD HH:MM UTC`（不引入时间库）。
fn format_ms(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        rem / 3600,
        (rem % 3600) / 60
    )
}

/// Howard Hinnant 的 days → (year, month, day)（公历）。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 { y + 1 } else { y },
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn formats_epoch_in_utc() {
        assert_eq!(format_ms(0), "1970-01-01 00:00 UTC");
        assert_eq!(format_ms(1_700_000_000_000), "2023-11-14 22:13 UTC");
    }
}
