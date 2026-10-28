//! Kernel 门面（设计 06 §2）：装配 + run/resume/abort + 会话管理。
//!
//! MCP 惰性连接（设计 06 §1.2）：`new` 只读配置，首次 `run`/`tools` 才枚举并
//! 并入注册表；单 server 失败只发 `Error` 事件，不阻断内核启动。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use timo_agent::{AgentHandle, EventBus, KernelEvent, PendingMessageQueue, SessionSink};
use timo_config::Clock;
use timo_tools::{ToolDefinition, ToolRegistry};

use crate::Result;
use crate::error::KernelError;
use crate::extensions::ExtensionRuntime;
use crate::prompt;
use crate::resolve::{CatalogDispatch, ResolvedModel, load_catalog, resolve_model};

mod cache_api;
mod codemode_api;
mod compaction_api;
mod ext_api;
mod options;
mod prompt_api;
mod ready;
mod sessions;
mod usage_api;
mod warm_api;

pub use options::KernelOptions;

/// 内核门面：薄壳唯一可见类型。
pub struct Kernel {
    work_dir: PathBuf,
    resolved: ResolvedModel,
    /// 装配期目录 + 请求期模型装配（扩展目录快照 / 路由换模型）。
    catalog: timo_ai::Catalog,
    dispatch: Arc<dyn timo_agent::ModelDispatch>,
    /// 虚拟模型路由状态（会话分支的当前值；run 内由 agent 更新）。
    route_state: Arc<std::sync::Mutex<Option<serde_json::Value>>>,
    /// 模型是否由命令行显式指定（显式优先于会话恢复，设计 10 §4.2）。
    selection_explicit: bool,
    bus: EventBus,
    mcp: Arc<timo_mcp::McpRuntime>,
    sessions_dir: PathBuf,
    session: SessionSink,
    session_id: Option<String>,
    history: Option<Vec<timo_ai::Message>>,
    clock: Arc<dyn Clock>,
    retry: timo_config::RetrySettings,
    registry: ToolRegistry,
    steering: Arc<PendingMessageQueue>,
    follow_up: Arc<PendingMessageQueue>,
    agent: Option<AgentHandle>,
    mcp_loaded: bool,
    max_turns: Option<usize>,
    data_dir: PathBuf,
    extension_paths: Vec<PathBuf>,
    ext_flags: std::collections::BTreeMap<String, String>,
    extensions_enabled: bool,
    extensions_interactive: bool,
    ext_abort: Arc<AtomicBool>,
    ext_running: Arc<AtomicBool>,
    ext: Option<ExtensionRuntime>,
    tool_review: Option<Arc<dyn timo_tools::ToolReview>>,
    /// 不落会话文件（`--no-session`）。
    no_session: bool,
    /// 工具允许列表（`--tools`）；`None` = 不过滤。
    tool_allowlist: Option<Vec<String>>,
    /// 设置里的默认激活集是否包含 `codemode`（`--tools` 可另行激活）。
    codemode_active: bool,
    /// 追加到系统提示尾部的命令行文本（`--append-system-prompt`）。
    append_prompt: Vec<String>,
    /// 当前渲染的系统提示（装配后有效）。
    prompt_text: String,
    /// 上下文装载告警。
    prompt_warnings: Vec<prompt::ResourceWarning>,
    /// 已发现技能（每次提示装配刷新）。
    skills: Vec<prompt::Skill>,
    /// 设置里的显式技能路径。
    skill_paths: Vec<String>,
    /// 技能是否进入命令发现。
    skill_commands_enabled: bool,
    /// 已装载的 prompt 模板（设计 09；首次装配与 `/reload` 刷新）。
    templates: Vec<prompt::templates::Template>,
    /// 设置与命令行里的显式模板路径。
    template_paths: Vec<String>,
    /// 是否发现默认模板目录（`--no-prompt-templates` 只关这一半，09 §2）。
    template_defaults: bool,
    /// 自动压缩设置（settings.compaction）。
    compaction: timo_config::CompactionSettings,
    /// 缓存预热器（settings.cache_warming）。
    warmer: crate::warmer::CacheWarmer,
}

