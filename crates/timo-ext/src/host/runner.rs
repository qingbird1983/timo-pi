//! 宿主线程内的单次操作：装载、工具调用、事件派发与 Promise 收敛。

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rquickjs::promise::PromiseState;
use rquickjs::{Context, Ctx, Function, Object, Value};
use serde_json::{Value as JsonValue, json};

use super::{HostServices, HostState, LoadedExtension, ToolCallResult, native, runtime};
use crate::Result;
use crate::diag::{DiagnosticEntry, ErrorCode};
use crate::error::ExtError;

/// 宿主运行时：核心 API 与事件派发分两文件，按序拼接后一次求值。
const RUNTIME_JS: &str = concat!(
    include_str!("../shim/runtime.js"),
    include_str!("../shim/runtime_dispatch.js")
);

pub(super) fn load_one(
    runtime_handle: &rquickjs::Runtime,
    id: &str,
    bundle: &str,
    deadline: &Rc<Cell<Option<Instant>>>,
    budget: Duration,
    state: Arc<Mutex<HostState>>,
    services: Arc<dyn HostServices>,
) -> Result<(LoadedExtension, Context)> {
    let context = Context::full(runtime_handle)
        .map_err(|error| runtime(ErrorCode::E202, format!("cannot create context: {error}")))?;
    context.with(|ctx| -> Result<()> {
        native::install(&ctx, id, Arc::clone(&state), Arc::clone(&services)).map_err(js_error)?;
        ctx.eval::<(), _>(RUNTIME_JS).map_err(js_error)?;
        let prompt = state
            .lock()
            .map(|state| state.system_prompt.clone())
            .unwrap_or_default();
        ctx.globals()
            .set("__timo_system_prompt", prompt)
            .map_err(js_error)?;
        let wrapped = format!(
            "(function(){{var module={{exports:{{}}}};var exports=module.exports;\n{bundle}\n;globalThis.__timo_factory=(module.exports&&module.exports.default)?module.exports.default:module.exports;}})();"
        );
        ctx.eval::<(), _>(wrapped).map_err(|error| {
            runtime(
                ErrorCode::E203,
                format!("bundle must expose a default factory: {error}"),
            )
        })?;
        let factory: Value = ctx.globals().get("__timo_factory").map_err(js_error)?;
        if !factory.is_function() {
            return Err(runtime(
                ErrorCode::E203,
                "bundle default export is not a function",
            ));
        }
        let mut config = services.config();
        if let Some(object) = config.as_object_mut() {
            object.insert("extId".into(), json!(id));
            object.insert("prefix".into(), json!(format!("plugin_{id}_")));
            object.insert("systemPrompt".into(), json!(""));
        }
        let config_json = serde_json::to_string(&config)
            .map_err(|error| runtime(ErrorCode::E202, error.to_string()))?;
        ctx.globals()
            .set(
                "__timo_config",
                ctx.json_parse(config_json.clone()).map_err(js_error)?,
            )
            .map_err(js_error)?;
        let _ = invoke(&ctx, deadline, Some(budget), || {
            let run: Function = ctx.globals().get("__timo_run_factory")?;
            run.call((config_json,))
        })?;
        if let Some(error) = take_string(&ctx, "lastError") {
            return Err(runtime(ErrorCode::E202, error));
        }
        Ok(())
    })?;
    let loaded = {
        let state = state
            .lock()
            .map_err(|_| runtime(ErrorCode::E202, "extension state lock poisoned".to_string()))?;
        LoadedExtension {
            id: id.to_string(),
            tools: state
                .tools
                .iter()
                .filter(|tool| tool.ext_id == id)
                .cloned()
                .collect(),
            commands: state
                .commands
                .iter()
                .filter(|(ext, _)| ext == id)
                .map(|(_, name)| name.clone())
                .collect(),
            hooks: state
                .hooks
                .iter()
                .filter(|(ext, _)| ext == id)
                .map(|(_, event)| event.clone())
                .collect(),
        }
    };
    Ok((loaded, context))
}

pub(super) fn call_one(
    contexts: &[(String, Context)],
    state: &Arc<Mutex<HostState>>,
    name: &str,
    args: &JsonValue,
    deadline: &Rc<Cell<Option<Instant>>>,
) -> Result<ToolCallResult> {
    let meta = state
        .lock()
        .ok()
        .and_then(|state| state.tools.iter().find(|tool| tool.name == name).cloned())
        .ok_or_else(|| runtime(ErrorCode::E303, format!("tool not registered: {name}")))?;
    let (_, context) = contexts
        .iter()
        .find(|(id, _)| id == &meta.ext_id)
        .ok_or_else(|| {
            runtime(
                ErrorCode::E305,
                format!("extension not loaded: {}", meta.ext_id),
            )
        })?;
    let args_json =
        serde_json::to_string(args).map_err(|error| runtime(ErrorCode::E304, error.to_string()))?;
    context.with(|ctx| -> Result<ToolCallResult> {
        let _ = invoke(&ctx, deadline, None, || {
            let call: Function = ctx.globals().get("__timo_call_tool")?;
            call.call((meta.name.clone(), args_json))
        })?;
        if let Some(error) = take_string(&ctx, "lastError") {
            return Err(runtime(ErrorCode::E303, error));
        }
        let result = take_string(&ctx, "lastResult").unwrap_or_else(|| "null".to_string());
        serde_json::from_str(&result).map_err(|error| {
            runtime(
                ErrorCode::E304,
                format!("tool result is not a valid AgentToolResult: {error}"),
            )
        })
    })
}

