//! Kernel 门面（设计 06 §2）：装配 + run/resume/abort + 会话管理。
//!
//! MCP 惰性连接（设计 06 §1.2）：`new` 只读配置，首次 `run`/`tools` 才枚举并
//! 并入注册表；单 server 失败只发 `Error` 事件，不阻断内核启动。

use std::path::PathBuf;
use std::sync::Arc;

use timo_agent::{
    AgentHandle, EventBus, KernelEvent, PendingMessageQueue, SessionSink, ToolExecutionMode,
};
use timo_config::Clock;
use timo_session::{SessionInfo, SessionWriter, list_sessions, project_context, read_entries};
use timo_tools::{ToolDefinition, ToolRegistry};

use crate::Result;
use crate::error::KernelError;
use crate::prompt::build_system_prompt;
use crate::resolve::{ResolvedModel, load_catalog, resolve_model};

/// 门面装配选项（CLI/未来 desktop 的全部入口参数）。
#[derive(Debug, Clone, Default)]
pub struct KernelOptions {
    /// 数据目录覆盖（缺省 TIMO_AGENT_DIR > 平台目录）。
    pub data_dir: Option<PathBuf>,
    /// 工作目录（工具与项目设置解析基底）。
    pub work_dir: Option<PathBuf>,
    /// 模型覆盖（`--provider P --model M`）。
    pub model_override: Option<(String, String)>,
    /// 思考档位覆盖（CLI > 设置）。
    pub thinking: Option<timo_config::ThinkingLevel>,
    /// 显式凭证（`--api-key`）。
    pub api_key: Option<String>,
    /// 会话目录覆盖。
    pub session_dir: Option<PathBuf>,
    /// 单次 run 的模型请求轮数上限（`--max-turns`）。
    pub max_turns: Option<usize>,
}

/// 内核门面：薄壳唯一可见类型。
pub struct Kernel {
    work_dir: PathBuf,
    resolved: ResolvedModel,
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
}

impl Kernel {
    /// 计算会话目录（与 [`Kernel::new`] 同一推导；`sessions` 命令无需模型凭证）。
    ///
    /// # Errors
    /// 平台数据目录不可得 → [`KernelError::Config`]。
    pub fn sessions_dir(data_dir: Option<&std::path::Path>) -> Result<PathBuf> {
        match data_dir {
            Some(d) => Ok(d.join("sessions")),
            None => Ok(timo_config::sessions_dir(None)?),
        }
    }

    /// 扫描会话目录（不装配内核；修改时间倒序）。
    #[must_use]
    pub fn list_sessions_at(dir: &std::path::Path) -> Vec<SessionInfo> {
        list_sessions(dir)
    }

    /// 装配内核：配置 → 目录 → 模型解析 → 内置工具注册表。
    ///
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
        Ok(Self {
            work_dir,
            resolved,
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
        })
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

    /// 已启用的 MCP server 名单（纯配置，不建连）。
    #[must_use]
    pub fn mcp_servers(&self) -> Vec<String> {
        self.mcp.enabled_servers()
    }

    /// 对模型声明的工具清单快照（首次调用触发 MCP 连接与枚举）。
    pub async fn tools(&mut self) -> Vec<ToolDefinition> {
        self.ensure_ready().await;
        self.registry.declared().into_iter().cloned().collect()
    }

    /// 运行一次 prompt（首轮自动建会话；续轮沿用）。
    ///
    /// # Errors
    /// 会话创建失败 / run 前置失败 → [`KernelError`]。
    pub async fn run(&mut self, prompt: &str) -> Result<Vec<timo_ai::Message>> {
        self.ensure_ready().await;
        if self.session.is_none() {
            self.open_new_session()?;
        }
        let history = self.history.take().unwrap_or_default();
        let session = self.session.clone();
        let Some(agent) = self.agent.as_ref() else {
            return Err(KernelError::Agent(timo_agent::AgentError::Setup(
                "agent not assembled".into(),
            )));
        };
        Ok(agent.run(prompt.to_string(), history, session).await?)
    }

    /// 恢复会话（投影历史；后续 run 追加到该文件）。
    ///
    /// # Errors
    /// 会话不存在/损坏 → [`KernelError::Session`]。
    pub async fn resume(&mut self, id: &str) -> Result<()> {
        let path = self.sessions_dir.join(format!("{id}.jsonl"));
        let entries = read_entries(&path)?;
        let writer = SessionWriter::open(
            &path,
            Arc::clone(&self.clock),
            Arc::new(timo_session::IdGen::new()),
        )?;
        self.session_id = Some(id.to_string());
        self.history = Some(project_context(&entries));
        self.session = Some(Arc::new(tokio::sync::Mutex::new(writer)));
        Ok(())
    }

    /// 首次使用前装齐 MCP 工具并构建 agent（bus/队列在 `new` 中已就位，
    /// 因此订阅先于首个事件）。
    async fn ensure_ready(&mut self) {
        if !self.mcp_loaded {
            self.mcp_loaded = true;
            for server in self.mcp.enabled_servers() {
                match self.mcp.definitions(&server).await {
                    Ok(defs) => self.registry.register_all(defs),
                    Err(e) => self.bus.emit(KernelEvent::Error {
                        context: format!("mcp server `{server}`: {e}"),
                    }),
                }
            }
        }
        if self.agent.is_none() {
            let system_prompt = build_system_prompt(&self.work_dir, &self.registry);
            let agent = AgentHandle::with_parts(
                timo_agent::LoopConfig {
                    model: self.resolved.model.clone(),
                    api: timo_ai::api_for(self.resolved.model.api),
                    tools: Arc::new(self.registry.clone()),
                    thinking_level: self.resolved.thinking,
                    auth: Arc::clone(&self.resolved.auth),
                    tool_execution: ToolExecutionMode::Parallel,
                    retry: self.retry.clone(),
                    cwd: Arc::from(self.work_dir.as_path()),
                    system_prompt,
                    clock: Arc::clone(&self.clock),
                    max_turns: self.max_turns,
                },
                self.bus.clone(),
                Arc::clone(&self.steering),
                Arc::clone(&self.follow_up),
            );
            self.agent = Some(agent);
        }
    }

    fn open_new_session(&mut self) -> Result<()> {
        let writer = SessionWriter::create(
            &self.sessions_dir,
            &self.work_dir.display().to_string(),
            Some(&format!(
                "{}/{}",
                self.resolved.model.provider, self.resolved.model.id
            )),
            Arc::clone(&self.clock),
            Arc::new(timo_session::IdGen::new()),
        )?;
        self.session_id = Some(writer.session_id().to_string());
        self.session = Some(Arc::new(tokio::sync::Mutex::new(writer)));
        Ok(())
    }

    /// 当前会话 id（未开跑为 None）。
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// 中止当前 run。
    pub fn abort(&self) {
        if let Some(agent) = &self.agent {
            agent.abort();
        }
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

    /// 会话列表（修改时间倒序）。
    #[must_use]
    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        list_sessions(&self.sessions_dir)
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

    /// 思考档位。
    #[must_use]
    pub fn thinking_label(&self) -> &'static str {
        self.resolved.thinking.as_str()
    }

    /// 重试设置（诊断）。
    #[must_use]
    pub fn retry_settings(&self) -> &timo_config::RetrySettings {
        &self.retry
    }
}
