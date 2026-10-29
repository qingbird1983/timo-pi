//! 插件宿主 API 的内核侧服务（设计 04 §3）：模型快照与 `runTool` 代理。
//!
//! 桌面隔离层完成「名字/声明/参数」校验，这里补上注册表与挡位裁决，
//! 再走内核同一工具管线（审阅、取消、审计）。

use std::time::Duration;

use serde_json::Value;
use timo_kernel::Kernel;
use timo_kernel::plugins::PermissionTier;
use timo_kernel::plugins::host::{self, HostError, ModelSummary};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

/// 一次插件代理的工具调用。
pub struct ToolCall {
    /// 插件实际挡位。
    pub tier: PermissionTier,
    /// 清单声明的工具名。
    pub tools: Vec<String>,
    /// 目标工具。
    pub name: String,
    /// 参数对象。
    pub arguments: Value,
    /// 实例取消令牌（禁用/卸载时触发）。
    pub cancel: CancellationToken,
    /// 结果回执。
    pub reply: oneshot::Sender<Result<Value, HostError>>,
}

/// UI 发来的内核侧请求（独立于 `BridgeCommand`：不能放进带 `Clone` 的命令队列）。
pub enum PluginRequest {
    /// 模型快照（`models.list`）。
    Models(oneshot::Sender<Vec<ModelSummary>>),
    /// 代理工具执行（`actions.runTool`）。
    Tool(ToolCall),
}

/// 单次 `runTool` 的宿主等待上限（设计 03 §3）。
const TOOL_TIMEOUT: Duration = Duration::from_secs(30);

/// 服务一次请求；调用方已超时则跳过副作用。
pub async fn serve(kernel: &mut Kernel, request: PluginRequest) {
    match request {
        PluginRequest::Models(reply) => {
            if reply.is_closed() {
                return;
            }
            let models = kernel.plugin_models().await;
            let _ = reply.send(models);
        }
        PluginRequest::Tool(call) => {
            let ToolCall {
                tier,
                tools,
                name,
                arguments,
                cancel,
                reply,
            } = call;
            if reply.is_closed() {
                return;
            }
            let Some(hint) = kernel.plugin_tool_hint(&name) else {
                let _ = reply.send(Err(HostError::new(
                    "E740",
                    format!("tool `{name}` is not registered"),
                )));
                return;
            };
            let queries: [String; 0] = [];
            let actions: [String; 0] = [];
            let caller = host::Caller {
                tier,
                queries: &queries,
                actions: &actions,
                tools: &tools,
            };
            if !tools.iter().any(|declared| declared == &name) {
                let _ = reply.send(Err(HostError::new(
                    "E740",
                    format!("tool `{name}` is not listed in the manifest"),
                )));
                return;
            }
            let action = host::Action::RunTool {
                name: name.clone(),
                arguments: arguments.clone(),
            };
            if let Err(error) = host::authorize(&caller, &action, hint) {
                let _ = reply.send(Err(error));
                return;
            }
            let cancelled = cancel.clone();
            let outcome = tokio::time::timeout(
                TOOL_TIMEOUT,
                kernel.run_plugin_tool(&name, arguments, cancel),
            )
            .await;
            if cancelled.is_cancelled() {
                let _ = reply.send(Err(HostError::new("E742", "runTool cancelled")));
                return;
            }
            let result = match outcome {
                Ok(Ok(output)) => Ok(tool_payload(output)),
                Ok(Err(error)) => Err(HostError::new("E741", error.to_string())),
                Err(_) => Err(HostError::new("E742", "runTool timed out")),
            };
            let _ = reply.send(result);
        }
    }
}

/// `runTool` 的返回形状：`{content, isError}`（设计 04 §3）。
fn tool_payload(output: timo_kernel::ToolOutput) -> Value {
    let content = serde_json::to_value(&output.content).unwrap_or(Value::Null);
    serde_json::json!({ "content": content, "isError": output.is_error })
}