pub(super) fn command_one(
    contexts: &[(String, Context)],
    state: &Arc<Mutex<HostState>>,
    name: &str,
    args: &JsonValue,
    deadline: &Rc<Cell<Option<Instant>>>,
    budget: Duration,
) -> Result<JsonValue> {
    let ext = state
        .lock()
        .ok()
        .and_then(|state| {
            state
                .commands
                .iter()
                .find(|(_, command)| command == name)
                .map(|(ext, _)| ext.clone())
        })
        .ok_or_else(|| runtime(ErrorCode::E303, format!("command not registered: {name}")))?;
    let (_, context) = contexts
        .iter()
        .find(|(id, _)| id == &ext)
        .ok_or_else(|| runtime(ErrorCode::E305, format!("extension not loaded: {ext}")))?;
    let args_json =
        serde_json::to_string(args).map_err(|error| runtime(ErrorCode::E304, error.to_string()))?;
    context.with(|ctx| -> Result<JsonValue> {
        let _ = invoke(&ctx, deadline, Some(budget), || {
            let run: Function = ctx.globals().get("__timo_run_command")?;
            run.call((name.to_string(), args_json))
        })?;
        if let Some(error) = take_string(&ctx, "lastError") {
            return Err(runtime(ErrorCode::E303, error));
        }
        Ok(JsonValue::Null)
    })
}

pub(super) fn route_one(
    contexts: &[(String, Context)],
    state: &Arc<Mutex<HostState>>,
    provider: &str,
    id: &str,
    payload: JsonValue,
    deadline: &Rc<Cell<Option<Instant>>>,
    budget: Duration,
) -> Result<JsonValue> {
    let ext = state
        .lock()
        .ok()
        .and_then(|state| {
            state
                .virtual_models
                .iter()
                .find(|model| model.provider == provider && model.id == id)
                .map(|model| model.ext_id.clone())
        })
        .ok_or_else(|| {
            runtime(
                ErrorCode::E303,
                format!("virtual model not registered: {provider}/{id}"),
            )
        })?;
    let (_, context) = contexts
        .iter()
        .find(|(loaded, _)| loaded == &ext)
        .ok_or_else(|| runtime(ErrorCode::E305, format!("extension not loaded: {ext}")))?;
    let payload_json = serde_json::to_string(&payload)
        .map_err(|error| runtime(ErrorCode::E304, error.to_string()))?;
    context.with(|ctx| -> Result<JsonValue> {
        let _ = invoke(&ctx, deadline, Some(budget), || {
            let call: Function = ctx.globals().get("__timo_route_virtual")?;
            call.call((provider.to_string(), id.to_string(), payload_json))
        })?;
        if let Some(error) = take_string(&ctx, "lastError") {
            return Err(runtime(ErrorCode::E303, error));
        }
        let result = take_string(&ctx, "lastResult").unwrap_or_else(|| "null".to_string());
        serde_json::from_str(&result).map_err(|error| {
            runtime(
                ErrorCode::E304,
                format!("virtual route result is not valid JSON: {error}"),
            )
        })
    })
}

