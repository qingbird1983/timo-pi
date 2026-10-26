//! 注入 QuickJS 的宿主桥（设计 04 §1：扩展触达 OS/用户的唯一通道）。

use std::sync::{Arc, Mutex};

use rquickjs::{Ctx, Function, Object};
use serde_json::{Value as JsonValue, json};

use crate::diag::{DiagnosticEntry, ErrorCode};
use crate::host::{HostServices, HostState, ToolMeta};

pub(super) fn install<'js>(
    ctx: &Ctx<'js>,
    ext_id: &str,
    state: Arc<Mutex<HostState>>,
    services: Arc<dyn HostServices>,
) -> rquickjs::Result<()> {
    let native = Object::new(ctx.clone())?;
    install_registration(ctx, &native, ext_id, Arc::clone(&state))?;
    install_diagnostics(ctx, &native, Arc::clone(&state))?;
    install_user(ctx, &native, Arc::clone(&services))?;
    install_actions(ctx, &native, Arc::clone(&services))?;
    ctx.globals().set("__timo_native", native)?;
    Ok(())
}

fn install_registration<'js>(
    ctx: &Ctx<'js>,
    native: &Object<'js>,
    ext_id: &str,
    state: Arc<Mutex<HostState>>,
) -> rquickjs::Result<()> {
    {
        let state = Arc::clone(&state);
        let ext_id = ext_id.to_string();
        native.set(
            "toolRegistered",
            Function::new(ctx.clone(), move |json: String| {
                if let Ok(mut meta) = serde_json::from_str::<ToolMeta>(&json) {
                    meta.ext_id = ext_id.clone();
                    if let Ok(mut state) = state.lock() {
                        state.tools.push(meta);
                    }
                }
            })?,
        )?;
    }
    {
        let state = Arc::clone(&state);
        let ext_id = ext_id.to_string();
        native.set(
            "commandRegistered",
            Function::new(ctx.clone(), move |name: String| {
                if let Ok(mut state) = state.lock() {
                    state.commands.push((ext_id.clone(), name));
                }
            })?,
        )?;
    }
    {
        let state = Arc::clone(&state);
        let ext_id = ext_id.to_string();
        native.set(
            "hookRegistered",
            Function::new(ctx.clone(), move |name: String| {
                if let Ok(mut state) = state.lock() {
                    state.hooks.push((ext_id.clone(), name));
                }
            })?,
        )?;
    }
    Ok(())
}

fn install_diagnostics<'js>(
    ctx: &Ctx<'js>,
    native: &Object<'js>,
    state: Arc<Mutex<HostState>>,
) -> rquickjs::Result<()> {
    native.set(
        "diag",
        Function::new(ctx.clone(), move |code: String, message: String| {
            let code = ErrorCode::parse(&code).unwrap_or(ErrorCode::E302);
            if let Ok(mut state) = state.lock() {
                state.diagnostics.push(DiagnosticEntry::new(code, message));
            }
        })?,
    )?;
    Ok(())
}

fn install_user<'js>(
    ctx: &Ctx<'js>,
    native: &Object<'js>,
    services: Arc<dyn HostServices>,
) -> rquickjs::Result<()> {
    {
        let services = Arc::clone(&services);
        native.set(
            "notify",
            Function::new(ctx.clone(), move |message: String, kind: String| {
                services.notify(&message, &kind);
            })?,
        )?;
    }
    {
        let services = Arc::clone(&services);
        native.set(
            "select",
            Function::new(ctx.clone(), move |title: String, options: String| {
                let options: Vec<String> = serde_json::from_str(&options).unwrap_or_default();
                services.select(&title, &options)
            })?,
        )?;
    }
    {
        let services = Arc::clone(&services);
        native.set(
            "confirm",
            Function::new(ctx.clone(), move |title: String, message: String| {
                services.confirm(&title, &message)
            })?,
        )?;
    }
    {
        let services = Arc::clone(&services);
        native.set(
            "input",
            Function::new(ctx.clone(), move |title: String, placeholder: String| {
                services.input(&title, &placeholder)
            })?,
        )?;
    }
    Ok(())
}

fn install_actions<'js>(
    ctx: &Ctx<'js>,
    native: &Object<'js>,
    services: Arc<dyn HostServices>,
) -> rquickjs::Result<()> {
    {
        let services = Arc::clone(&services);
        native.set(
            "exec",
            Function::new(ctx.clone(), move |request: String| {
                bridge_json(execute_json(services.as_ref(), &request))
            })?,
        )?;
    }
    {
        let services = Arc::clone(&services);
        native.set(
            "fetch",
            Function::new(ctx.clone(), move |request: String| {
                let url = serde_json::from_str::<JsonValue>(&request)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("url")
                            .and_then(JsonValue::as_str)
                            .map(str::to_string)
                    })
                    .unwrap_or_default();
                match services.fetch(&url) {
                    Ok(out) => json!({
                        "ok": true,
                        "status": out.status,
                        "headers": out.headers,
                        "body": out.body,
                    })
                    .to_string(),
                    Err(error) => json!({"ok": false, "error": error}).to_string(),
                }
            })?,
        )?;
    }
    {
        let services = Arc::clone(&services);
        native.set(
            "action",
            Function::new(ctx.clone(), move |name: String, payload: String| {
                let payload: JsonValue = serde_json::from_str(&payload).unwrap_or(JsonValue::Null);
                bridge_json(services.action(&name, &payload))
            })?,
        )?;
    }
    {
        let services = Arc::clone(&services);
        native.set(
            "isAborted",
            Function::new(ctx.clone(), move || services.is_aborted())?,
        )?;
    }
    {
        let services = Arc::clone(&services);
        native.set(
            "abort",
            Function::new(ctx.clone(), move || services.abort())?,
        )?;
    }
    {
        let services = Arc::clone(&services);
        native.set(
            "isRunning",
            Function::new(ctx.clone(), move || services.is_running())?,
        )?;
    }
    native.set(
        "toolUpdate",
        Function::new(ctx.clone(), move |_json: String| {})?,
    )?;
    Ok(())
}

fn execute_json(services: &dyn HostServices, request: &str) -> JsonValue {
    let value: JsonValue = serde_json::from_str(request).unwrap_or(JsonValue::Null);
    let command = value
        .get("command")
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .to_string();
    let args: Vec<String> = value
        .get("args")
        .and_then(JsonValue::as_array)
        .map(|args| {
            args.iter()
                .filter_map(JsonValue::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let options = value.get("options").cloned().unwrap_or(JsonValue::Null);
    match services.exec(&command, &args, &options) {
        Ok(out) => json!({
            "ok": true,
            "stdout": out.stdout,
            "stderr": out.stderr,
            "code": out.code,
        }),
        Err(error) => json!({"ok": false, "error": error}),
    }
}

fn bridge_json(value: JsonValue) -> String {
    value.to_string()
}
