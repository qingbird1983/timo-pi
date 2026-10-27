//! codemode 沙箱：QuickJS 线程、嵌套调用泵、输出与资源限制（设计 phase-3.5/11 §4.3/§4.4）。

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use timo_tools::{CallableTool, NestedCall, ToolCallCtx, ToolOutput};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use super::format::{self, ScriptRun};
use super::{Limits, describe};

mod worker;

/// 中断后等待脚本线程收尾的上界（兜底；正常路径远小于此）。
const THREAD_STOP_GRACE: Duration = Duration::from_secs(5);

/// 进程级 worker 上限（设计 §4.4；跨会话共享）。
static WORKERS: OnceLock<Arc<Semaphore>> = OnceLock::new();

pub(super) fn workers(limit: usize) -> Arc<Semaphore> {
    Arc::clone(WORKERS.get_or_init(|| Arc::new(Semaphore::new(limit))))
}

struct PostRequest {
    id: u64,
    name: String,
    args: Value,
}

struct Settled {
    id: u64,
    ok: bool,
    payload: String,
}

struct ScriptInput {
    body: String,
    decls_json: String,
    limits_json: String,
    request_tx: tokio::sync::mpsc::UnboundedSender<PostRequest>,
    settle_rx: std::sync::mpsc::Receiver<Settled>,
    cancel: CancellationToken,
    deadline: Option<Instant>,
    timeout_ms: Option<u64>,
    memory_bytes: usize,
}

