//! 主应用主题 → NSH shadcn token 的映射（设计 03 §2、04 §5）。
//!
//! 快照给插件读（`theme.get` / `theme.changed`），CSS 变量串交给
//! `AppView::set_theme_json`。插件补丁已按内核白名单消毒，这里只做叠加。

use std::collections::BTreeMap;

use gpui_kit::Hsla;
use gpui_kit::component::Theme;
use serde_json::{Map, Value, json};

/// 当前主题的 token 表（颜色为 `#rrggbb[aa]`，长度为 `Npx`）。
#[must_use]
pub(crate) fn tokens(theme: &Theme) -> BTreeMap<String, String> {
    let colors = &theme.colors;
    let pairs: [(&str, Hsla); 31] = [
        ("background", colors.background),
        ("foreground", colors.foreground),
        // gpui-kit 没有 card 字段：卡片与窗口同底。
        ("card", colors.background),
        ("card-foreground", colors.foreground),
        ("popover", colors.popover),
        ("popover-foreground", colors.popover_foreground),
        ("primary", colors.primary),
        ("primary-foreground", colors.primary_foreground),
        ("secondary", colors.secondary),
        ("secondary-foreground", colors.secondary_foreground),
        ("muted", colors.muted),
        ("muted-foreground", colors.muted_foreground),
        ("accent", colors.accent),
        ("accent-foreground", colors.accent_foreground),
        ("destructive", colors.danger),
        ("border", colors.border),
        ("input", colors.input),
        ("ring", colors.ring),
        ("chart-1", colors.chart_1),
        ("chart-2", colors.chart_2),
        ("chart-3", colors.chart_3),
        ("chart-4", colors.chart_4),
        ("chart-5", colors.chart_5),
        ("sidebar", colors.sidebar),
        ("sidebar-foreground", colors.sidebar_foreground),
        ("sidebar-primary", colors.sidebar_primary),
        (
            "sidebar-primary-foreground",
            colors.sidebar_primary_foreground,
        ),
        ("sidebar-accent", colors.sidebar_accent),
        (
            "sidebar-accent-foreground",
            colors.sidebar_accent_foreground,
        ),
        ("sidebar-border", colors.sidebar_border),
        ("sidebar-ring", colors.ring),
    ];
    let mut out = BTreeMap::new();
    for (name, color) in pairs {
        out.insert(name.to_string(), hex(color));
    }
    let radius = f32::from(theme.radius);
    out.insert("radius".into(), px(radius));
    out.insert("radius-sm".into(), px((radius * 0.5).max(0.0)));
    out.insert("radius-md".into(), px(radius));
    out.insert("radius-lg".into(), px(f32::from(theme.radius_lg)));
    let base = f32::from(theme.font_size);
    out.insert("text-xs".into(), px(base * 0.75));
    out.insert("text-sm".into(), px(base * 0.875));
    out.insert("text-base".into(), px(base));
    out.insert("text-lg".into(), px(base * 1.125));
    for (step, value) in [4.0, 8.0, 12.0, 16.0, 20.0, 24.0].into_iter().enumerate() {
        out.insert(format!("space-{}", step + 1), px(value));
    }
    out
}

/// `{appearance, tokens}` 快照；补丁覆盖在 token 上。
#[must_use]
pub(crate) fn snapshot(
    dark: bool,
    tokens: &BTreeMap<String, String>,
    patch: Option<&BTreeMap<String, String>>,
) -> Value {
    let mut merged = tokens.clone();
    if let Some(patch) = patch {
        merged.extend(
            patch
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
    json!({
        "appearance": if dark { "dark" } else { "light" },
        "tokens": merged,
    })
}

/// `set_theme_json` 需要的 `{ "cssVars": { "theme": {…} } }`。
///
/// 全部写进 `theme`：桌面同一时刻只画一种外观，切换时重新下发。
#[must_use]
pub(crate) fn css_vars(
    tokens: &BTreeMap<String, String>,
    patch: Option<&BTreeMap<String, String>>,
) -> String {
    let mut vars = Map::new();
    for (key, value) in tokens {
        vars.insert(key.clone(), Value::String(value.clone()));
    }
    if let Some(patch) = patch {
        for (key, value) in patch {
            vars.insert(key.clone(), Value::String(value.clone()));
        }
    }
    json!({ "cssVars": { "theme": vars } }).to_string()
}

fn hex(color: Hsla) -> String {
    let rgba = color.to_rgb();
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let alpha = channel(rgba.a);
    if alpha == 255 {
        format!(
            "#{:02x}{:02x}{:02x}",
            channel(rgba.r),
            channel(rgba.g),
            channel(rgba.b)
        )
    } else {
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            channel(rgba.r),
            channel(rgba.g),
            channel(rgba.b),
            alpha
        )
    }
}

fn px(value: f32) -> String {
    format!("{value:.1}px")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn sample() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("background".to_string(), "#ffffff".to_string()),
            ("primary".to_string(), "#112233".to_string()),
        ])
    }

    #[test]
    fn tokens_cover_colors_and_lengths() {
        let tokens = tokens(&Theme::default());
        assert!(tokens["background"].starts_with('#'));
        assert!(tokens["foreground"].starts_with('#'));
        assert_eq!(tokens["card"], tokens["background"]);
        assert!(tokens["radius"].ends_with("px"));
        assert_eq!(tokens.len(), 45);
    }

    #[test]
    fn patch_wins_in_snapshot_and_css() {
        let patch = BTreeMap::from([("background".to_string(), "#101010".to_string())]);
        let snapshot = snapshot(true, &sample(), Some(&patch));
        assert_eq!(snapshot["appearance"], "dark");
        assert_eq!(snapshot["tokens"]["background"], "#101010");
        assert_eq!(snapshot["tokens"]["primary"], "#112233");

        let css = css_vars(&sample(), Some(&patch));
        let value: Value = serde_json::from_str(&css).unwrap();
        assert_eq!(value["cssVars"]["theme"]["background"], "#101010");
        assert_eq!(value["cssVars"]["theme"]["primary"], "#112233");
    }
}
