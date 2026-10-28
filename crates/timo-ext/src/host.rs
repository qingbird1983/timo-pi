//! QuickJS 扩展宿主（设计 01/04）：专用线程、Context 隔离、同步宿主桥。
//!
//! 线程模型：一条 OS 线程持有 `Runtime` 与每扩展一个 `Context`；内核经
//! 请求通道调用，宿主桥（exec/UI）在调用线程内同步完成。Promise 用
//! `execute_pending_job` 驱动，预算由 QuickJS 中断处理器切断。

mod api;
mod native;
mod runner;
mod services;
mod types;

pub use api::{DirItem, ExecOutput, ExecRequest, FetchOutput, Grants, HostServices};
pub use services::BasicServices;
pub use types::{LoadedExtension, ToolCallResult, ToolMeta, VirtualModelMeta};

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rquickjs::{Context, Runtime};
use serde_json::Value as JsonValue;

use crate::Result;
use crate::diag::{DiagnosticEntry, ErrorCode};
use crate::error::ExtError;
use runner::{call_one, command_one, dispatch_all, load_one, route_one};

#[derive(Default)]
pub(crate) struct HostState {
    pub(crate) tools: Vec<ToolMeta>,
    pub(crate) commands: Vec<(String, String)>,
    pub(crate) hooks: Vec<(String, String)>,
    pub(crate) virtual_models: Vec<VirtualModelMeta>,
    pub(crate) diagnostics: Vec<(String, DiagnosticEntry)>,
    pub(crate) system_prompt: String,
}

enum Request {
    Load {
        id: String,
        bundle: String,
        services: Arc<dyn HostServices>,
        reply: Sender<Result<LoadedExtension>>,
    },
    CallTool {
        name: String,
        args: JsonValue,
        cancel: Option<Arc<AtomicBool>>,
        reply: Sender<Result<ToolCallResult>>,
    },
    Dispatch {
        event: String,
        payload: JsonValue,
        reply: Sender<Result<JsonValue>>,
    },
    Command {
        name: String,
        args: JsonValue,
        reply: Sender<Result<JsonValue>>,
    },
    RouteVirtual {
        provider: String,
        id: String,
        payload: JsonValue,
        reply: Sender<Result<JsonValue>>,
    },
    Event {
        event: String,
        payload: JsonValue,
    },
    SetSystemPrompt(String),
    Shutdown,
}

/// 扩展宿主门面（Send；真正执行在专用线程）。
pub struct ExtensionHost {
    tx: Sender<Request>,
    state: Arc<Mutex<HostState>>,
    services: Arc<dyn HostServices>,
    join: Option<JoinHandle<()>>,
}

