//! 主题 token 消毒（设计 03 §2）：白名单键、纯颜色/长度值，越界键记 E743。
//!
//! 插件只能调整观感，不能注入布局、动画、阴影、字体族或 `url(...)`；
//! 这里返回补丁与警告，应用由桌面主题层完成。

use std::collections::BTreeMap;

use serde_json::Value;

/// 通过消毒的主题补丁；键与值都保持字符串。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemePatch {
    /// `token → value`。
    pub tokens: BTreeMap<String, String>,
}

impl ThemePatch {
    /// 是否为空（无任何合法 token）。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}

/// 一条被忽略的 token（诊断为 E743 警告）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeWarning {
    /// 越界/非法的键（顶层结构错误时为空串）。
    pub key: String,
    /// 忽略原因。
    pub reason: String,
}

impl ThemeWarning {
    /// 诊断码，固定为 E743。
    #[must_use]
    pub fn code(&self) -> &'static str {
        "E743"
    }
}

/// 值的允许类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    Color,
    Length,
}

/// 消毒一次 `theme.update` 提交；返回可应用的补丁与被忽略项。
#[must_use]
pub fn sanitize_theme(tokens: &Value) -> (ThemePatch, Vec<ThemeWarning>) {
    let mut patch = ThemePatch::default();
    let mut warnings = vec![];
    let Some(map) = tokens.as_object() else {
        warnings.push(warning("", "theme patch must be an object"));
        return (patch, warnings);
    };
    for (key, value) in map {
        let Some(category) = category(key) else {
            warnings.push(warning(key, "token is outside the theme allowlist"));
            continue;
        };
        let Some(raw) = value.as_str() else {
            warnings.push(warning(key, "token value must be a string"));
            continue;
        };
        if raw.chars().count() > 64 {
            warnings.push(warning(key, "token value exceeds 64 characters"));
            continue;
        }
        if is_dangerous(raw) {
            warnings.push(warning(key, "token value contains a forbidden construct"));
            continue;
        }
        let accepted = match category {
            Category::Color => is_color(raw),
            Category::Length => is_length(raw),
        };
        if !accepted {
            warnings.push(warning(key, "token value is not a valid color or length"));
            continue;
        }
        patch.tokens.insert(key.clone(), raw.trim().to_string());
    }
    (patch, warnings)
}

/// 白名单：颜色、圆角、字号、间距。
fn category(key: &str) -> Option<Category> {
    const COLORS: &[&str] = &[
        "background",
        "foreground",
        "card",
        "border",
        "primary",
        "muted",
        "accent",
    ];
    const LENGTHS: &[&str] = &[
        "radius-sm",
        "radius-md",
        "radius-lg",
        "text-xs",
        "text-sm",
        "text-base",
        "text-lg",
        "space-1",
        "space-2",
        "space-3",
        "space-4",
        "space-5",
        "space-6",
    ];
    if COLORS.contains(&key) || is_sidebar_token(key) {
        return Some(Category::Color);
    }
    LENGTHS.contains(&key).then_some(Category::Length)
}

fn is_sidebar_token(key: &str) -> bool {
    if key == "sidebar" {
        return true;
    }
    key.strip_prefix("sidebar-").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

fn is_dangerous(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "url(",
        "var(",
        "expression",
        "javascript:",
        "@import",
        "\\",
        "{",
        "}",
        ";",
        "<",
        ">",
        "\n",
        "\r",
    ];
    NEEDLES.iter().any(|needle| lower.contains(needle))
}

fn is_color(raw: &str) -> bool {
    let value = raw.trim();
    if let Some(hex) = value.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    let lower = value.to_ascii_lowercase();
    for prefix in ["rgb(", "rgba(", "hsl(", "hsla(", "oklch(", "color("] {
        if lower.starts_with(prefix) {
            return lower.ends_with(')');
        }
    }
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic() || byte == b'-')
}

fn is_length(raw: &str) -> bool {
    let value = raw.trim();
    let number = ["px", "rem", "em", "%"]
        .iter()
        .find_map(|unit| value.strip_suffix(unit))
        .unwrap_or(value);
    if number.is_empty() {
        return false;
    }
    let bytes = number.as_bytes();
    if !(bytes[0].is_ascii_digit() || bytes[0] == b'.') {
        return false;
    }
    number.parse::<f64>().is_ok_and(f64::is_finite)
}

fn warning(key: &str, reason: &str) -> ThemeWarning {
    ThemeWarning {
        key: key.to_string(),
        reason: reason.to_string(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn allowed_tokens_pass_through() {
        let (patch, warnings) = sanitize_theme(&json!({
            "background": "#101010",
            "sidebar-foreground": "hsl(0 0% 90%)",
            "primary": "tomato",
            "radius-md": "6px",
            "text-sm": "14",
            "space-2": "0.5rem",
        }));
        assert!(warnings.is_empty());
        assert_eq!(patch.tokens.len(), 6);
        assert_eq!(patch.tokens["background"], "#101010");
        assert_eq!(patch.tokens["space-2"], "0.5rem");
    }

    #[test]
    fn layout_and_injection_are_ignored() {
        let (patch, warnings) = sanitize_theme(&json!({
            "display": "none",
            "position": "fixed",
            "grid-template": "1fr",
            "background": "url(https://evil.example/x.png)",
            "border": "red; display: none",
            "card": "var(--sidebar)",
            "radius-lg": "calc(9999px)",
            "font-family": "Comic Sans",
        }));
        assert!(patch.is_empty());
        assert_eq!(warnings.len(), 8);
        assert!(warnings.iter().all(|item| item.code() == "E743"));
    }

    #[test]
    fn values_are_typed_and_bounded() {
        let (patch, warnings) = sanitize_theme(&json!({
            "primary": 42,
            "accent": "x".repeat(65),
            "muted": "rgb(0, 0, 0)",
        }));
        assert_eq!(patch.tokens.len(), 1);
        assert_eq!(patch.tokens["muted"], "rgb(0, 0, 0)");
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn non_object_is_a_single_warning() {
        let (patch, warnings) = sanitize_theme(&json!(["background", "#fff"]));
        assert!(patch.is_empty());
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].key, "");
    }
}