pub(super) fn dispatch_all(
    contexts: &[(String, Context)],
    state: &Arc<Mutex<HostState>>,
    event: &str,
    payload: JsonValue,
    deadline: &Rc<Cell<Option<Instant>>>,
    budget: Duration,
) -> Result<JsonValue> {
    let mut current = payload;
    let mut merged = JsonValue::Null;
    let mut any = false;
    for (id, context) in contexts {
        let subscribed = state
            .lock()
            .map(|state| {
                state
                    .hooks
                    .iter()
                    .any(|(ext, name)| ext == id && name == event)
            })
            .unwrap_or(false);
        if !subscribed {
            continue;
        }
        any = true;
        let payload_json = serde_json::to_string(&current)
            .map_err(|error| runtime(ErrorCode::E304, error.to_string()))?;
        let result = match context.with(|ctx| -> Result<JsonValue> {
            let _ = invoke(&ctx, deadline, Some(budget), || {
                let dispatch: Function = ctx.globals().get("__timo_dispatch")?;
                dispatch.call((event.to_string(), payload_json))
            })?;
            if let Some(error) = take_string(&ctx, "lastError") {
                return Err(runtime(ErrorCode::E302, error));
            }
            let result = take_string(&ctx, "lastResult").unwrap_or_else(|| "null".to_string());
            serde_json::from_str(&result).map_err(|error| {
                runtime(
                    ErrorCode::E304,
                    format!("dispatch result is not valid JSON: {error}"),
                )
            })
        }) {
            Ok(value) => value,
            Err(error) => {
                if let Ok(mut state) = state.lock() {
                    state.diagnostics.push((
                        id.clone(),
                        DiagnosticEntry::new(
                            error.code().unwrap_or(ErrorCode::E302),
                            error.to_string(),
                        ),
                    ));
                }
                return Err(error);
            }
        };
        match event {
            "tool_call" => {
                if let Some(input) = result.get("input")
                    && let Some(object) = current.as_object_mut()
                {
                    object.insert("input".into(), input.clone());
                }
                if result.get("block").and_then(JsonValue::as_bool) == Some(true) {
                    merged = result;
                    break;
                }
                merged = result;
            }
            "tool_result" => {
                if let (Some(object), Some(patch)) = (current.as_object_mut(), result.as_object()) {
                    for (key, value) in patch {
                        object.insert(key.clone(), value.clone());
                    }
                }
                merged = current.clone();
            }
            "context" | "context_with_system" => {
                if let Some(messages) = result.get("messages")
                    && let Some(object) = current.as_object_mut()
                {
                    object.insert("messages".into(), messages.clone());
                }
                merged = current.clone();
            }
            "before_agent_start" => {
                if let Some(prompt) = result.get("systemPrompt")
                    && let Some(object) = current.as_object_mut()
                {
                    object.insert("systemPrompt".into(), prompt.clone());
                }
                merged = current.clone();
            }
            "project_trust" => {
                // 第一个 yes/no 拥有决定；undecided/null 继续。
                if matches!(
                    result.get("trusted").and_then(JsonValue::as_str),
                    Some("yes" | "no")
                ) {
                    merged = result;
                    break;
                }
            }
            _ => {
                if !result.is_null() {
                    merged = result;
                }
            }
        }
    }
    if !any {
        return Ok(JsonValue::Null);
    }
    if event == "tool_call"
        && let (Some(object), Some(input)) = (merged.as_object_mut(), current.get("input"))
    {
        object.insert("input".into(), input.clone());
    }
    Ok(merged)
}

/// 调用 JS 入口并把 Promise 收敛到完成；预算到点或取消时中断。
pub(super) fn invoke<'js>(
    ctx: &Ctx<'js>,
    deadline: &Rc<Cell<Option<Instant>>>,
    budget: Option<Duration>,
    call: impl FnOnce() -> rquickjs::Result<Value<'js>>,
) -> Result<String> {
    deadline.set(budget.map(|budget| Instant::now() + budget));
    let outcome = call();
    let value = match outcome {
        Ok(value) => value,
        Err(error) => {
            let expired = deadline.get().is_some_and(|at| Instant::now() >= at);
            deadline.set(None);
            return Err(if expired {
                runtime(ErrorCode::E301, "extension handler exceeded its budget")
            } else {
                js_error(error)
            });
        }
    };
    let settled = if value.is_promise() {
        let promise = value
            .into_promise()
            .ok_or_else(|| runtime(ErrorCode::E302, "value is not a promise"))?;
        loop {
            match promise.state() {
                PromiseState::Resolved => break,
                PromiseState::Rejected => {
                    deadline.set(None);
                    return Err(runtime(ErrorCode::E302, "extension promise rejected"));
                }
                PromiseState::Pending => {
                    if deadline.get().is_some_and(|at| Instant::now() >= at) {
                        deadline.set(None);
                        return Err(runtime(
                            ErrorCode::E301,
                            "extension handler exceeded its budget",
                        ));
                    }
                    if !ctx.execute_pending_job() {
                        deadline.set(None);
                        return Err(runtime(ErrorCode::E301, "extension promise did not settle"));
                    }
                }
            }
        }
        promise
            .result::<Value<'js>>()
            .ok_or_else(|| runtime(ErrorCode::E302, "promise has no result"))?
            .map_err(js_error)?
    } else {
        value
    };
    let json = match ctx.json_stringify(settled).map_err(js_error)? {
        Some(text) => text.to_string().map_err(js_error)?,
        None => "null".to_string(),
    };
    deadline.set(None);
    Ok(json)
}

fn take_string(ctx: &Ctx<'_>, field: &str) -> Option<String> {
    let state: Object = ctx.globals().get("__timo_state").ok()?;
    let value: Option<String> = state.get(field).ok().flatten();
    value.filter(|text| !text.is_empty())
}

fn js_error(error: rquickjs::Error) -> ExtError {
    runtime(ErrorCode::E302, format!("QuickJS error: {error}"))
}
