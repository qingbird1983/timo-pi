//! 宿主工作线程：查询、存储、`runTool` 与主题补丁（设计 03 §3、04 §2）。
//!
//! handler 只把调用入队；真正的读写在这里的独立线程与 tokio 运行时完成，
//! UI 副作用统一转成 [`HostUiEvent`]。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nsh_engine::HostReply;
use serde_json::{Value, json};
use timo_kernel::plugins::PermissionTier;
use timo_kernel::plugins::host::{self as kernel_host, HostError, PluginStorage, ThemeWarning};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::bridge::{PluginRequest, ToolCall};

use super::host_api::{HostJob, HostUiEvent};

/// 查询上限（`runTool` 单独 30s；设计 03 §3、04 §2）。
const QUERY_TIMEOUT: Duration = Duration::from_secs(3);
/// `runTool` 宿主等待上限（与内核侧一致）。
const TOOL_TIMEOUT: Duration = Duration::from_secs(30);

/// 一个实例的宿主线程上下文（进入线程后不再借用清单）。
pub(super) struct WorkerContext {
    pub(super) plugin_id: String,
    pub(super) version: String,
    pub(super) tier: PermissionTier,
    pub(super) queries: Vec<String>,
    pub(super) actions: Vec<String>,
    pub(super) tools: Vec<String>,
    pub(super) slots: Vec<String>,
    pub(super) storage: PluginStorage,
    pub(super) sessions_dir: PathBuf,
    pub(super) work_dir: PathBuf,
    pub(super) read_scopes: Vec<String>,
    pub(super) theme: Arc<Mutex<Value>>,
    pub(super) ui: UnboundedSender<HostUiEvent>,
    pub(super) kernel: UnboundedSender<PluginRequest>,
    pub(super) cancel: CancellationToken,
}

pub(super) fn worker_loop(
    receiver: tokio::sync::mpsc::UnboundedReceiver<HostJob>,
    context: WorkerContext,
) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    runtime.block_on(async move {
        let mut receiver = receiver;
        while let Some(job) = receiver.recv().await {
            handle_job(job, &context).await;
        }
    });
}

async fn handle_job(job: HostJob, context: &WorkerContext) {
    let HostJob {
        caller,
        name,
        data,
        reply,
    } = job;
    if name == "theme.update" {
        apply_theme(&data, context);
        return;
    }
    let who = caller.caller();
    if kernel_host::QUERIES.contains(&name.as_str()) {
        let result = match kernel_host::parse_query(&who, &name, &data) {
            Ok(query) => tokio::time::timeout(QUERY_TIMEOUT, run_query(query, context))
                .await
                .unwrap_or_else(|_| {
                    Err(HostError::new("E742", format!("query `{name}` timed out")))
                }),
            Err(error) => Err(error),
        };
        answer(reply, result);
        return;
    }
    let action = match kernel_host::parse_action(&who, &name, &data) {
        Ok(action) => action,
        Err(error) => {
            answer(reply, Err(error));
            return;
        }
    };
    if !matches!(action, kernel_host::Action::RunTool { .. })
        && let Err(error) = kernel_host::authorize(&who, &action, None)
    {
        answer(reply, Err(error));
        return;
    }
    match action {
        kernel_host::Action::RunTool { name, arguments } => {
            run_tool(reply, context, name, arguments).await;
        }
        action => answer(reply, run_action(action, context)),
    }
}

async fn run_query(query: kernel_host::Query, context: &WorkerContext) -> Result<Value, HostError> {
    use kernel_host::Query;
    match query {
        Query::SessionsList { limit, offset } => Ok(json!(kernel_host::list_sessions(
            &context.sessions_dir,
            limit,
            offset
        ))),
        Query::SessionsSearch { query, limit } => Ok(json!(kernel_host::search_sessions(
            &context.sessions_dir,
            &query,
            limit
        ))),
        Query::SessionsGet { id } => Ok(kernel_host::get_session(&context.sessions_dir, &id)
            .map_or(Value::Null, |row| json!(row))),
        Query::ModelsList => models(context).await,
        Query::ThemeGet => Ok(context
            .theme
            .lock()
            .map(|theme| theme.clone())
            .unwrap_or(Value::Null)),
        Query::PluginInfo => Ok(json!({
            "id": context.plugin_id,
            "version": context.version,
            "tier": context.tier,
            "slots": context.slots,
            "capabilities": { "queries": context.queries, "actions": context.actions },
        })),
        Query::StorageGet { key } => context
            .storage
            .get(&key)
            .map(|value| value.map_or(Value::Null, |value| json!({ "value": value }))),
    }
}

async fn models(context: &WorkerContext) -> Result<Value, HostError> {
    let (reply, answer) = oneshot::channel();
    context
        .kernel
        .send(PluginRequest::Models(reply))
        .map_err(|_| HostError::new("E742", "kernel is not available"))?;
    match tokio::time::timeout(QUERY_TIMEOUT, answer).await {
        Ok(Ok(models)) => Ok(serde_json::to_value(models).unwrap_or(Value::Null)),
        _ => Err(HostError::new("E742", "models.list timed out")),
    }
}

