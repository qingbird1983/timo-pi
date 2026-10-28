//! 消息模型（设计 01 §2；语义对齐 pi packages/ai/src/types.ts）。
//!
//! 不变量（验收断言）：`usage.input` 不含 cache_read/cache_write；
//! `total_tokens` = input+output+cache_read+cache_write；`reasoning` ⊆ `output`。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 停止原因（pi StopReason，含 camelCase `toolUse` 的线上写法）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    /// 流进行中（占位，不会出现在终态）。
    #[serde(rename = "pending")]
    Pending,
    /// 正常结束。
    #[serde(rename = "stop")]
    Stop,
    /// 输出长度截断。
    #[serde(rename = "length")]
    Length,
    /// 请求工具调用。
    #[serde(rename = "toolUse")]
    ToolUse,
    /// 错误终止。
    #[serde(rename = "error")]
    Error,
    /// 用户取消。
    #[serde(rename = "aborted")]
    Aborted,
    /// 延迟交付（Phase 5+，占位）。
    #[serde(rename = "deferred")]
    Deferred,
}

/// 内容块（消息的最小组成单元）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Content {
    /// 文本。
    Text {
        /// 文本内容。
        text: String,
    },
    /// 思考块（推理模型输出；signature 供供应商回放）。
    Thinking {
        /// 思考文本。
        thinking: String,
        /// 供应商回放签名（Anthropic signature / OpenAI reasoning 字段名）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
        /// 是否为 redacted thinking（Anthropic）。
        #[serde(default)]
        redacted: bool,
    },
    /// 图像（base64）。
    Image {
        /// base64 编码数据。
        data: String,
        /// MIME 类型。
        mime_type: String,
    },
    /// 工具调用（仅出现在 assistant 消息）。
    ToolCall {
        /// 调用 id（结果回引用）。
        id: String,
        /// 工具名。
        name: String,
        /// 参数对象；解析失败时为 `{}`。
        #[serde(default)]
        arguments: Value,
        /// 原始参数非法 JSON 时的原因。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arguments_error: Option<String>,
    },
}

/// token 用量与费用（每条 assistant 消息必填，零值初始化）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// 输入 token（**不含** cache_read/cache_write）。
    #[serde(default)]
    pub input: u64,
    /// 输出 token（含 reasoning）。
    #[serde(default)]
    pub output: u64,
    /// 缓存读 token。
    #[serde(default)]
    pub cache_read: u64,
    /// 缓存写 token。
    #[serde(default)]
    pub cache_write: u64,
    /// 推理 token（output 的子集；OpenAI reasoning_tokens）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<u64>,
    /// 1h 缓存写 token（Anthropic 专属）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_1h: Option<u64>,
    /// 四项之和。
    #[serde(default)]
    pub total_tokens: u64,
    /// 费用（USD）。
    #[serde(default)]
    pub cost: Cost,
}

/// 费用明细（USD）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    /// 输入费用。
    #[serde(default)]
    pub input: f64,
    /// 输出费用。
    #[serde(default)]
    pub output: f64,
    /// 缓存读费用。
    #[serde(default)]
    pub cache_read: f64,
    /// 缓存写费用。
    #[serde(default)]
    pub cache_write: f64,
    /// 总费用。
    #[serde(default)]
    pub total: f64,
}

/// assistant 消息（流式终态与 partial 同构）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    /// 内容块（Text/Thinking/ToolCall）。
    pub content: Vec<Content>,
    /// 协议类型。
    pub api: String,
    /// 供应商 id。
    pub provider: String,
    /// 请求的模型 id。
    pub model: String,
    /// 响应里的模型 id（供应商可能重写）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_model: Option<String>,
    /// 供应商响应 id。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    /// 用量（必填，零值初始化）。
    pub usage: Usage,
    /// 停止原因（必填）。
    pub stop_reason: StopReason,
    /// 供应商原生停止字符串（end_turn / tool_calls…）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_stop_reason: Option<String>,
    /// stop_reason = Error/Aborted 时的人读原因。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// 本次请求实际使用的思考档位。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<timo_config::ThinkingLevel>,
    /// 最终消息上的总耗时（毫秒）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// unix 毫秒时间戳。
    pub timestamp: i64,
}

