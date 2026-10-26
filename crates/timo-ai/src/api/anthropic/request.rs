//! anthropic 请求体构造（设计 01 §4）。

use serde_json::{Value, json};

use crate::catalog::Model;
use crate::stream::StreamOptions;
use crate::types::Context;

use super::convert;

/// 思考档位 → budget_tokens（1024 起步，逐档翻倍；上限夹在 max_tokens 内）。
#[must_use]
pub fn thinking_budget(level: timo_config::ThinkingLevel, max_tokens: u64) -> u64 {
    let base: u64 = match level {
        timo_config::ThinkingLevel::Minimal => 1024,
        timo_config::ThinkingLevel::Low => 2048,
        timo_config::ThinkingLevel::Medium => 4096,
        timo_config::ThinkingLevel::High => 8192,
        timo_config::ThinkingLevel::Xhigh => 16_384,
        timo_config::ThinkingLevel::Max => 32_768,
        timo_config::ThinkingLevel::Off => 0,
    };
    base.clamp(1024, max_tokens.saturating_sub(1).max(1024))
}

/// 构造 `/v1/messages` 请求体。
#[must_use]
pub fn build_body(model: &Model, context: &Context, options: &StreamOptions) -> Value {
    let max_tokens = options.max_tokens.unwrap_or(model.max_tokens);
    let level = context_thinking(context);
    let mut body = json!({
        "model": model.id,
        "max_tokens": max_tokens,
        "stream": true,
        "messages": convert::messages_to_api(context),
    });
    if let Some(prompt) = &context.system_prompt {
        body["system"] = json!([{
            "type": "text",
            "text": prompt,
            "cache_control": {"type": "ephemeral"},
        }]);
    }
    if !context.tools.is_empty() {
        body["tools"] = convert::tools_to_api(context);
    }
    if level != timo_config::ThinkingLevel::Off && model.reasoning {
        body["thinking"] = json!({
            "type": "enabled",
            "budget_tokens": thinking_budget(level, max_tokens),
        });
    } else if let Some(t) = options.temperature {
        body["temperature"] = json!(t);
    }
    body
}

/// 本次请求的思考档位（循环经 `Context::thinking_level` 注入；缺省 Off）。
fn context_thinking(context: &Context) -> timo_config::ThinkingLevel {
    context
        .thinking_level
        .unwrap_or(timo_config::ThinkingLevel::Off)
}

/// 工具 schema 透传（JSON Schema 通货；接缝 #5）。
#[must_use]
pub fn tool_schema(parameters: &Value) -> Value {
    if parameters.get("type") == Some(&json!("object")) {
        parameters.clone()
    } else {
        json!({"type": "object", "properties": {}, "required": []})
    }
}

/// Content 图像块 → anthropic source 结构。
#[must_use]
pub fn image_source(mime: &str, data: &str) -> Value {
    json!({"type": "image", "source": {"type": "base64", "media_type": mime, "data": data}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_clamped_by_max_tokens() {
        assert_eq!(
            thinking_budget(timo_config::ThinkingLevel::Medium, 64000),
            4096
        );
        // budget 必须 < max_tokens：档位预算被夹到 max_tokens-1
        assert_eq!(thinking_budget(timo_config::ThinkingLevel::Max, 2048), 2047);
        assert_eq!(
            thinking_budget(timo_config::ThinkingLevel::Max, 64000),
            32768
        );
        assert_eq!(
            thinking_budget(timo_config::ThinkingLevel::Off, 64000),
            1024
        );
    }

    #[test]
    fn body_has_stream_and_tools() {
        let model: Model = serde_json::from_value(json!({
            "id": "m", "name": "m", "provider": "anthropic", "api": "anthropic-messages",
            "base_url": "x", "reasoning": false, "input": ["text"], "cost": {},
            "context_window": 1, "max_tokens": 100
        }))
        .expect("m");
        let ctx = Context {
            system_prompt: Some("sys".into()),
            messages: vec![],
            tools: vec![crate::types::Tool {
                name: "read".into(),
                description: "d".into(),
                parameters: json!({"type": "object", "properties": {}}),
            }],
            thinking_level: None,
        };
        let body = build_body(&model, &ctx, &StreamOptions::default());
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["max_tokens"], json!(100));
        assert_eq!(body["tools"][0]["name"], "read");
        assert_eq!(body["system"][0]["text"], "sys");
    }
}