impl Kernel {
    /// 装配内核：配置 → 目录 → 模型解析 → 内置工具注册表。    ///
    /// MCP 仅读配置（不连接）；agent 在首次 [`Kernel::run`]/[`Kernel::tools`] 时装配。
    ///
    /// # Errors
    /// 配置缺失/非法、模型解析失败 → [`KernelError`]。
    pub async fn new(options: KernelOptions) -> Result<Self> {
        let (settings_path, models_path, mcp_path) = match &options.data_dir {
            Some(dir) => (
                dir.join("settings.json"),
                dir.join("models.json"),
                dir.join("mcp.json"),
            ),
            None => (
                timo_config::settings_path()?,
                timo_config::models_path()?,
                timo_config::mcp_path()?,
            ),
        };
        let data_dir = match &options.data_dir {
            Some(dir) => dir.clone(),
            None => timo_config::agent_dir()?,
        };
        let default_sessions = Self::sessions_dir(options.data_dir.as_deref())?;
        let work_dir = options
            .work_dir
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let settings = timo_config::load_settings(
            &settings_path,
            &timo_config::project_settings_path(&work_dir),
        )?;
        let custom = timo_config::load_custom_providers(&models_path)?;
        let catalog = load_catalog(&custom)?;
        let selection_explicit = options.model_override.is_some();
        let dispatch: Arc<dyn timo_agent::ModelDispatch> = Arc::new(CatalogDispatch::new(
            catalog.clone(),
            custom.clone(),
            options.api_key.clone(),
        ));
        let resolved = resolve_model(
            &catalog,
            &settings,
            options.model_override.as_ref(),
            options.api_key.as_deref(),
            &custom,
        )
        .await?;
        let resolved = match options.thinking {
            Some(t) => ResolvedModel {
                thinking: t,
                ..resolved
            },
            None => resolved,
        };
        let sessions_dir = match options.session_dir.as_deref() {
            Some(d) => PathBuf::from(d),
            None => default_sessions,
        };
        let skill_paths = settings.skills.clone();
        let skill_commands_enabled = settings.enable_skill_commands;
        // 显式模板路径：设置在前、命令行在后；`-np` 只关默认目录（09 §2）。
        let mut template_paths = settings.prompts.clone();
        template_paths.extend(options.prompt_template_paths.clone());
        let template_defaults = settings.enable_prompt_templates && !options.no_prompt_templates;
        // 工具注册表：内置（默认激活集 + default_tools 修饰；未激活 = Hidden）
        let active = timo_config::resolve_default_tools(
            &timo_tools::tools::DEFAULT_ACTIVE
                .iter()
                .map(|s| (*s).to_string())
                .collect::<Vec<_>>(),
            &settings.default_tools,
        );
        let mut registry = ToolRegistry::new();
        for def in timo_tools::tools::builtin_definitions(&work_dir) {
            let mut def = def;
            if !active.contains(&def.name) {
                def.exposure = timo_tools::Exposure::Hidden;
            }
            registry.register(def);
        }
        // MCP：只读配置；连接延后到首次枚举（设计 06 §1.2）
        let mcp_entries =
            timo_mcp::load_server_registry(&mcp_path, &work_dir.join(".timo").join("mcp.json"))?;
        let mcp = timo_mcp::McpRuntime::new(mcp_entries).shared();
        let clock: Arc<dyn Clock> = Arc::new(timo_config::SystemClock);
        let mut kernel = Self {
            work_dir,
            resolved,
            catalog,
            dispatch,
            route_state: Arc::new(std::sync::Mutex::new(None)),
            selection_explicit,
            bus: EventBus::new(),
            mcp,
            sessions_dir,
            session: None,
            session_id: None,
            history: None,
            clock,
            retry: settings.retry,
            registry,
            steering: Arc::new(PendingMessageQueue::new()),
            follow_up: Arc::new(PendingMessageQueue::new()),
            agent: None,
            mcp_loaded: false,
            max_turns: options.max_turns,
            data_dir,
            extension_paths: options.extension_paths,
            ext_flags: options.ext_flags.into_iter().collect(),
            extensions_enabled: !options.no_extensions,
            extensions_interactive: options.extensions_interactive.unwrap_or_else(|| {
                use std::io::IsTerminal;
                std::io::stdin().is_terminal()
            }),
            ext_abort: Arc::new(AtomicBool::new(false)),
            ext_running: Arc::new(AtomicBool::new(false)),
            ext: None,
            tool_review: options.tool_review,
            no_session: options.no_session,
            tool_allowlist: options.tool_allowlist,
            codemode_active: active.iter().any(|name| name == "codemode"),
            append_prompt: options.append_system_prompt,
            prompt_text: String::new(),
            prompt_warnings: Vec::new(),
            skills: Vec::new(),
            skill_paths,
            skill_commands_enabled,
            templates: Vec::new(),
            template_paths,
            template_defaults,
            compaction: settings.compaction,
            warmer: crate::warmer::CacheWarmer::new(settings.cache_warming),
        };
        // 模板在装配期就装载：`/模板名` 在首轮输入也要能认出来（09 G5）。
        let template_warnings = kernel.load_templates();
        kernel.prompt_warnings.extend(template_warnings);
        Ok(kernel)
    }

    /// 事件总线（必须在 run 前订阅以接收全程事件）。
    #[must_use]
    pub fn events(&self) -> tokio::sync::broadcast::Receiver<KernelEvent> {
        self.bus.subscribe()
    }

    /// MCP 运行时状态（诊断；查询本身不建连）。
    pub async fn mcp_state(&self, server: &str) -> timo_mcp::ServerState {
        self.mcp.state(server).await
    }

    /// 对模型声明的工具清单快照（首次调用触发 MCP 连接与枚举）。
    pub async fn tools(&mut self) -> Vec<ToolDefinition> {
        self.ensure_ready().await;
        self.registry.declared().into_iter().cloned().collect()
    }