impl AssistantMessage {
    /// 构造零值 pending 消息（流起点 / 失败兜底）。
    #[must_use]
    pub fn pending(api: &str, provider: &str, model: &str, timestamp: i64) -> Self {
        Self {
            content: vec![],
            api: api.to_string(),
            provider: provider.to_string(),
            model: model.to_string(),
            response_model: None,
            response_id: None,
            usage: Usage::default(),
            stop_reason: StopReason::Pending,
            raw_stop_reason: None,
            error_message: None,
            thinking_level: None,
            duration_ms: None,
            timestamp,
        }
    }
}

/// user 消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    /// 内容块（Text/Image）。
    pub content: Vec<Content>,
    /// unix 毫秒时间戳。
    pub timestamp: i64,
}

/// system 消息（提示词 + 工具声明由循环折叠，见 design 01 §2）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMessage {
    /// 系统提示文本。
    pub content: String,
    /// unix 毫秒时间戳。
    pub timestamp: i64,
}

/// 工具结果消息（模型可读的执行回执）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultMessage {
    /// 对应 ToolCall id。
    pub tool_call_id: String,
    /// 工具名。
    pub tool_name: String,
    /// 内容块（Text/Image）。
    pub content: Vec<Content>,
    /// 是否为错误结果（模型可读失败报告）。
    pub is_error: bool,
    /// 工具自身用量（非 LLM 记账）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// 执行时长（毫秒）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// unix 毫秒时间戳。
    pub timestamp: i64,
}

/// 消息统一枚举（serde 以 `role` 判别；会话 entry payload 同构）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Message {
    /// 系统提示。
    System(SystemMessage),
    /// 用户输入。
    User(UserMessage),
    /// 模型回复。
    Assistant(AssistantMessage),
    /// 工具结果。
    ToolResult(ToolResultMessage),
}

impl Message {
    /// unix 毫秒时间戳。
    #[must_use]
    pub fn timestamp(&self) -> i64 {
        match self {
            Self::System(m) => m.timestamp,
            Self::User(m) => m.timestamp,
            Self::Assistant(m) => m.timestamp,
            Self::ToolResult(m) => m.timestamp,
        }
    }
}

/// 工具声明（发给模型的描述；JSON Schema 通货，接缝 #5）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// 工具名（含命名空间前缀）。
    pub name: String,
    /// 给模型的描述。
    pub description: String,
    /// 参数 JSON Schema。
    pub parameters: Value,
}

/// 发往协议层的上下文（折叠前的统一表示）。
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// 系统提示（None = 无）。
    pub system_prompt: Option<String>,
    /// 消息历史。
    pub messages: Vec<Message>,
    /// 本轮可用工具。
    pub tools: Vec<Tool>,
    /// 本次请求的思考档位（Off = 不启用 thinking）。
    pub thinking_level: Option<timo_config::ThinkingLevel>,
}

/// 便捷构造：纯文本 user 消息。
#[must_use]
pub fn user_text(text: &str, timestamp: i64) -> Message {
    Message::User(UserMessage {
        content: vec![Content::Text {
            text: text.to_string(),
        }],
        timestamp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_invariants_documented_by_constructor() {
        let u = Usage {
            input: 10,
            output: 5,
            cache_read: 3,
            cache_write: 2,
            total_tokens: 20,
            ..Default::default()
        };
        assert_eq!(
            u.total_tokens,
            u.input + u.output + u.cache_read + u.cache_write
        );
    }

    #[test]
    fn message_serde_role_tags() {
        let m = user_text("hi", 1);
        let v = serde_json::to_value(&m).expect("ser");
        assert_eq!(v["role"], "user");
        let back: Message = serde_json::from_value(v).expect("de");
        assert_eq!(back.timestamp(), 1);
    }

    #[test]
    fn stop_reason_wire_names() {
        let v = serde_json::to_value(StopReason::ToolUse).expect("ser");
        assert_eq!(v, "toolUse");
    }
}
