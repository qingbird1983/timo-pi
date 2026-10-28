//! 首次使用前的装配：MCP 枚举、扩展加载、提示与 agent（设计 06 / 08）。

use std::sync::Arc;

use timo_agent::{AgentHandle, KernelEvent, LoopConfig, ToolExecutionMode};

use super::Kernel;
use crate::extensions::{self, ExtensionOptions};
use crate::prompt;

impl Kernel {
    /// 首次使用前装齐 MCP 工具并构建 agent（bus/队列在 `new` 中已就位，
    /// 因此订阅先于首个事件）。
    pub(super) async fn ensure_ready(&mut self) {
        let ext_events = self.bus.subscribe();
        // 占位注册（`Hidden`）：让 `--tools codemode` 不被当成未知工具；allowlist 收敛后再按最终注册表重建。
        self.register_codemode(false);
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
        if self.ext.is_none() && self.extensions_enabled {
            let runtime = extensions::load_extensions(ExtensionOptions {
                data_dir: self.data_dir.clone(),
                work_dir: self.work_dir.clone(),
                explicit: self.extension_paths.clone(),
                interactive: self.extensions_interactive,
                include_project_pi: true,
                abort: Arc::clone(&self.ext_abort),
                running: Arc::clone(&self.ext_running),
                flags: self.ext_flags.clone(),
                models: self.extension_models(),
            })
            .await;
            if let Some(host) = &runtime.host {
                for meta in host.tools() {
                    self.registry
                        .register(extensions::tool_definition(&meta, Arc::clone(host)));
                }
                extensions::forward_events(ext_events, Arc::clone(host));
            }
            self.ext = Some(runtime);
        }
        // `--tools`：内置 + MCP + 扩展全部注册完再收敛声明面（设计 08 §4.2）。
        let stray = self
            .tool_allowlist
            .clone()
            .map(|allowlist| self.registry.apply_allowlist(&allowlist))
            .unwrap_or_default();
        self.apply_codemode_activation();
        if self.agent.is_none() {
            let (hooks, provider_hooks) =
                match self.ext.as_ref().and_then(|runtime| runtime.hooks.clone()) {
                    Some(hooks) => (
                        Some(Arc::clone(&hooks) as Arc<dyn timo_agent::LoopHooks>),
                        Some(hooks as Arc<dyn timo_ai::ProviderHooks>),
                    ),
                    None => (None, None),
                };
            let (system_prompt, skills, mut warnings) = self.build_prompt();
            for stray in &stray {
                warnings.push(prompt::ResourceWarning {
                    kind: prompt::WarningKind::UnknownTool,
                    message: format!("unknown tool `{stray}`"),
                    path: self.work_dir.clone(),
                });
            }
            self.prompt_warnings = warnings;
            self.skills = skills;
            let agent = AgentHandle::with_parts(
                LoopConfig {
                    model: self.resolved.model.clone(),
                    api: timo_ai::api_for(self.resolved.model.api),
                    tools: Arc::new(self.registry.clone()),
                    thinking_level: self.resolved.thinking,
                    auth: Arc::clone(&self.resolved.auth),
                    tool_execution: ToolExecutionMode::Parallel,
                    retry: self.retry.clone(),
                    cwd: Arc::from(self.work_dir.as_path()),
                    system_prompt: system_prompt.clone(),
                    clock: Arc::clone(&self.clock),
                    max_turns: self.max_turns,
                    compaction: Some(self.compaction.clone()),
                    hooks,
                    provider_hooks,
                    review: self.tool_review.clone(),
                    dispatch: Some(Arc::clone(&self.dispatch)),
                    route_state: Arc::clone(&self.route_state),
                },
                self.bus.clone(),
                Arc::clone(&self.steering),
                Arc::clone(&self.follow_up),
            );
            self.prompt_text = system_prompt;
            self.agent = Some(agent);
        }
    }
}
