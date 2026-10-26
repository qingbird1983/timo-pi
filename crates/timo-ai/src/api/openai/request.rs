//! openai-completions 请求体构造（设计 01 §5）。

use serde_json::{Value, json};

use crate::catalog::Model;
use crate::stream::StreamOptions;
use crate::types::Context;

use super::convert;

/// 构造 `/chat/completions` 请求体。
#[must_use]
pub fn build_body(model: &Model, context: &Context, options: &StreamOptions) -> Value {
    let level = context
        .thinking_level
        .unwrap_or(timo_config::ThinkingLevel::Off);
    let mut messages = vec![];
    if let Some(sys) = &context.system_prompt {
        messages.push(json!({"role": "system", "content": sys}));
    }
    messages.extend(convert::messages_to_api(&context.messages));

    let mut body = json!({
        "model": model.id,
        "messages": messages,
        "stream": true,
    });
    if model.compat.supports_usage_in_streaming {
        body["stream_options"] = json!({"include_usage": true});
    }
    let max_tokens = options.max_tokens.unwrap_or(model.max_tokens);
    if model.compat.max_tokens_field {
        body["max_tokens"] = json!(max_tokens);
    } else {
        body["max_completion_tokens"] = json!(max_tokens);
    }
    if !context.tools.is_empty() {
        body["tools"] = tools_to_api(context);
    }
    if let Some(t) = options.temperature {
        body["temperature"] = json!(t);
    }
    if model.reasoning && level != timo_config::ThinkingLevel::Off {
        body["reasoning_effort"] = json!(level.as_str());
    }
    body
}

/// 工具声明：`{type:"function", function:{name, description, parameters}}`（schema 原样透传）。
#[must_use]
pub fn tools_to_api(context: &Context) -> Value {
    Value::Array(
        context
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    },
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Tool;

    fn model(compat_max_tokens_field: bool) -> Model {
        serde_json::from_value(json!({
            "id": "gpt-x", "name": "x", "provider": "openai", "api": "openai-completions",
            "base_url": "https://api.openai.com/v1", "reasoning": true, "input": ["text"],
            "cost": {}, "context_window": 1, "max_tokens": 100,
            "compat": {"max_tokens_field": compat_max_tokens_field}
        }))
        .expect("model")
    }

    #[test]
    fn body_shape_default() {
        let ctx = Context {
            system_prompt: Some("s".into()),
            messages: vec![],
            tools: vec![Tool {
                name: "read".into(),
                description: "d".into(),
                parameters: json!({"type": "object"}),
            }],
            thinking_level: Some(timo_config::ThinkingLevel::Medium),
        };
        let body = build_body(&model(false), &ctx, &StreamOptions::default());
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["stream_options"]["include_usage"], json!(true));
        assert_eq!(body["max_completion_tokens"], json!(100));
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["reasoning_effort"], json!("medium"));
        assert_eq!(body["messages"][0]["role"], "system");
    }

    #[test]
    fn compat_max_tokens_field() {
        let ctx = Context::default();
        let body = build_body(&model(true), &ctx, &StreamOptions::default());
        assert_eq!(body["max_tokens"], json!(100));
        assert!(body.get("max_completion_tokens").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }
}
