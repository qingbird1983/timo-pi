//! codemode 沙箱：QuickJS 线程、嵌套调用泵、输出与资源限制（设计 phase-3.5/11 §4.3/§4.4）。

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use rquickjs::promise::{Promise, PromiseState};
use rquickjs::{Context, Function, Runtime};
use serde_json::{Value, json};
use timo_tools::{CallableTool, NestedCall, ToolCallCtx, ToolOutput};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use super::format::{self, ScriptRun};
use super::{Limits, describe};

const SHIM: &str = include_str!("shim.js");

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

struct Taken {
    items: Vec<String>,
    console: Vec<String>,
    error: Option<String>,
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
            let _ = done_tx.send(run_blocking(input));
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
                let cancel = script_cancel.clone();
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
    // 脚本结束（正常或中断）：停掉未完成的嵌套调用并让脚本线程收尾。
    script_cancel.cancel();
    tasks.abort_all();
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

fn run_blocking(input: ScriptInput) -> ScriptRun {
    let runtime = match Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            return ScriptRun {
                items: Vec::new(),
                console: Vec::new(),
                error: Some(format!("Script sandbox failed: {error}")),
            };
        }
    };
    runtime.set_memory_limit(input.memory_bytes);
    let ScriptInput {
        body,
        decls_json,
        limits_json,
        request_tx,
        settle_rx,
        cancel,
        deadline,
        timeout_ms,
        ..
    } = input;
    {
        let cancel = cancel.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            cancel.is_cancelled() || deadline.is_some_and(|at| Instant::now() >= at)
        })));
    }
    let context = match Context::full(&runtime) {
        Ok(context) => context,
        Err(error) => {
            return ScriptRun {
                items: Vec::new(),
                console: Vec::new(),
                error: Some(format!("Script sandbox failed: {error}")),
            };
        }
    };
    let result = context.with(|ctx| {
        let install = (|| -> rquickjs::Result<()> {
            let post = Function::new(ctx.clone(), move |json: String| {
                let Ok(value) = serde_json::from_str::<Value>(&json) else {
                    return;
                };
                let (Some(id), Some(name)) = (
                    value.get("id").and_then(Value::as_u64),
                    value.get("name").and_then(Value::as_str),
                ) else {
                    return;
                };
                let _ = request_tx.send(PostRequest {
                    id,
                    name: name.to_string(),
                    args: value.get("args").cloned().unwrap_or(Value::Null),
                });
            })?;
            ctx.globals().set("__timo_decls_json", decls_json)?;
            ctx.globals().set("__timo_limits_json", limits_json)?;
            ctx.globals().set("__timo_post", post)?;
            ctx.eval::<(), _>(SHIM)?;
            Ok(())
        })();
        if let Err(error) = install {
            return ScriptRun {
                items: Vec::new(),
                console: Vec::new(),
                error: Some(format!("Script sandbox failed: {error}")),
            };
        }
        let wrapper = format!(
            "globalThis.__timo_main = async function() {{ try {{ const value = await (async function() {{\n{body}\n}}).call(globalThis); if (value !== undefined) text(value); }} catch (error) {{ if (!(error && error.__timo_exit)) {{ globalThis.__timo_error = __timo_describe_error(error); }} }} }};"
        );
        if let Err(error) = ctx.eval::<(), _>(wrapper) {
            let taken = take_output(&ctx).unwrap_or(Taken {
                items: Vec::new(),
                console: Vec::new(),
                error: None,
            });
            return ScriptRun {
                items: taken.items,
                console: taken.console,
                error: Some(format!("SyntaxError: {error}")),
            };
        }
        let outcome = (|| -> Result<(), String> {
            let value: rquickjs::Value<'_> = ctx
                .eval("__timo_main()")
                .map_err(|error| error.to_string())?;
            let promise = value
                .into_promise()
                .ok_or_else(|| "script did not return a promise".to_string())?;
            pump(&ctx, &promise, &settle_rx, &cancel, deadline, timeout_ms)
        })();
        let taken = take_output(&ctx).unwrap_or(Taken {
            items: Vec::new(),
            console: Vec::new(),
            error: None,
        });
        let error = match outcome {
            Ok(()) => taken.error,
            Err(stopped) => {
                let stopped_reason = stop_reason(&cancel, deadline, timeout_ms);
                Some(stopped_reason.unwrap_or(stopped))
            }
        };
        ScriptRun {
            items: taken.items,
            console: taken.console,
            error,
        }
    });
    runtime.set_interrupt_handler(None);
    result
}

fn pump(
    ctx: &rquickjs::Ctx<'_>,
    promise: &Promise<'_>,
    settle_rx: &std::sync::mpsc::Receiver<Settled>,
    cancel: &CancellationToken,
    deadline: Option<Instant>,
    timeout_ms: Option<u64>,
) -> Result<(), String> {
    loop {
        while ctx.execute_pending_job() {}
        loop {
            match settle_rx.try_recv() {
                Ok(settled) => settle(ctx, settled)?,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(stop_reason(cancel, deadline, timeout_ms)
                        .unwrap_or_else(|| "Script sandbox stopped".to_string()));
                }
            }
        }
        match promise.state() {
            PromiseState::Resolved => return Ok(()),
            PromiseState::Rejected => {
                return Err(promise
                    .result::<rquickjs::Value<'_>>()
                    .map(|value| format!("{value:?}"))
                    .unwrap_or_else(|| "script promise rejected".to_string()));
            }
            PromiseState::Pending => {}
        }
        if let Some(reason) = stop_reason(cancel, deadline, timeout_ms) {
            return Err(reason);
        }
        let pending: i64 = ctx.eval("__timo_pending_count()").unwrap_or(0);
        if pending == 0 {
            return Err("Script promise did not settle (no timers in the sandbox)".to_string());
        }
        let wait = deadline.map_or(Duration::from_secs(86_400), |at| {
            at.saturating_duration_since(Instant::now())
        });
        match settle_rx.recv_timeout(wait) {
            Ok(settled) => settle(ctx, settled)?,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(stop_reason(cancel, deadline, timeout_ms)
                    .unwrap_or_else(|| "Script sandbox stopped".to_string()));
            }
        }
    }
}

fn settle(ctx: &rquickjs::Ctx<'_>, settled: Settled) -> Result<(), String> {
    let function: Function<'_> = ctx
        .globals()
        .get("__timo_settle")
        .map_err(|error| error.to_string())?;
    function
        .call((settled.id, settled.ok, settled.payload))
        .map_err(|error| error.to_string())
}

fn stop_reason(
    cancel: &CancellationToken,
    deadline: Option<Instant>,
    timeout_ms: Option<u64>,
) -> Option<String> {
    if cancel.is_cancelled() {
        return Some("Script aborted: execution aborted".to_string());
    }
    if deadline.is_some_and(|at| Instant::now() >= at) {
        return Some(format!(
            "Script timed out: execution timed out after {} ms",
            timeout_ms.unwrap_or(0)
        ));
    }
    None
}

fn take_output(ctx: &rquickjs::Ctx<'_>) -> Option<Taken> {
    let json: String = ctx.eval("__timo_take_output()").ok()?;
    let value: Value = serde_json::from_str(&json).ok()?;
    let strings = |key: &str| -> Vec<String> {
        value
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    Some(Taken {
        items: strings("items"),
        console: strings("console"),
        error: value
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}
