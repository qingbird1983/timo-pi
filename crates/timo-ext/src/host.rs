//! QuickJS 扩展宿主（设计 01/04）：专用线程、Context 隔离、同步宿主桥。
//!
//! 线程模型：一条 OS 线程持有 `Runtime` 与每扩展一个 `Context`；内核经
//! 请求通道调用，宿主桥（exec/UI）在调用线程内同步完成。Promise 用
//! `execute_pending_job` 驱动，预算由 QuickJS 中断处理器切断。

mod native;
mod runner;
mod services;

pub use services::BasicServices;

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rquickjs::{Context, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

use crate::Result;
use crate::diag::{DiagnosticEntry, ErrorCode};
use crate::error::ExtError;
use runner::{call_one, dispatch_all, load_one};

/// 已授予能力（v1 子集；完整规则见设计 03）。
#[derive(Debug, Clone, Default)]
pub struct Grants {
    /// 是否允许 `pi.exec`。
    pub exec: bool,
    /// 允许出网的域名（空 = 无网）。
    pub net_domains: Vec<String>,
}

/// 子进程执行结果。
#[derive(Debug, Clone, Serialize)]
pub struct ExecOutput {
    /// stdout。
    pub stdout: String,
    /// stderr。
    pub stderr: String,
    /// 退出码。
    pub code: i32,
}

/// HTTP 获取结果。
#[derive(Debug, Clone, Serialize)]
pub struct FetchOutput {
    /// 状态码。
    pub status: u16,
    /// 响应头。
    pub headers: Vec<(String, String)>,
    /// 响应正文（文本）。
    pub body: String,
}

/// 宿主服务：扩展触达 OS/用户的唯一通道（由 CLI 或测试实现）。
pub trait HostServices: Send + Sync {
    /// 运行子进程；未授权返回含 `E401` 的错误文案。
    fn exec(
        &self,
        command: &str,
        args: &[String],
        options: &JsonValue,
    ) -> std::result::Result<ExecOutput, String>;
    /// 发起 HTTP 请求（默认未实现）。
    fn fetch(&self, url: &str) -> std::result::Result<FetchOutput, String> {
        let _ = url;
        Err("E401 network capability is not available".to_string())
    }
    /// 用户通知。
    fn notify(&self, message: &str, kind: &str);
    /// 选择对话框；无 UI 返回 `None`。
    fn select(&self, title: &str, options: &[String]) -> Option<String>;
    /// 确认对话框；无 UI 返回 false。
    fn confirm(&self, title: &str, message: &str) -> bool;
    /// 文本输入；无 UI 返回 `None`。
    fn input(&self, title: &str, placeholder: &str) -> Option<String>;
    /// `pi.*` 动作（getSettings/getAllTools/...）。
    fn action(&self, name: &str, payload: &JsonValue) -> JsonValue {
        let _ = (name, payload);
        JsonValue::Null
    }
    /// 传给 JS 的上下文配置（cwd/hasUI/model/...）。
    fn config(&self) -> JsonValue {
        json!({})
    }
    /// 当前 run 是否已取消。
    fn is_aborted(&self) -> bool {
        false
    }
    /// 请求取消当前 run。
    fn abort(&self) {}
    /// 是否有 run 在进行。
    fn is_running(&self) -> bool {
        false
    }
}

/// 工具元数据（注册时由 JS 上报；`name` 已含命名空间前缀）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolMeta {
    /// 模型可见名（`plugin_<id>_<name>`）。
    pub name: String,
    /// 扩展内原始名。
    pub js_name: String,
    /// 人读标签。
    #[serde(default)]
    pub label: String,
    /// 模型描述。
    #[serde(default)]
    pub description: String,
    /// 参数 JSON Schema。
    #[serde(default)]
    pub parameters: JsonValue,
    /// 系统提示片段。
    #[serde(default)]
    pub prompt_snippet: Option<String>,
    /// 系统提示指引。
    #[serde(default)]
    pub prompt_guidelines: Vec<String>,
    /// 结构化输出 schema。
    #[serde(default)]
    pub output_schema: Option<JsonValue>,
    /// 暴露面。
    #[serde(default = "default_exposure")]
    pub exposure: String,
    /// 注解。
    #[serde(default)]
    pub annotations: Option<JsonValue>,
    /// 执行模式。
    #[serde(default)]
    pub execution_mode: Option<String>,
    /// 所属扩展（宿主填写，不由 JS 传入）。
    #[serde(skip)]
    pub ext_id: String,
}