/// 执行一段脚本（工具定义的 execute 入口）。
pub(super) async fn execute(
    ctx: ToolCallCtx,
    callable: Arc<Vec<CallableTool>>,
    limits: Arc<Limits>,
) -> timo_tools::Result<ToolOutput> {
    let started = Instant::now();
    let Some(host) = ctx.nested.clone() else {
        return Ok(ToolOutput::error_text("codemode needs a session"));
    };
    let code = ctx
        .params
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let source = match format::parse_source(&code) {
        Ok(source) => source,
        Err(error) => {
            return Ok(format::failure_output(
                &format!("Script error: bad @options line: {error}"),
                started,
                &[],
            ));
        }
    };

    // worker 准入：等待计入脚本 deadline。
    let semaphore = workers(limits.workers);
    let wait = source
        .timeout_ms
        .map(Duration::from_millis)
        .map_or(limits.worker_wait, |timeout| {
            timeout.min(limits.worker_wait)
        });
    let permit = tokio::select! {
        _ = ctx.cancel.cancelled() => {
            return Ok(format::failure_output(
                "Script aborted: execution aborted",
                started,
                &[],
            ));
        }
        acquired = tokio::time::timeout(wait, semaphore.clone().acquire_owned()) => {
            match acquired {
                Ok(Ok(permit)) => permit,
                Ok(Err(_)) => {
                    return Ok(format::failure_output(
                        "Script sandbox failed: codemode worker pool is closed",
                        started,
                        &[],
                    ));
                }
                Err(_) => {
                    let message = match source.timeout_ms {
                        Some(timeout) if Duration::from_millis(timeout) <= limits.worker_wait => format!(
                            "Script timed out: execution timed out after {timeout} ms while waiting for a free codemode worker"
                        ),
                        _ => format!(
                            "No codemode worker became free within {} seconds. All {} workers, shared by every session in this process, are held by other scripts. Retry after they finish.",
                            limits.worker_wait.as_secs(),
                            limits.workers
                        ),
                    };
                    return Ok(format::failure_output(&message, started, &[]));
                }
            }
        }
    };

    let deadline = source
        .timeout_ms
        .map(|ms| started + Duration::from_millis(ms));
    let script_cancel = ctx.cancel.child_token();
    // 子调用用另一个派生令牌：超时后取消子调用清理，但不把"脚本自身"标成父取消，
    // 否则超时会被误报成 Script aborted。
    let children_cancel = script_cancel.child_token();
    let parent_id = ctx.tool_call_id.clone();
    let updates = ctx.updates.clone();
    let by_name: HashMap<String, CallableTool> = callable
        .iter()
        .map(|tool| (tool.name.clone(), tool.clone()))
        .collect();
    let decls = describe::declarations(&callable);
    let decls_json = serde_json::to_string(
        &decls
            .iter()
            .map(|decl| {
                json!({
                    "id": decl.id,
                    "name": decl.name,
                    "declaration": decl.declaration,
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".to_string());
    let limits_json = json!({
        "max_chars": limits.max_output_chars,
        "max_calls": limits.max_output_calls,
    })
    .to_string();

    let (request_tx, mut request_rx) = tokio::sync::mpsc::unbounded_channel::<PostRequest>();
    let (settle_tx, settle_rx) = std::sync::mpsc::channel::<Settled>();
    let input = ScriptInput {
        body: source.body,
        decls_json,
        limits_json,
        request_tx,
        settle_rx,
        cancel: script_cancel.clone(),
        deadline,
        timeout_ms: source.timeout_ms,
        memory_bytes: limits.memory_bytes,
    };
    // 每条脚本一条独立 OS 线程（不用 `spawn_blocking`：runtime 关闭会等待
    // 卡死的阻塞任务，而中断失效时脚本线程可能停不下来；std 线程可被进程退出回收）。
    let (done_tx, mut done) = tokio::sync::oneshot::channel::<ScriptRun>();
    if let Err(error) = std::thread::Builder::new()
        .name("timo-codemode".into())
        .spawn(move || {
            let _ = done_tx.send(worker::run_blocking(input));
        })
    {
        return Ok(format::failure_output(
            &format!("Script sandbox failed: {error}"),
            started,
            &[],
        ));
    }
    let mut tasks = tokio::task::JoinSet::new();
    let far_future = Instant::now() + Duration::from_secs(60 * 60 * 24 * 365);
    let sleep_until = tokio::time::sleep_until(tokio::time::Instant::from_std(
        deadline.unwrap_or(far_future),
    ));
    tokio::pin!(sleep_until);

    let outcome = loop {
        tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => break None,
            _ = &mut sleep_until => break None,
            Some(request) = request_rx.recv() => {
                let host = Arc::clone(&host);
                let sender = settle_tx.clone();
                let by_name = by_name.clone();
                let cancel = children_cancel.clone();
                let parent_id = parent_id.clone();
                let updates = updates.clone();
                tasks.spawn(async move {
                    let name = request.name.clone();
                    let Some(tool) = by_name.get(&name) else {
                        let _ = sender.send(Settled {
                            id: request.id,
                            ok: false,
                            payload: format!("Unknown tool \"{name}\""),
                        });
                        return;
                    };
                    let _ = updates
                        .send(format::progress(
                            host.as_ref(),
                            &parent_id,
                            format!("→ tools.{name}"),
                        ))
                        .await;
                    let outcome = host
                        .execute(NestedCall {
                            parent_id: parent_id.clone(),
                            name: name.clone(),
                            arguments: request.args,
                            cancel,
                            updates: updates.clone(),
                        })
                        .await;
                    let (ok, payload) = format::script_value(tool, &name, &outcome.output);
                    let _ = updates
                        .send(format::progress(
                            host.as_ref(),
                            &parent_id,
                            format!("← {name} {}", if ok { "ok" } else { "error" }),
                        ))
                        .await;
                    let _ = sender.send(Settled {
                        id: request.id,
                        ok,
                        payload,
                    });
                });
            }
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {}
            result = &mut done => break Some(result),
        }
    };
    // 脚本结束（正常或中断）：先取消子调用，给它们一个清理窗口（杀进程树等），
    // 再兜底 abort 仍在跑的调用。
    children_cancel.cancel();
    let drained = tokio::time::timeout(Duration::from_secs(2), async {
        while tasks.join_next().await.is_some() {}
    })
    .await;
    if drained.is_err() {
        tasks.abort_all();
    }
    drop(settle_tx);
    let result = match outcome {
        Some(result) => result,
        // 兜底：中断处理器失效时线程可能停不下来，调用本身仍须有界返回
        // （线程在后台自生自灭；正常路径在毫秒级结束）。
        None => match tokio::time::timeout(THREAD_STOP_GRACE, &mut done).await {
            Ok(result) => result,
            Err(_) => Ok(ScriptRun {
                items: Vec::new(),
                console: Vec::new(),
                error: Some(
                    "Script sandbox stopped: the script thread did not stop in time".into(),
                ),
            }),
        },
    };
    drop(permit);
    Ok(format::finish_output(
        result,
        host.as_ref(),
        &parent_id,
        started,
        source.max_output_tokens,
    ))
}
