//! CLI 输入分发（设计 phase-3.5/02 §6.2）：扩展命令 → 内建 → `/skill:` → 普通提示。

use timo_kernel::{Kernel, WarmingState};

use crate::run_prompt;

pub(crate) enum CommandOutcome {
    Handled,
    Quit,
    Prompt,
}

/// 首次装配后打印一次资源告警（`/reload` 会单独再打）。
pub(crate) static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 输入分发（设计 02 §6.2）：扩展命令 → 内建 → `/skill:` → 普通提示。
pub(crate) async fn handle_command(kernel: &mut Kernel, text: &str) -> CommandOutcome {
    if !text.starts_with('/') {
        return CommandOutcome::Prompt;
    }

    if let Some((raw, args)) = timo_kernel::split_command(text) {
        let candidates = kernel.extension_commands();
        let name = candidates
            .iter()
            .find(|name| name.as_str() == raw || name.ends_with(&format!("_{raw}")))
            .cloned();
        if let Some(name) = name {
            if let Err(error) = kernel.run_extension_command(&name, args) {
                eprintln!("[error] {error}");
            }
            return CommandOutcome::Handled;
        }
    }

    if text.starts_with("/skill:") {
        let expanded = kernel.expand_input(text);
        if expanded != text {
            let _ = run_prompt(kernel, &expanded).await;
            return CommandOutcome::Handled;
        }
        eprintln!("[error] unknown skill command `{text}`");
        return CommandOutcome::Handled;
    }

    let body = text.trim_start_matches('/');
    let (raw, rest) = match body.split_once(char::is_whitespace) {
        Some((raw, rest)) => (raw, rest.trim()),
        None => (body, ""),
    };

    match raw {
        "abort" => {
            kernel.abort();
            CommandOutcome::Handled
        }
        "ext" => {
            if let Some(table) = kernel.extension_report_human() {
                eprintln!("{table}");
            }
            CommandOutcome::Handled
        }
        "new" => {
            kernel.new_session();
            eprintln!("[session] new draft (saved on the first prompt)");
            CommandOutcome::Handled
        }
        "resume" => {
            if rest.is_empty() {
                for session in kernel.list_sessions().iter().take(10) {
                    println!(
                        "{}  {} msgs  {}",
                        session.id, session.message_count, session.first_message
                    );
                }
                eprintln!("[session] /resume <id> to resume");
            } else if let Err(error) = kernel.resume(rest).await {
                eprintln!("[error] {error}");
            }
            CommandOutcome::Handled
        }
        "session" => {
            match kernel.session_id() {
                Some(id) => {
                    let summary = kernel.usage_summary();
                    println!(
                        "[session] {id} · {} · {} messages · input={} output={} cache_read={} cache_write={} · ${:.4}",
                        kernel.model_label(),
                        kernel.history().map_or(0, |history| history.len()),
                        summary.totals.input,
                        summary.totals.output,
                        summary.totals.cache_read,
                        summary.totals.cache_write,
                        summary.totals.cost
                    );
                    for bucket in &summary.buckets {
                        println!(
                            "  {} tokens={} ${:.4}",
                            bucket.key, bucket.tokens, bucket.cost
                        );
                    }
                    match kernel.cache_stats() {
                        Ok(stats) if stats.miss_count > 0 => {
                            let mut line = format!(
                                "  cache: {} misses · {} tok re-billed · ${:.4}",
                                stats.miss_count, stats.missed_tokens, stats.missed_cost
                            );
                            if let Some(idle) = stats.misses.iter().map(|m| m.idle_ms).max()
                                && idle > 300_000
                            {
                                line.push_str(&format!(
                                    " (last miss after {}m idle; TTL may have expired)",
                                    idle / 60_000
                                ));
                            }
                            println!("{line}");
                        }
                        Ok(_) => {}
                        Err(error) => eprintln!("[error] cache stats: {error}"),
                    }
                    println!("  warming: {}", warming_line(kernel.cache_warming_status()));
                }
                None => println!("[session] draft (not saved yet)"),
            }
            CommandOutcome::Handled
        }
        "reload" => {
            kernel.reload_resources();
            print_resource_warnings(kernel);
            eprintln!("[reload] context files and skills reloaded");
            CommandOutcome::Handled
        }
        "export" => {
            let target = (!rest.is_empty()).then(|| std::path::Path::new(rest));
            match kernel.export_session(target) {
                Ok(path) => println!("[export] {}", path.display()),
                Err(error) => eprintln!("[error] {error}"),
            }
            CommandOutcome::Handled
        }
        "compact" => {
            let instructions = (!rest.is_empty()).then_some(rest);
            match kernel.compact(instructions).await {
                Ok(outcome) => println!(
                    "[compact] tokens: {} → {} (kept {} messages)",
                    outcome.tokens_before, outcome.tokens_after, outcome.keep_recent_messages
                ),
                Err(error) => eprintln!("[error] {error}"),
            }
            CommandOutcome::Handled
        }
        "quit" => CommandOutcome::Quit,
        _ => {
            match kernel
                .slash_commands()
                .into_iter()
                .find(|command| command.name == raw)
            {
                Some(command) => eprintln!(
                    "[command] /{} is not available yet ({})",
                    command.name,
                    describe_status(command.status)
                ),
                None => eprintln!("[error] unknown command `/{raw}`"),
            }
            CommandOutcome::Handled
        }
    }
}

fn describe_status(status: timo_kernel::CommandStatus) -> String {
    match status {
        timo_kernel::CommandStatus::Ready => "ready".into(),
        timo_kernel::CommandStatus::Planned(batch) => format!("planned for {batch}"),
        timo_kernel::CommandStatus::Unsupported => "out of scope for this phase".into(),
    }
}

/// 预热状态的一行描述（`/session`）。
fn warming_line(state: WarmingState) -> String {
    match state {
        WarmingState::Inactive { reason } => format!("inactive ({reason})"),
        WarmingState::Refreshing => "refreshing".into(),
        WarmingState::Scheduled { next_warm_at_ms } => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
                .unwrap_or(0);
            let left = (next_warm_at_ms - now).max(0);
            if left >= 60_000 {
                format!("scheduled in {}m", left / 60_000)
            } else {
                format!("scheduled in {}s", (left + 999) / 1000)
            }
        }
    }
}

pub(crate) fn print_resource_warnings(kernel: &Kernel) {
    for warning in kernel.resource_warnings() {
        eprintln!("[warn] {}: {}", warning.path.display(), warning.message);
    }
}
