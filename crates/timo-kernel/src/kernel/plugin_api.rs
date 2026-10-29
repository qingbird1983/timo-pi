//! 插件宿主 API 的内核执行面（设计 04 §3）：模型快照与 `runTool` 代理。
//!
//! 桌面隔离层把校验过的调用送到这里，工具仍走同一注册表、审阅与取消管线。

use std::sync::Arc;

use serde_json::Value;
use timo_tools::ToolOutput;
use tokio_util::sync::CancellationToken;

use super::Kernel;
use crate::Result;
use crate::plugins::host::{self, ModelSummary};

impl Kernel {
    /// 插件可见的模型快照（不含密钥与 base URL）。
    pub async fn plugin_models(&mut self) -> Vec<ModelSummary> {
        self.ensure_ready().await;
        host::list_models(&self.catalog)
    }

    /// 工具的只读注解（未注册 → `None`，由宿主按 E740 拒绝）。
    #[must_use]
    pub fn plugin_tool_hint(&self, name: &str) -> Option<Option<bool>> {
        self.registry
            .get(name)
            .map(|definition| definition.annotations.read_only_hint)
    }

    /// 经内核工具管线执行插件代理的 `runTool`。
    ///
    /// # Errors
    /// 工具未注册、参数 schema 失败或执行错误 → [`crate::KernelError`]。
    pub async fn run_plugin_tool(
        &mut self,
        name: &str,
        arguments: Value,
        cancel: CancellationToken,
    ) -> Result<ToolOutput> {
        self.ensure_ready().await;
        let (updates, mut progress) = tokio::sync::mpsc::channel(16);
        // 插件调用方不消费进度：排空以免工具的 `send` 背压卡住。
        tokio::spawn(async move { while progress.recv().await.is_some() {} });
        let ctx = timo_tools::ToolCallCtx {
            tool_call_id: format!("plugin-{}", self.clock.now_ms()),
            params: arguments,
            cancel,
            updates,
            cwd: Arc::from(self.work_dir.as_path()),
            review: self.tool_review.clone(),
            nested: None,
        };
        Ok(self.registry.execute(name, ctx).await?)
    }
}
