//! 冒烟壳：仅验收载体，交互不打磨（docs/design/phase-1/06 §3）。
//!
//! stdout 只出流式正文、`[tool]` 起止行与 usage 摘要；错误走 stderr。
//! 薄壳只依赖 `timo-kernel` 门面（architecture/01 原则 2）。

use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, Subcommand};

mod commands;
mod ext_cmd;
mod json_mode;
mod print;
mod printer;
mod sessions_cmd;
use commands::{CommandOutcome, WARNED, handle_command, print_resource_warnings};
use printer::Printer;
use timo_kernel::{Kernel, KernelOptions, ThinkingLevel};

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
    Sessions(sessions_cmd::SessionsArgs),
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
    /// Do not persist a session for this run (ephemeral; subagents).
    #[arg(long = "no-session")]
    no_session: bool,
    /// Comma-separated allowlist of tool names or `*` patterns to enable for this run.
    #[arg(long = "tools", value_name = "TOOLS", value_delimiter = ',')]
    tools: Vec<String>,
    /// Append text, or the contents of `@path`, to the system prompt (repeatable).
    #[arg(long = "append-system-prompt", value_name = "TEXT|@PATH")]
    append_system_prompt: Vec<String>,
    /// Load a prompt template file or directory (repeatable).
    #[arg(long = "prompt-template", value_name = "PATH")]
    prompt_template: Vec<String>,
    /// Skip the default prompt template directories (explicit paths still load).
    #[arg(long = "no-prompt-templates")]
    no_prompt_templates: bool,
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
            Command::Sessions(args) => sessions_cmd::sessions_command(args),
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
    if args.common.no_session {
        eprintln!("[error] --no-session cannot be combined with resume");
        return 2;
    }
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
        // 只给 --model：provider 留空，由内核按目录唯一性解析（设计 08 §5）。
        (None, Some(m)) => Some((String::new(), m.clone())),
        (Some(_), None) => return Err("--provider needs --model as well".into()),
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
    // `--tools`：逗号列表去空；整表为空视为用法错误（设计 08 §4.2）。
    let tools: Vec<String> = common
        .tools
        .iter()
        .map(|tool| tool.trim().to_string())
        .filter(|tool| !tool.is_empty())
        .collect();
    if !common.tools.is_empty() && tools.is_empty() {
        return Err("--tools expects at least one tool name or pattern".into());
    }
    // `--append-system-prompt`：`@path` 读文件，读不到即用法错误；空文本忽略并警告。
    let mut append_prompt = Vec::new();
    for item in &common.append_system_prompt {
        match item.strip_prefix('@') {
            Some(path) => append_prompt.push(std::fs::read_to_string(path).map_err(|error| {
                format!("--append-system-prompt cannot read `{path}`: {error}")
            })?),
            None if item.trim().is_empty() => {
                eprintln!("[warn] --append-system-prompt: empty text ignored");
            }
            None => append_prompt.push(item.clone()),
        }
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
        no_session: common.no_session,
        tool_allowlist: (!tools.is_empty()).then_some(tools),
        append_system_prompt: append_prompt,
        prompt_template_paths: common
            .prompt_template
            .iter()
            .map(|path| path.trim().to_string())
            .filter(|path| !path.is_empty())
            .collect(),
        no_prompt_templates: common.no_prompt_templates,
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// 用真实 clap 解析器构造 `Common`（测试镜像生产路径，不手搭结构体）。
    fn common(args: &[&str]) -> Common {
        let cmd = <Common as clap::Args>::augment_args(
            clap::Command::new("timo").disable_help_flag(true),
        );
        // argv[0] 是程序名：不补一位的话第一个 flag 会被当成命令名吃掉。
        let mut argv = vec!["timo"];
        argv.extend_from_slice(args);
        let matches = cmd.try_get_matches_from(argv).expect("parse");
        <Common as clap::FromArgMatches>::from_arg_matches(&matches).expect("into Common")
    }

    #[test]
    fn tool_allowlist_and_prompt_flags_reach_kernel_options() {
        let options = build_options(
            &common(&[
                "--no-session",
                "--tools",
                "read, grep",
                "--append-system-prompt",
                "flag rule",
            ]),
            None,
        )
        .expect("options");
        assert!(options.no_session);
        assert_eq!(
            options.tool_allowlist.as_deref(),
            Some(["read".to_string(), "grep".to_string()].as_slice())
        );
        assert_eq!(options.append_system_prompt, ["flag rule".to_string()]);
    }

    #[test]
    fn at_prefix_reads_the_file_into_the_prompt_appendix() {
        let dir = std::env::temp_dir().join(format!("timo-cli-append-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("rule.md");
        std::fs::write(&path, "rule from file").expect("write");
        let flag = format!("@{}", path.display());
        let options = build_options(&common(&["--append-system-prompt", &flag]), None)
            .expect("readable file");
        assert_eq!(options.append_system_prompt, ["rule from file".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 走生产入口拿用法错误文本（`KernelOptions` 不含 Debug，不能 unwrap_err）。
    fn usage_error(common: &Common) -> String {
        match build_options(common, None) {
            Ok(_) => panic!("expected a usage error"),
            Err(error) => error,
        }
    }

    #[test]
    fn empty_tool_list_and_unreadable_append_file_are_usage_errors() {
        assert_eq!(
            usage_error(&common(&["--tools", ",,"])),
            "--tools expects at least one tool name or pattern"
        );
        let error = usage_error(&common(&["--append-system-prompt", "@missing-rule.md"]));
        assert!(
            error.starts_with("--append-system-prompt cannot read `missing-rule.md`:"),
            "{error}"
        );
    }

    #[test]
    fn a_bare_model_flag_leaves_the_provider_empty() {
        let options = build_options(&common(&["--model", "deepseek-chat"]), None)
            .expect("bare --model is legal");
        assert_eq!(
            options
                .model_override
                .as_ref()
                .map(|(p, m)| (p.as_str(), m.as_str())),
            Some(("", "deepseek-chat"))
        );
        assert_eq!(
            usage_error(&common(&["--provider", "deepseek"])),
            "--provider needs --model as well"
        );
    }

    #[test]
    fn template_flags_reach_kernel_options() {
        let options = build_options(
            &common(&[
                "--prompt-template",
                "a.md",
                "--prompt-template",
                " b ",
                "--no-prompt-templates",
            ]),
            None,
        )
        .expect("options");
        assert_eq!(
            options.prompt_template_paths,
            ["a.md".to_string(), "b".to_string()]
        );
        assert!(options.no_prompt_templates);
        // 空白项被丢掉，不会把 cwd 本身当成模板目录去扫。
        assert_eq!(
            build_options(&common(&["--prompt-template", "   "]), None)
                .expect("options")
                .prompt_template_paths,
            Vec::<String>::new()
        );
    }
}