fn default_exposure() -> String {
    "direct".to_string()
}

/// 一次工具执行的结果（JS `AgentToolResult` 的 JSON 形态）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolCallResult {
    /// 模型可见内容块。
    pub content: JsonValue,
    /// UI/日志细节。
    pub details: JsonValue,
    /// 结构化输出。
    pub structured_content: Option<JsonValue>,
    /// 是否错误结果。
    pub is_error: bool,
    /// 本批早停提示。
    pub terminate: bool,
}

/// 单扩展装载结果。
#[derive(Debug, Clone)]
pub struct LoadedExtension {
    /// 扩展 id。
    pub id: String,
    /// 注册的工具。
    pub tools: Vec<ToolMeta>,
    /// 注册的命令（已含前缀）。
    pub commands: Vec<String>,
    /// 订阅的事件。
    pub hooks: Vec<String>,
}

#[derive(Default)]
pub(crate) struct HostState {
    pub(crate) tools: Vec<ToolMeta>,
    pub(crate) commands: Vec<(String, String)>,
    pub(crate) hooks: Vec<(String, String)>,
    pub(crate) diagnostics: Vec<DiagnosticEntry>,
}

enum Request {
    Load {
        id: String,
        bundle: String,
        reply: Sender<Result<LoadedExtension>>,
    },
    CallTool {
        name: String,
        args: JsonValue,
        reply: Sender<Result<ToolCallResult>>,
    },
    Dispatch {
        event: String,
        payload: JsonValue,
        reply: Sender<Result<JsonValue>>,
    },
    Shutdown,
}

/// 扩展宿主门面（Send；真正执行在专用线程）。
pub struct ExtensionHost {
    tx: Sender<Request>,
    state: Arc<Mutex<HostState>>,
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
        let join = std::thread::Builder::new()
            .name("timo-ext".to_string())
            .spawn(move || actor(rx, services, budget, thread_state))
            .map_err(|error| {
                runtime(
                    ErrorCode::E202,
                    format!("cannot spawn extension thread: {error}"),
                )
            })?;
        Ok(Self {
            tx,
            state,
            join: Some(join),
        })
    }

    /// 装载一个已打包扩展（`bundle` 为 CJS 源码路径）。
    ///
    /// # Errors
    /// 读文件失败/E202（工厂抛错）/E203（模块形状非法）。
    pub fn load(&self, id: &str, bundle: &Path) -> Result<LoadedExtension> {
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
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(Request::CallTool {
                name: name.to_string(),
                args,
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

    /// 当前诊断快照。
    #[must_use]
    pub fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        self.state
            .lock()
            .map(|state| state.diagnostics.clone())
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
    runtime.set_interrupt_handler(Some(Box::new(move || {
        handler_services.is_aborted()
            || handler_deadline
                .get()
                .is_some_and(|at| Instant::now() >= at)
    })));
    let mut contexts: Vec<(String, Context)> = Vec::new();
    while let Ok(request) = rx.recv() {
        match request {
            Request::Load { id, bundle, reply } => {
                let result = load_one(
                    &runtime,
                    &id,
                    &bundle,
                    &deadline,
                    budget,
                    Arc::clone(&state),
                    Arc::clone(&services),
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
            Request::CallTool { name, args, reply } => {
                let result = call_one(&contexts, &state, &name, &args, &deadline);
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
        Request::Shutdown => {}
    }
}

/// 构造带错误码的运行时错误（`E2xx`/`E301`/`E30x`）。
pub(crate) fn runtime(code: ErrorCode, message: impl Into<String>) -> ExtError {
    ExtError::Runtime {
        code,
        message: message.into(),
    }
}
