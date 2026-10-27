//! 宿主与 JS 之间的数据类型（设计 01 §2）。

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

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
    /// 工具自身用量：形态与 `timo_ai::Usage` 一致，缺省 `None`（设计 08 §4.3）。
    pub usage: Option<timo_ai::Usage>,
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

/// 虚拟模型注册元数据（路由函数留在 JS；设计 10 §4.2）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualModelMeta {
    /// 供应商 id。
    pub provider: String,
    /// 模型 id。
    pub id: String,
    /// 展示名。
    #[serde(default)]
    pub name: String,
    /// 可选档位（仅注册期校验）。
    #[serde(default)]
    pub thinking_levels: Vec<String>,
    /// 首个响应前的窗口（仅注册期校验）。
    #[serde(default)]
    pub context_window: Option<u64>,
    /// 首个响应前的输出上限（仅注册期校验）。
    #[serde(default)]
    pub max_tokens: Option<u64>,
    /// 输入模态（仅注册期校验）。
    #[serde(default)]
    pub input: Vec<String>,
    /// 所属扩展（宿主填写，不由 JS 传入）。
    #[serde(skip)]
    pub ext_id: String,
}
