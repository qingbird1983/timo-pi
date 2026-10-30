//! slot 启动数据与脱敏（设计 03 §1.1）。
//!
//! 插件只能拿到自己实例的槽位数据；`toolCard` 的参数与结果先截断到 8 KiB，
//! 再抹掉常见凭据形态，绝不携带系统提示或其他 slot 的内容。

use serde_json::{Value, json};

use crate::features::chat::ToolCallSnapshot;

/// 单个字段的字符上限（8 KiB）。
const MAX_CHARS: usize = 8 * 1024;
/// 会话标题上限。
const MAX_TITLE_CHARS: usize = 512;
/// 抹掉后的占位文本。
const REDACTED: &str = "[redacted]";

/// 凭据前缀（大小写敏感）。
const SECRET_PREFIXES: [&str; 8] = [
    "sk-",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "AKIA",
];
/// 凭据键名（大小写不敏感），后接 `=`/`:` 时其值被抹掉。
const SECRET_KEYS: [&str; 9] = [
    "key",
    "apikey",
    "api_key",
    "secret",
    "token",
    "access_token",
    "client_secret",
    "password",
    "passwd",
];

pub(super) fn panel_data() -> Value {
    json!({ "panelId": "panel", "width": 248 })
}

pub(super) fn tool_card_data(tool: &str, call: Option<&ToolCallSnapshot>) -> Value {
    let (status, input, output) = match call {
        Some(call) => (call.status, sanitize(&call.input), sanitize(&call.output)),
        None => ("idle", String::new(), String::new()),
    };
    json!({ "toolName": tool, "status": status, "input": input, "output": output })
}

pub(super) fn composer_data(text: &str) -> Value {
    json!({ "text": clip_chars(text, MAX_CHARS) })
}

pub(super) fn entry_extra_data(session: &str, title: &str) -> Value {
    json!({ "sessionId": session, "title": clip_chars(title, MAX_TITLE_CHARS) })
}

/// 截断（按字符边界）并抹掉凭据。
pub(super) fn sanitize(text: &str) -> String {
    let clipped = clip_chars(text, MAX_CHARS);
    let mut out = String::with_capacity(clipped.len());
    let mut rest = clipped.as_str();
    while let Some((start, end)) = find_secret(rest) {
        out.push_str(&rest[..start]);
        out.push_str(REDACTED);
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn clip_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}

/// 找到下一段需要抹掉的内容，返回 `(值起点, 值终点)`。
fn find_secret(text: &str) -> Option<(usize, usize)> {
    let lower = text.to_ascii_lowercase();
    let mut best: Option<(usize, usize)> = None;
    let mut consider = |start: usize, end: usize| {
        if end > start && best.is_none_or(|(best, _)| start < best) {
            best = Some((start, end));
        }
    };
    for prefix in SECRET_PREFIXES {
        for (index, _) in text.match_indices(prefix) {
            let value = index + prefix.len();
            let end = token_end(text, value);
            if end.saturating_sub(value) >= 8 {
                consider(index, end);
            }
        }
    }
    for key in SECRET_KEYS {
        for (index, _) in lower.match_indices(key) {
            if index > 0 {
                let previous = text[..index].chars().next_back();
                if previous.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
                    continue;
                }
            }
            let after = index + key.len();
            let Some(value) = value_after_separator(text, after) else {
                continue;
            };
            consider(value, token_end(text, value));
        }
    }
    if let Some(index) = lower.find("bearer ") {
        let value = index + "bearer ".len();
        consider(value, token_end(text, value));
    }
    best
}

/// `=`/`:` 之后的值起点；键与分隔符之间允许空格与成对引号。
fn value_after_separator(text: &str, from: usize) -> Option<usize> {
    let rest = text.get(from..)?;
    let rest = rest.trim_start_matches([' ', '\t', '"', '\'']);
    let rest = rest.strip_prefix('=').or_else(|| rest.strip_prefix(':'))?;
    let value = rest.trim_start_matches([' ', '\t', '"', '\'', '>']);
    if value.is_empty() {
        return None;
    }
    Some(text.len() - value.len())
}

/// 值的结束位置：空白或常见分隔符。
fn token_end(text: &str, start: usize) -> usize {
    text[start..]
        .char_indices()
        .find(|(_, ch)| {
            ch.is_whitespace() || matches!(ch, '"' | '\'' | '`' | ',' | '}' | ']' | ')' | '&')
        })
        .map_or(text.len(), |(offset, _)| start + offset)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_untouched() {
        assert_eq!(sanitize("read src/main.rs ok"), "read src/main.rs ok");
        assert_eq!(
            sanitize("the token bucket is empty"),
            "the token bucket is empty"
        );
    }

    #[test]
    fn cloud_keys_are_redacted() {
        let out = sanitize("sk-abcdefghijklmnop and AKIAABCDEFGHIJKLMNOP");
        assert!(!out.contains("abcdefghijklmnop"), "{out}");
        assert!(!out.contains("ABCDEFGHIJKLMNOP"), "{out}");
        assert_eq!(out.matches(REDACTED).count(), 2);
    }

    #[test]
    fn bearer_and_assignments_are_redacted() {
        let out = sanitize("Authorization: Bearer abc.def-ghi password=hunter2");
        assert!(!out.contains("abc.def-ghi"), "{out}");
        assert!(!out.contains("hunter2"), "{out}");
        assert!(out.contains("Authorization: Bearer"), "{out}");

        let out = sanitize("{\"api_key\": \"AKIA-payload\"}");
        assert!(!out.contains("AKIA-payload"), "{out}");
    }

    #[test]
    fn gt_and_colon_secret_forms_are_redacted() {
        assert!(!sanitize("token: ghp_1234567890").contains("ghp_1234567890"));
        assert!(!sanitize("secret => Topsecret").contains("Topsecret"));
    }

    #[test]
    fn long_text_is_clipped_at_a_char_boundary() {
        let text = "汉".repeat(MAX_CHARS + 10);
        let out = sanitize(&text);
        assert_eq!(out.chars().count(), MAX_CHARS);
    }

    #[test]
    fn tool_card_data_is_deserialized_shape() {
        let data = tool_card_data("grep", None);
        assert_eq!(data["toolName"], "grep");
        assert_eq!(data["status"], "idle");
        assert_eq!(data["input"], "");
    }
}