impl ExtensionHost {
    /// 启动宿主线程。
    ///
    /// # Errors
    /// 线程创建失败 → [`ExtError::Runtime`]。
    pub fn start(services: Arc<dyn HostServices>, budget: Duration) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(HostState::default()));
        let thread_state = Arc::clone(&state);
        let thread_services = Arc::clone(&services);
        let join = std::thread::Builder::new()
            .name("timo-ext".to_string())
            .spawn(move || actor(rx, thread_services, budget, thread_state))
            .map_err(|error| {
                runtime(
                    ErrorCode::E202,
                    format!("cannot spawn extension thread: {error}"),
                )
            })?;
        Ok(Self {
            tx,
            state,
            services,
            join: Some(join),
        })
    }

    /// 装载一个已打包扩展（`bundle` 为 CJS 源码路径）。
    ///
    /// # Errors
    /// 读文件失败/E202（工厂抛错）/E203（模块形状非法）。
    pub fn load(&self, id: &str, bundle: &Path) -> Result<LoadedExtension> {
        self.load_with(id, bundle, Arc::clone(&self.services))
    }

    /// 以扩展专属宿主服务装载（每扩展独立能力集）。
    ///
    /// # Errors
    /// 同 [`ExtensionHost::load`]。
    pub fn load_with(
        &self,
        id: &str,
        bundle: &Path,
        services: Arc<dyn HostServices>,
    ) -> Result<LoadedExtension> {
        let code = std::fs::read_to_string(bundle).map_err(|error| {
            runtime(
                ErrorCode::E202,
                format!("cannot read bundle {}: {error}", bundle.display()),
            )
        })?;
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(Request::Load {
                id: id.to_string(),
                bundle: code,
                services,
                reply,
            })
            .map_err(|_| runtime(ErrorCode::E305, "extension host is not running"))?;
        rx.recv()
            .map_err(|_| runtime(ErrorCode::E305, "extension host dropped the reply"))?
    }

    /// 执行一个已注册工具。
    ///
    /// # Errors
    /// 工具不存在/E303（JS 抛错）/E301（无法收敛）。
    pub fn call_tool(&self, name: &str, args: JsonValue) -> Result<ToolCallResult> {
        self.call_tool_cancellable(name, args, None)
    }

    /// 执行一个已注册工具；`cancel` 置位后 QuickJS 中断处理器结束本次执行。
    ///
    /// # Errors
    /// 工具不存在/E303（JS 抛错）/E301（无法收敛或取消）。
    pub fn call_tool_cancellable(
        &self,
        name: &str,
        args: JsonValue,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<ToolCallResult> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(Request::CallTool {
                name: name.to_string(),
                args,
                cancel,
                reply,
            })
            .map_err(|_| runtime(ErrorCode::E305, "extension host is not running"))?;
        rx.recv()
            .map_err(|_| runtime(ErrorCode::E305, "extension host dropped the reply"))?
    }

    /// 派发一个事件到所有订阅者。
    ///
    /// # Errors
    /// E301/E302（钩子失败或超时）。
    pub fn dispatch(&self, event: &str, payload: JsonValue) -> Result<JsonValue> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(Request::Dispatch {
                event: event.to_string(),
                payload,
                reply,
            })
            .map_err(|_| runtime(ErrorCode::E305, "extension host is not running"))?;
        rx.recv()
            .map_err(|_| runtime(ErrorCode::E305, "extension host dropped the reply"))?
    }

    /// 执行一个已注册命令（`/name args`）。
    ///
    /// # Errors
    /// 命令不存在/E303（JS 抛错）/E301（无法收敛）。
    pub fn run_command(&self, name: &str, args: JsonValue) -> Result<JsonValue> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(Request::Command {
                name: name.to_string(),
                args,
                reply,
            })
            .map_err(|_| runtime(ErrorCode::E305, "extension host is not running"))?;
        rx.recv()
            .map_err(|_| runtime(ErrorCode::E305, "extension host dropped the reply"))?
    }

    /// 调一个已注册虚拟模型的 JS 路由函数。
    ///
    /// # Errors
    /// 未注册/E303（JS 抛错）/E301（无法收敛）。
    pub fn route_virtual(&self, provider: &str, id: &str, payload: JsonValue) -> Result<JsonValue> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(Request::RouteVirtual {
                provider: provider.to_string(),
                id: id.to_string(),
                payload,
                reply,
            })
            .map_err(|_| runtime(ErrorCode::E305, "extension host is not running"))?;
        rx.recv()
            .map_err(|_| runtime(ErrorCode::E305, "extension host dropped the reply"))?
    }

    /// 当前已注册虚拟模型快照。
    #[must_use]
    pub fn virtual_models(&self) -> Vec<VirtualModelMeta> {
        self.state
            .lock()
            .map(|state| state.virtual_models.clone())
            .unwrap_or_default()
    }

    /// 是否有扩展订阅某事件。
    #[must_use]
    pub fn has_hook(&self, event: &str) -> bool {
        self.state
            .lock()
            .map(|state| state.hooks.iter().any(|(_, name)| name == event))
            .unwrap_or(false)
    }

    /// 投递一个通知事件（无订阅者时直接丢弃；不等待结果）。
    pub fn notify_event(&self, event: &str, payload: JsonValue) {
        if !self.has_hook(event) {
            return;
        }
        let _ = self.tx.send(Request::Event {
            event: event.to_string(),
            payload,
        });
    }

    /// 当前已注册工具快照。
    #[must_use]
    pub fn tools(&self) -> Vec<ToolMeta> {
        self.state
            .lock()
            .map(|state| state.tools.clone())
            .unwrap_or_default()
    }

    /// 当前已注册命令快照。
    #[must_use]
    pub fn commands(&self) -> Vec<String> {
        self.state
            .lock()
            .map(|state| {
                state
                    .commands
                    .iter()
                    .map(|(_, name)| name.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 指定扩展的诊断快照。
    #[must_use]
    pub fn diagnostics_for(&self, id: &str) -> Vec<DiagnosticEntry> {
        self.state
            .lock()
            .map(|state| {
                state
                    .diagnostics
                    .iter()
                    .filter(|(ext, _)| ext == id)
                    .map(|(_, entry)| entry.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 记录一条诊断（宿主侧主动上报，如事件超限）。
    pub fn note(&self, code: ErrorCode, message: impl Into<String>) {
        if let Ok(mut state) = self.state.lock() {
            state
                .diagnostics
                .push((String::new(), DiagnosticEntry::new(code, message)));
        }
    }

    /// 更新所有 Context 可见的系统提示（`ctx.getSystemPrompt`）。
    pub fn set_system_prompt(&self, prompt: impl Into<String>) {
        let prompt = prompt.into();
        if let Ok(mut state) = self.state.lock() {
            state.system_prompt.clone_from(&prompt);
        }
        let _ = self.tx.send(Request::SetSystemPrompt(prompt));
    }

    /// 当前生效系统提示。
    #[must_use]
    pub fn system_prompt(&self) -> String {
        self.state
            .lock()
            .map(|state| state.system_prompt.clone())
            .unwrap_or_default()
    }
}

impl Drop for ExtensionHost {
    fn drop(&mut self) {
        let _ = self.tx.send(Request::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn actor(
    rx: Receiver<Request>,
    services: Arc<dyn HostServices>,
    budget: Duration,
    state: Arc<Mutex<HostState>>,
) {
    let runtime = match Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            let message = format!("cannot create QuickJS runtime: {error}");
            for request in rx {
                fail_request(request, &message);
            }
            return;
        }
    };
    let deadline: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    let handler_deadline = Rc::clone(&deadline);
    let handler_services = Arc::clone(&services);
    // 当前工具调用的取消标志（per-call；普通 hook 派发为 None）。
    let call_cancel: Rc<RefCell<Option<Arc<AtomicBool>>>> = Rc::new(RefCell::new(None));
    let handler_cancel = Rc::clone(&call_cancel);
    runtime.set_interrupt_handler(Some(Box::new(move || {
        handler_services.is_aborted()
            || handler_cancel
                .borrow()
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Relaxed))
            || handler_deadline
                .get()
                .is_some_and(|at| Instant::now() >= at)
    })));
    let mut contexts: Vec<(String, Context)> = Vec::new();
    while let Ok(request) = rx.recv() {
        match request {
            Request::Load {
                id,
                bundle,
                services: ext_services,
                reply,
            } => {
                let result = load_one(
                    &runtime,
                    &id,
                    &bundle,
                    &deadline,
                    budget,
                    Arc::clone(&state),
                    ext_services,
                );
                match result {
                    Ok((loaded, context)) => {
                        contexts.push((id, context));
                        let _ = reply.send(Ok(loaded));
                    }
                    Err(error) => {
                        let _ = reply.send(Err(error));
                    }
                }
            }
            Request::CallTool {
                name,
                args,
                cancel,
                reply,
            } => {
                let cancelled = cancel.clone();
                *call_cancel.borrow_mut() = cancel;
                let mut result = call_one(&contexts, &state, &name, &args, &deadline);
                *call_cancel.borrow_mut() = None;
                if cancelled
                    .as_ref()
                    .is_some_and(|flag| flag.load(Ordering::Relaxed))
                    && result.is_err()
                {
                    result = Err(crate::host::runtime(
                        ErrorCode::E301,
                        "extension tool call cancelled",
                    ));
                }
                let _ = reply.send(result);
            }
            Request::Dispatch {
                event,
                payload,
                reply,
            } => {
                let result = dispatch_all(&contexts, &state, &event, payload, &deadline, budget);
                let _ = reply.send(result);
            }
            Request::Command { name, args, reply } => {
                let result = command_one(&contexts, &state, &name, &args, &deadline, budget);
                let _ = reply.send(result);
            }
            Request::RouteVirtual {
                provider,
                id,
                payload,
                reply,
            } => {
                let result = route_one(
                    &contexts, &state, &provider, &id, payload, &deadline, budget,
                );
                let _ = reply.send(result);
            }
            Request::Event { event, payload } => {
                let _ = dispatch_all(&contexts, &state, &event, payload, &deadline, budget);
            }
            Request::SetSystemPrompt(prompt) => {
                for (_, context) in &contexts {
                    let _ = context
                        .with(|ctx| ctx.globals().set("__timo_system_prompt", prompt.clone()));
                }
            }
            Request::Shutdown => break,
        }
    }
}

fn fail_request(request: Request, message: &str) {
    let error = || runtime(ErrorCode::E202, message.to_string());
    match request {
        Request::Load { reply, .. } => {
            let _ = reply.send(Err(error()));
        }
        Request::CallTool { reply, .. } => {
            let _ = reply.send(Err(error()));
        }
        Request::Dispatch { reply, .. } => {
            let _ = reply.send(Err(error()));
        }
        Request::Command { reply, .. } => {
            let _ = reply.send(Err(error()));
        }
        Request::RouteVirtual { reply, .. } => {
            let _ = reply.send(Err(error()));
        }
        Request::Event { .. } | Request::SetSystemPrompt(_) | Request::Shutdown => {}
    }
}

/// 构造带错误码的运行时错误（`E2xx`/`E301`/`E30x`）。
pub(crate) fn runtime(code: ErrorCode, message: impl Into<String>) -> ExtError {
    ExtError::Runtime {
        code,
        message: message.into(),
    }
}