fn run_action(action: kernel_host::Action, context: &WorkerContext) -> Result<Value, HostError> {
    use kernel_host::Action;
    match action {
        Action::Notify { level, message } => emit(
            context,
            HostUiEvent::Notify {
                plugin: context.plugin_id.clone(),
                level,
                message,
            },
        ),
        Action::OpenSession { session_id } => {
            emit(context, HostUiEvent::OpenSession(session_id));
        }
        Action::OpenPath { path } => {
            let resolved =
                kernel_host::check_open_path(&context.work_dir, &context.read_scopes, &path)?;
            emit(
                context,
                HostUiEvent::OpenPath(resolved.to_string_lossy().into_owned()),
            );
        }
        Action::SendPrompt { text, submit } => {
            emit(context, HostUiEvent::SendPrompt { text, submit });
        }
        Action::OpenPanel { .. } => {
            emit(
                context,
                HostUiEvent::OpenPanel {
                    plugin: context.plugin_id.clone(),
                },
            );
        }
        Action::ClipboardWrite { text } => {
            emit(context, HostUiEvent::ClipboardWrite(text));
        }
        Action::StorageSet { key, value } => context.storage.set(&key, value)?,
        Action::RunTool { .. } => unreachable!("runTool is handled by run_tool"),
    }
    Ok(Value::Null)
}

async fn run_tool(
    reply: Option<HostReply>,
    context: &WorkerContext,
    name: String,
    arguments: Value,
) {
    let (answer, result) = oneshot::channel();
    let call = ToolCall {
        tier: context.tier,
        tools: context.tools.clone(),
        name,
        arguments,
        cancel: context.cancel.clone(),
        reply: answer,
    };
    if context.kernel.send(PluginRequest::Tool(call)).is_err() {
        reject(reply, "E742", "kernel is not available");
        return;
    }
    match tokio::time::timeout(TOOL_TIMEOUT, result).await {
        Ok(Ok(Ok(value))) => resolve(reply, value),
        Ok(Ok(Err(error))) => reject(reply, error.code(), error.message()),
        Ok(Err(_)) => reject(reply, "E742", "runTool cancelled"),
        Err(_) => reject(reply, "E742", "runTool timed out"),
    }
}

fn apply_theme(data: &Value, context: &WorkerContext) {
    let (patch, warnings) = kernel_host::sanitize_theme(data);
    for ThemeWarning { key, reason } in warnings {
        emit(
            context,
            HostUiEvent::Diagnostic {
                plugin: context.plugin_id.clone(),
                code: "E743".into(),
                message: format!("{key}: {reason}"),
            },
        );
    }
    if !patch.is_empty() {
        emit(
            context,
            HostUiEvent::Theme {
                plugin: context.plugin_id.clone(),
                tokens: patch.tokens,
            },
        );
    }
}

fn emit(context: &WorkerContext, event: HostUiEvent) {
    let _ = context.ui.send(event);
}

fn answer(reply: Option<HostReply>, result: Result<Value, HostError>) {
    match result {
        Ok(value) => resolve(reply, value),
        Err(error) => reject(reply, error.code(), error.message()),
    }
}

fn resolve(reply: Option<HostReply>, value: Value) {
    if let Some(reply) = reply {
        reply.resolve(value);
    }
}

fn reject(reply: Option<HostReply>, code: &str, message: &str) {
    if let Some(reply) = reply {
        reply.reject(format!("{code}: {message}"));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use serde_json::json;

    use super::*;
    use tokio::sync::mpsc::unbounded_channel;

    /// 主题补丁越界键只产生 E743 诊断，合法 token 进 Theme 事件。
    #[test]
    fn theme_warnings_are_reported_and_tokens_sanitized() {
        let (ui, mut events) = unbounded_channel();
        let context = WorkerContext {
            plugin_id: "demo".into(),
            version: "1.0.0".into(),
            tier: PermissionTier::ReadOnly,
            queries: vec![],
            actions: vec![],
            tools: vec![],
            slots: vec!["panel".into()],
            storage: PluginStorage::new(std::env::temp_dir().as_path(), "demo-theme").unwrap(),
            sessions_dir: std::env::temp_dir(),
            work_dir: std::env::temp_dir(),
            read_scopes: vec![],
            theme: Arc::new(Mutex::new(Value::Null)),
            ui,
            kernel: unbounded_channel().0,
            cancel: CancellationToken::new(),
        };
        apply_theme(
            &json!({ "background": "#101010", "display": "none" }),
            &context,
        );
        let mut saw_theme = false;
        let mut saw_warning = false;
        while let Ok(event) = events.try_recv() {
            match event {
                HostUiEvent::Theme { tokens, .. } => {
                    assert_eq!(
                        tokens.get("background").map(String::as_str),
                        Some("#101010")
                    );
                    assert_eq!(tokens.len(), 1);
                    saw_theme = true;
                }
                HostUiEvent::Diagnostic { code, message, .. } => {
                    assert_eq!(code, "E743");
                    assert!(message.contains("display"));
                    saw_warning = true;
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
        assert!(saw_theme && saw_warning);
    }
}
