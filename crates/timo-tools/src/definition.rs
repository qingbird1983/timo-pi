//! 工具元模型（设计 03 §1；接缝 #4：MCP/扩展/内置同构）。

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::Result;
use crate::error::ToolError;

/// 工具对模型/用户的暴露面（pi exposure）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Exposure {
    /// 对模型声明、用户可见（默认）。
    #[default]
    Direct,
    /// 只对模型声明。
    ModelOnly,
    /// 仅 codemode 脚本可调（Phase 2 起有意义；Phase 1 视同 Deferred）。
    Codemode,
    /// 注册但不对模型声明（tool_search 按需加载，Phase 2 起有意义）。
    Deferred,
    /// 注册但不可达。
    Hidden,
}

/// 命名空间（MCP server 组等）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    /// 命名空间名（如 `mcp__docs`）。
    pub name: String,
    /// 描述。
    pub description: Option<String>,
}

/// 工具注解（MCP hints 透传；对齐 pi ToolAnnotations）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Annotations {
    /// 只读提示。
    pub read_only_hint: Option<bool>,
    /// 破坏性提示。
    pub destructive_hint: Option<bool>,
    /// 幂等提示。
    pub idempotent_hint: Option<bool>,
    /// 开放世界提示。
    pub open_world_hint: Option<bool>,
}

/// 执行模式（pi executionMode；任一 Sequential 被调工具使整批串行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionMode {
    /// 并行（默认）。
    #[default]
    Parallel,
    /// 串行。
    Sequential,
}

/// 工具执行结果（pi AgentToolResult；`content` 是模型唯一可见物）。
#[derive(Debug, Clone, Default)]
pub struct ToolOutput {
    /// 模型可见内容（Text/Image）。
    pub content: Vec<timo_ai::Content>,
    /// UI/日志细节（永不上模型）。
    pub details: Value,
    /// 结构化输出（供程序化调用方，Phase 1 少用）。
    pub structured_content: Option<Value>,
    /// 是否为"模型可读失败报告"（不抛错路径）。
    pub is_error: bool,
    /// 本批早停信号（全部工具 terminate=true 才生效）。
    pub terminate: bool,
    /// 工具自身用量（子代理这类"内部还有一次模型调用"的工具用它回报，设计 08 §4.3）。
    pub usage: Option<timo_ai::Usage>,
}

impl ToolOutput {
    /// 纯文本输出捷径。
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![timo_ai::Content::Text { text: text.into() }],
            ..Self::default()
        }
    }

    /// 错误文本输出（is_error = true）。
    #[must_use]
    pub fn error_text(text: impl Into<String>) -> Self {
        Self {
            is_error: true,
            ..Self::text(text)
        }
    }
}

/// 单次工具调用上下文。
#[derive(Clone)]
pub struct ToolCallCtx {
    /// 调用 id。
    pub tool_call_id: String,
    /// 已通过 JSON Schema 校验的参数。
    pub params: Value,
    /// 取消令牌（接缝 #7）。
    pub cancel: CancellationToken,
    /// 进度/部分结果通道（settle 后发送被忽略，pi onUpdate 同构）。
    pub updates: tokio::sync::mpsc::Sender<ToolOutput>,
    /// 工作目录。
    pub cwd: Arc<std::path::Path>,
    /// 执行前审阅器（None = 自动执行，Phase 1/2 行为）。
    pub review: Option<Arc<dyn crate::review::ToolReview>>,
    /// 嵌套工具执行宿主（设计 11 §4.1；None = 该次调用不能嵌套别的工具）。
    pub nested: Option<Arc<dyn crate::nested::NestedToolHost>>,
}

/// 工具执行 future。
pub type ToolFuture = Pin<Box<dyn Future<Output = Result<ToolOutput>> + Send>>;

/// 工具定义（pi ToolDefinition 的 Phase 1 子集；扩展期字段 Phase 2 只加不改）。
#[derive(Clone)]
pub struct ToolDefinition {
    /// LLM 调用名（内置裸名；MCP `mcp__<server>__<tool>`）。
    pub name: String,
    /// 人读标签。
    pub label: String,
    /// 模型描述。
    pub description: String,
    /// 系统提示"Available tools"一行。
    pub prompt_snippet: Option<String>,
    /// 系统提示 Guidelines 追加条目。
    pub prompt_guidelines: Vec<String>,
    /// 参数 JSON Schema（接缝 #5 唯一通货）。
    pub parameters: Value,
    /// structuredContent 的 JSON Schema（可选）。
    pub output_schema: Option<Value>,
    /// 暴露面。
    pub exposure: Exposure,
    /// 命名空间（None = 内置）。
    pub namespace: Option<Namespace>,
    /// 注解。
    pub annotations: Annotations,
    /// 执行模式。
    pub execution_mode: ExecutionMode,
    /// 执行体。
    pub execute: Arc<dyn Fn(ToolCallCtx) -> ToolFuture + Send + Sync>,
}

impl ToolDefinition {
    /// 执行入口（参数已校验；取消/进度经 ctx）。
    pub fn run(&self, ctx: ToolCallCtx) -> ToolFuture {
        (self.execute)(ctx)
    }

    /// 是否对模型声明（Hidden/Deferred/Codemode 不声明；设计 06 §1.3）。
    #[must_use]
    pub fn declared_to_model(&self) -> bool {
        matches!(self.exposure, Exposure::Direct | Exposure::ModelOnly)
    }
}

impl std::fmt::Debug for ToolDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolDefinition")
            .field("name", &self.name)
            .field("exposure", &self.exposure)
            .field("execution_mode", &self.execution_mode)
            .finish_non_exhaustive()
    }
}

/// 参数 JSON Schema 校验（接缝 #5 唯一校验路径）。
///
/// # Errors
/// 校验失败 → [`ToolError::InvalidParams`]（含校验器文案）。
pub fn validate_params(schema: &Value, params: &Value) -> Result<()> {
    let validator = jsonschema::validator_for(schema).map_err(|e| ToolError::InvalidParams {
        reason: format!("bad schema: {e}"),
    })?;
    let errors: Vec<String> = validator
        .iter_errors(params)
        .map(|e| e.to_string())
        .collect();
    match errors.first() {
        Some(first) => Err(ToolError::InvalidParams {
            reason: first.clone(),
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn schema_validation_reports_first_error() {
        let schema = json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"]
        });
        assert!(validate_params(&schema, &json!({"path": "a"})).is_ok());
        let err = validate_params(&schema, &json!({})).expect_err("missing required");
        assert!(matches!(err, ToolError::InvalidParams { .. }));
    }
}
