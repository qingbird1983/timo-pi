//! 无头打印模式（phase-3.5/06）：`-p` 单次执行，text 只出终答、json 出 JSONL。
//!
//! 走完整内核（工具、扩展、会话持久化），没有审阅 UI，工具调用自动执行。

use std::pin::Pin;
use std::time::Duration;

use clap::{Args, ValueEnum};
use timo_kernel::{AssistantMessage, Kernel, KernelEvent, StopReason};

use crate::json_mode::{JsonWriter, assistant_text, last_assistant};
use crate::{Common, build_options};

/// 无头输出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PrintMode {
    /// 只输出最终回答正文。
    Text,
    /// 逐行输出 JSON 事件流。
    Json,
}

#[derive(Args)]
pub(crate) struct PrintArgs {
    /// Run a prompt, print the answer, exit (repeatable; same session).
    #[arg(short = 'p', long = "print", value_name = "PROMPT")]
    pub(crate) prompts: Vec<String>,
    /// Output format for print runs.
    #[arg(long = "mode", value_enum)]
    pub(crate) mode: Option<PrintMode>,
    #[command(flatten)]
    pub(crate) common: Common,
    /// Maximum model-request turns per prompt.
    #[arg(long = "max-turns")]
    pub(crate) max_turns: Option<usize>,
}

/// `-p` 入口；返回进程退出码（0 成功 / 1 运行失败 / 2 用法）。
pub(crate) async fn run(args: PrintArgs) -> i32 {
    let mode = args.mode.unwrap_or(PrintMode::Text);
    let options = match build_options(&args.common, args.max_turns) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("[error] {message}");
            return 2;
        }
    };
    let cwd = header_cwd(&args.common);
    let mut kernel = match Kernel::new(options).await {
        Ok(kernel) => kernel,
        Err(error) => {
            eprintln!("[error] {error}");
            return 1;
        }
    };
    // 先建会话，才能在首行输出会话头。
    if let Err(error) = kernel.start_session() {
        eprintln!("[error] {error}");
        return 1;
    }
    let session_id = kernel.session_id().unwrap_or_default().to_string();
    let model = kernel.model_label();
    let mut writer = (mode == PrintMode::Json).then(JsonWriter::new);
    if let Some(writer) = writer.as_mut() {
        writer.header(&session_id, &cwd, &model);
    }
    for prompt in &args.prompts {
        let code = prompt_once(&mut kernel, prompt, writer.as_mut()).await;
        if code != 0 {
            return code;
        }
    }
    0
}

/// 一条 prompt：消费事件（json 模式写出，text 模式忽略），按终止原因给退出码。
async fn prompt_once(
    kernel: &mut Kernel,
    prompt: &str,
    mut writer: Option<&mut JsonWriter>,
) -> i32 {
    let abort = kernel.abort_handle();
    let mut events = kernel.events();
    let mut saw_end = false;
    let mut aborted = false;
    let mut deadline: Option<Pin<Box<tokio::time::Sleep>>> = None;
    let outcome = {
        let run = kernel.run(prompt);
        tokio::pin!(run);
        loop {
            tokio::select! {
                event = events.recv() => {
                    match event {
                        Ok(event) => {
                            if matches!(event, KernelEvent::AgentFinished { .. }) {
                                saw_end = true;
                            }
                            if let Some(writer) = writer.as_mut() {
                                writer.handle(&event);
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {}
                    }
                }
                result = &mut run => break result,
                _ = tokio::signal::ctrl_c() => {
                    if aborted {
                        std::process::exit(130);
                    }
                    aborted = true;
                    if let Some(handle) = &abort {
                        handle.abort();
                    }
                    eprintln!("[abort] cancelling current run (Ctrl+C again to exit)");
                    deadline = Some(Box::pin(tokio::time::sleep(Duration::from_secs(5))));
                }
                _ = async {
                    match deadline.as_mut() {
                        Some(sleep) => sleep.as_mut().await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    // 第一次中断后 5 秒仍未收尾：直接退出。
                    std::process::exit(130);
                }
            }
        }
    };
    // run 返回时收尾事件已同步发出；清空队列，保证 agent_end 不丢。
    loop {
        match events.try_recv() {
            Ok(event) => {
                if matches!(event, KernelEvent::AgentFinished { .. }) {
                    saw_end = true;
                }
                if let Some(writer) = writer.as_mut() {
                    writer.handle(&event);
                }
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
            Err(_) => break,
        }
    }
    if let Some(writer) = writer.as_mut()
        && !saw_end
    {
        writer.end_without_events();
    }
    if aborted {
        std::process::exit(130);
    }
    match outcome {
        Ok(messages) => {
            if writer.is_some() {
                exit_for(last_assistant(&messages))
            } else {
                print_answer(last_assistant(&messages))
            }
        }
        Err(error) => {
            eprintln!("[error] {error}");
            1
        }
    }
}

/// text 模式：正文写 stdout，返回退出码。
fn print_answer(last: Option<&AssistantMessage>) -> i32 {
    let Some(last) = last else {
        eprintln!("[error] run produced no assistant message");
        return 1;
    };
    let text = assistant_text(last);
    if !text.is_empty() {
        print!("{text}");
        if !text.ends_with('\n') {
            println!();
        }
    }
    exit_for(Some(last))
}

/// 终止原因 → 退出码（Stop 0；Length/Error/Aborted 1）。
fn exit_for(last: Option<&AssistantMessage>) -> i32 {
    let Some(last) = last else {
        eprintln!("[error] run produced no assistant message");
        return 1;
    };
    match last.stop_reason {
        StopReason::Stop => 0,
        StopReason::Length => {
            eprintln!("[truncated] response hit the token limit");
            1
        }
        reason => {
            let message = last
                .error_message
                .clone()
                .unwrap_or_else(|| format!("run ended with {reason:?}"));
            eprintln!("[error] {message}");
            1
        }
    }
}

/// 会话头里的 cwd（`--cwd` 或当前目录）。
#[must_use]
pub(crate) fn header_cwd(common: &Common) -> String {
    common
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .map(|path| path.display().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use timo_kernel::Content;

    fn assistant(reason: StopReason) -> AssistantMessage {
        let mut message = AssistantMessage::pending("openai-completions", "fake", "test", 1);
        message.content = vec![Content::Text { text: "ok".into() }];
        message.stop_reason = reason;
        message
    }

    #[test]
    fn stop_is_success_and_truncation_fails() {
        assert_eq!(exit_for(Some(&assistant(StopReason::Stop))), 0);
        assert_eq!(exit_for(Some(&assistant(StopReason::Length))), 1);
        assert_eq!(exit_for(Some(&assistant(StopReason::Error))), 1);
        assert_eq!(exit_for(None), 1);
    }
}