    /// 运行一次 prompt（首轮自动建会话；续轮沿用）；`/skill:name` 在这里统一展开。
    ///
    /// # Errors
    /// 会话创建失败 / run 前置失败 → [`KernelError`]。
    pub async fn run(&mut self, prompt: &str) -> Result<Vec<timo_ai::Message>> {
        // 新 run 会改变上下文：旧预热定时器立即作废，收尾时按新上下文重排。
        self.warmer.cancel("context changed");
        self.ensure_ready().await;
        let prompt = self.expand_input(prompt);
        self.ensure_session()?;
        self.ext_abort.store(false, Ordering::SeqCst);
        self.ext_running.store(true, Ordering::SeqCst);
        // 克隆而不是 take：run 前置失败（空 prompt / 已有 run）时不能丢掉已恢复的历史。
        let history = self.history.clone().unwrap_or_default();
        let session = self.session.clone();
        let Some(agent) = self.agent.as_ref() else {
            return Err(KernelError::Agent(timo_agent::AgentError::Setup(
                "agent not assembled".into(),
            )));
        };
        let out = agent.run_with_context(prompt, history, session).await;
        if let Some(runtime) = &mut self.ext {
            runtime.absorb_runtime_errors();
        }
        self.ext_running.store(false, Ordering::SeqCst);
        self.ext_abort.store(false, Ordering::SeqCst);
        let mut outcome = out?;
        if let Some(request) = outcome.last_request.take() {
            self.arm_warmer(request, &outcome.new_messages);
        }
        Ok(self.commit_outcome(outcome))
    }

    /// 把一次 run 的完整上下文写回 history，返回本次新增消息。
    fn commit_outcome(&mut self, outcome: timo_agent::RunOutcome) -> Vec<timo_ai::Message> {
        self.history = Some(outcome.context);
        outcome.new_messages
    }

    /// 当前会话 id（未开跑为 None）。
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// 立即创建并落盘新会话（首次 [`Kernel::run`] 原本也会建；无头模式需要先拿 id 输出会话头）。
    ///
    /// # Errors
    /// 会话文件创建失败 → [`KernelError::Session`]。
    pub fn start_session(&mut self) -> Result<()> {
        self.ensure_session()
    }

    /// 会话闸门：`--no-session` 时 run 与 `start_session` 都不落任何文件（设计 08 §4.2）。
    ///
    /// # Errors
    /// 会话文件创建失败 → [`KernelError::Session`]。
    fn ensure_session(&mut self) -> Result<()> {
        if self.session.is_none() && !self.no_session {
            self.open_new_session()?;
        }
        Ok(())
    }

    /// 中止当前 run。
    pub fn abort(&self) {
        self.ext_abort.store(true, Ordering::SeqCst);
        if let Some(agent) = &self.agent {
            agent.abort();
        }
    }

    /// 扩展取消标志（CLI 在 run 借出期间先置位再调 abort 句柄）。
    #[must_use]
    pub fn abort_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.ext_abort)
    }

    /// 中止句柄（首次 [`Kernel::tools`] 装配 agent 后可取；供 run 借出期间拦截信号）。
    #[must_use]
    pub fn abort_handle(&self) -> Option<timo_agent::AbortHandle> {
        self.agent
            .as_ref()
            .map(timo_agent::AgentHandle::abort_handle)
    }

    /// 等待当前 run 结束（无 run 时立即返回）。
    pub async fn wait_idle(&self) {
        if let Some(agent) = &self.agent {
            agent.wait_idle().await;
        }
    }

    /// steering 注入（当前轮工具批结束后、下轮请求前）。
    pub fn steer(&self, text: &str) {
        self.steering
            .push(timo_ai::user_text(text, self.clock.now_ms()));
    }

    /// follow-up 注入（自然停止前）。
    pub fn follow_up(&self, text: &str) {
        self.follow_up
            .push(timo_ai::user_text(text, self.clock.now_ms()));
    }

    /// 对模型声明的工具名快照（不触发 MCP 连接；连接后语义见 [`Kernel::tools`]）。
    #[must_use]
    pub fn declared_tools(&self) -> Vec<String> {
        self.registry
            .declared()
            .into_iter()
            .map(|d| d.name.clone())
            .collect()
    }

    /// 解析后的模型描述（`provider/id`）。
    #[must_use]
    pub fn model_label(&self) -> String {
        format!(
            "{}/{}",
            self.resolved.model.provider, self.resolved.model.id
        )
    }

    /// 当前分支的虚拟模型路由状态（无则 None；设计 10 §4.2）。
    #[must_use]
    pub fn route_state(&self) -> Option<serde_json::Value> {
        self.route_state.lock().ok().and_then(|state| state.clone())
    }

    /// 思考档位。
    #[must_use]
    pub fn thinking_label(&self) -> &'static str {
        self.resolved.thinking.as_str()
    }
}

#[cfg(test)]
#[path = "kernel/kernel_tests.rs"]
mod tests;
