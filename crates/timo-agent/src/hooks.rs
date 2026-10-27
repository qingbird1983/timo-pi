//! 循环钩子（设计 04 §2.1）：扩展宿主注入的请求-响应接缝。
//!
//! 循环只认识本 trait，不认识"扩展"（接缝 #2）。所有方法默认空实现；
//! `LoopConfig.hooks = None` 时行为与 Phase 1 逐字节一致。

use async_trait::async_trait;
use serde_json::Value;
use timo_ai::{Content, Message, Model};
use timo_config::ThinkingLevel;
use timo_tools::ToolOutput;

/// run 开始前的请求（`AgentStarted` 之前）。
pub struct BeforeAgentStart<'a> {
    /// 本次 run 的初始提示消息。
    pub prompt: &'a [Message],
    /// 当前系统提示。
    pub system_prompt: &'a str,
}

/// 路由原因（设计 10 §4.2，对齐蓝本 `ModelRouteReason`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteReason {
    /// 用户消息（prompt / steering / follow-up）之后的首次请求。
    User,
    /// 工具结果或扩展消息之后的续轮。
    Continuation,
    /// 失败后的自动重试（含压缩后重试）。
    Retry,
    /// 循环外的旁路请求（压缩摘要等）。
    Direct,
}

impl RouteReason {
    /// JS 侧规范名（蓝本同值）。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Continuation => "continuation",
            Self::Retry => "retry",
            Self::Direct => "direct",
        }
    }
}

/// 虚拟模型路由请求（设计 10 §4.2）。
pub struct RouteRequest<'a> {
    /// 选中的模型（可为虚拟；请求模型由路由决定）。
    pub model: &'a Model,
    /// 选中的思考档位（虚拟档位，语义由路由决定）。
    pub thinking: ThinkingLevel,
    /// 本请求的原因。
    pub reason: RouteReason,
    /// `messages` 里最近一次成功响应的物理模型与档位。
    pub previous: Option<(&'a Model, ThinkingLevel)>,
    /// 失败的那次请求（`reason = Retry`）；路由自身失败时为 None。
    pub failed: Option<(&'a Model, ThinkingLevel)>,
    /// 本分支上次返回的路由状态（`Direct` 不传）。
    pub state: Option<&'a serde_json::Value>,
    /// 将发送的上下文（不含 system）。
    pub messages: &'a [Message],
}

/// 路由结果：本请求使用的物理模型与档位。
pub struct RouteChoice {
    /// 物理模型（内核按 `provider/id` 再核对目录）。
    pub model: Model,
    /// 本请求的思考档位。
    pub thinking: ThinkingLevel,
    /// 新的路由状态（`None` = 保持不变；`Direct` 忽略）。
    pub state: Option<serde_json::Value>,
}

/// 工具调用前的裁决（pi `tool_call` 事件）。
#[derive(Debug, Clone, Default)]
pub struct BeforeToolCall {
    /// 非空即否决；值为给模型的理由。
    pub block: Option<String>,
    /// 替换后的参数（改参）。
    pub args: Option<Value>,
    /// 本批早停信号。
    pub terminate: bool,
}

/// 工具执行结果快照（`after_tool_call` 入参）。
pub struct ToolOutcome<'a> {
    /// 工具调用 id。
    pub tool_call_id: &'a str,
    /// 工具名（含命名空间前缀）。
    pub tool_name: &'a str,
    /// 执行结果。
    pub output: &'a ToolOutput,
}

/// 结果字段级覆盖；`None` 表示保留原值。
#[derive(Debug, Clone, Default)]
pub struct ToolOutputPatch {
    /// 替换模型可见内容。
    pub content: Option<Vec<Content>>,
    /// 替换 UI/日志细节。
    pub details: Option<Value>,
    /// 替换结构化输出（`Some(None)` = 清空）。
    pub structured_content: Option<Option<Value>>,
    /// 替换错误标记。
    pub is_error: Option<bool>,
    /// 替换早停信号。
    pub terminate: Option<bool>,
}

impl ToolOutputPatch {
    /// 把补丁写回结果。
    pub fn apply(self, output: &mut ToolOutput) {
        if let Some(content) = self.content {
            output.content = content;
        }
        if let Some(details) = self.details {
            output.details = details;
        }
        if let Some(structured) = self.structured_content {
            output.structured_content = structured;
        }
        if let Some(is_error) = self.is_error {
            output.is_error = is_error;
        }
        if let Some(terminate) = self.terminate {
            output.terminate = terminate;
        }
    }
}

/// 循环钩子；所有方法默认空实现。
#[async_trait]
pub trait LoopHooks: Send + Sync {
    /// run 开始前；返回替换后的系统提示。
    async fn before_agent_start(&self, _req: BeforeAgentStart<'_>) -> Option<String> {
        None
    }

    /// 每次模型请求前（不含 system）；返回替换后的消息列表。
    async fn transform_context(&self, _messages: Vec<Message>) -> Option<Vec<Message>> {
        None
    }

    /// `transform_context` 之后、含 system 的完整 transcript；返回即最终使用值。
    async fn transform_full_context(
        &self,
        _system_prompt: Option<String>,
        _messages: Vec<Message>,
    ) -> Option<(Option<String>, Vec<Message>)> {
        None
    }

    /// 虚拟模型路由；`Ok(None)` = 沿用选中模型（虚拟选中未注册路由时由适配器拒绝）。
    ///
    /// # Errors
    /// 路由函数抛错/返回非法模型 → 调用方以错误终态结束本次请求。
    async fn route_model(
        &self,
        _req: RouteRequest<'_>,
    ) -> std::result::Result<Option<RouteChoice>, String> {
        Ok(None)
    }

    /// 工具参数校验前；可否决/改参/提前终止。
    async fn before_tool_call(&self, _call: &Content) -> Option<BeforeToolCall> {
        None
    }

    /// 工具执行后、结果入上下文前；字段级覆盖。
    async fn after_tool_call(&self, _outcome: &ToolOutcome<'_>) -> Option<ToolOutputPatch> {
        None
    }

    /// 当前生效系统提示（`ctx.getSystemPrompt`）。
    fn system_prompt(&self) -> Option<String> {
        None
    }
}
