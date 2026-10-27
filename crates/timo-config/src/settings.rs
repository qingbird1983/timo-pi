//! settings.json：结构、加载与合并（设计 05 §2）。
//!
//! 未知字段一律忽略（向前兼容）；解析失败报 [`ConfigError::Invalid`]，不静默降级。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::error::ConfigError;
use crate::thinking::ThinkingLevel;

/// assistant 轮次重试设置（对齐 pi settings.retry 默认值）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RetrySettings {
    /// 是否启用重试。
    pub enabled: bool,
    /// 最大重试次数（首次调用不计）。
    pub max_retries: u32,
    /// 退避基数（毫秒）。
    pub base_delay_ms: u64,
    /// 单次退避上限（毫秒）。
    pub max_retry_delay_ms: u64,
}

impl Default for RetrySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: 3,
            base_delay_ms: 2000,
            max_retry_delay_ms: 60_000,
        }
    }
}

/// read 工具图像设置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageSettings {
    /// 自动缩放大图（设计 03 §3 read）。
    pub auto_resize: bool,
}

impl Default for ImageSettings {
    fn default() -> Self {
        Self { auto_resize: true }
    }
}

/// 自动上下文压缩设置（phase-3.5/04；默认值与 pi 一致）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CompactionSettings {
    /// 是否启用阈值触发。
    pub enabled: bool,
    /// 给输出预留的 token（窗口 − reserve 为触发线）。
    pub reserve_tokens: u64,
    /// 压缩后保留的近期 token 目标。
    pub keep_recent_tokens: u64,
}

impl Default for CompactionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            reserve_tokens: 16384,
            keep_recent_tokens: 20000,
        }
    }
}

/// 设置文件顶层结构（全局 + 项目深合并后的结果）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 默认供应商 id。
    pub default_provider: Option<String>,
    /// 默认模型 id（配合 `default_provider`）。
    pub default_model: Option<String>,
    /// 默认思考档位（None → 内核取 medium）。
    pub default_thinking_level: Option<ThinkingLevel>,
    /// 内置工具激活集（支持 `+name`/`-name` 修饰）。
    pub default_tools: Vec<String>,
    /// 会话目录覆盖。
    pub session_dir: Option<String>,
    /// shell 可执行路径（Windows 缺省探测 bash→PowerShell）。
    pub shell_path: Option<String>,
    /// 命令前置行。
    pub shell_command_prefix: Option<String>,
    /// 重试设置。
    pub retry: RetrySettings,
    /// 图像设置。
    pub images: ImageSettings,
    /// 请求 max_tokens 覆盖。
    pub max_tokens: Option<u64>,
    /// 请求 temperature 覆盖。
    pub temperature: Option<f64>,
    /// 显式技能路径（文件或目录；相对 cwd，phase-3.5/02）。
    pub skills: Vec<String>,
    /// 技能是否进入命令发现（手动 `/skill:` 不受影响）。
    pub enable_skill_commands: bool,
    /// 自动上下文压缩。
    pub compaction: CompactionSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_provider: None,
            default_model: None,
            default_thinking_level: None,
            default_tools: Vec::new(),
            session_dir: None,
            shell_path: None,
            shell_command_prefix: None,
            retry: RetrySettings::default(),
            images: ImageSettings::default(),
            max_tokens: None,
            temperature: None,
            skills: Vec::new(),
            enable_skill_commands: true,
            compaction: CompactionSettings::default(),
        }
    }
}

/// 递归合并两个 JSON 对象：`over` 胜出；数组整体替换。
fn merge_json(base: &mut serde_json::Value, over: serde_json::Value) {
    match (base, over) {
        (serde_json::Value::Object(b), serde_json::Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(slot) => merge_json(slot, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (slot, over) => *slot = over,
    }
}

/// 解析单个设置文件为 JSON；文件不存在返回 `Ok(None)`。
fn read_json(path: &Path) -> Result<Option<serde_json::Value>> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(ConfigError::Invalid {
                path: path.to_path_buf(),
                reason: e.to_string(),
            });
        }
    };
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|e| ConfigError::Invalid {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })
}

/// 加载设置：全局（`settings_path()`，可缺）+ 项目（`<cwd>/.timo/settings.json`，可缺），项目深合并胜出。
///
/// # Errors
/// 任一文件存在但非法时返回 [`ConfigError::Invalid`]。
pub fn load_settings(global: &Path, project: &Path) -> Result<Settings> {
    let mut merged = serde_json::Value::Object(serde_json::Map::new());
    if let Some(v) = read_json(global)? {
        merge_json(&mut merged, v);
    }
    if let Some(v) = read_json(project)? {
        merge_json(&mut merged, v);
    }
    serde_json::from_value(merged).map_err(|e| ConfigError::Invalid {
        path: global.to_path_buf(),
        reason: e.to_string(),
    })
}

/// 按 pi 语义合并两层 `default_tools`：项目里的裸名整体替换集合，`+name`/`-name` 增删。
#[must_use]
pub fn resolve_default_tools(global: &[String], project: &[String]) -> Vec<String> {
    let mut tools: Vec<String> = global.to_vec();
    for item in project {
        if let Some(name) = item.strip_prefix('+') {
            if !tools.iter().any(|t| t == name) {
                tools.push(name.to_string());
            }
        } else if let Some(name) = item.strip_prefix('-') {
            tools.retain(|t| t != name);
        } else {
            // 首个裸名触发整体替换
            tools = vec![item.clone()];
        }
    }
    tools
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_merge_project_wins() {
        let global = serde_json::json!({
            "default_provider": "anthropic",
            "retry": {"max_retries": 3, "base_delay_ms": 2000}
        });
        let project = serde_json::json!({
            "retry": {"max_retries": 5},
            "default_model": "m1"
        });
        let mut merged = global;
        merge_json(&mut merged, project);
        assert_eq!(merged["default_provider"], "anthropic");
        assert_eq!(merged["default_model"], "m1");
        assert_eq!(merged["retry"]["max_retries"], 5);
        assert_eq!(merged["retry"]["base_delay_ms"], 2000);
    }

    #[test]
    fn unknown_fields_ignored() {
        let s: Settings =
            serde_json::from_value(serde_json::json!({"$schema": "x", "future_thing": 1}))
                .expect("unknown fields must be ignored");
        assert_eq!(s.default_provider, None);
    }

    #[test]
    fn defaults_match_pi() {
        let s = Settings::default();
        assert!(s.retry.enabled && s.retry.max_retries == 3 && s.retry.base_delay_ms == 2000);
        assert!(s.images.auto_resize);
        assert!(s.enable_skill_commands);
        assert!(s.skills.is_empty());
    }

    #[test]
    fn skill_keys_are_snake_case_and_mergeable() {
        let s: Settings = serde_json::from_value(serde_json::json!({
            "skills": ["/extras/skills"],
            "enable_skill_commands": false
        }))
        .expect("parse");
        assert_eq!(s.skills, vec!["/extras/skills".to_string()]);
        assert!(!s.enable_skill_commands);
    }

    #[test]
    fn tools_modifiers() {
        let g = vec!["read".to_string(), "bash".to_string(), "edit".to_string()];
        let p = vec!["+grep".to_string(), "-bash".to_string()];
        assert_eq!(resolve_default_tools(&g, &p), vec!["read", "edit", "grep"]);
        let p2 = vec!["find".to_string(), "+write".to_string()];
        assert_eq!(resolve_default_tools(&g, &p2), vec!["find", "write"]);
    }

    #[test]
    fn invalid_json_reports_path() {
        let dir = tempfile_dir();
        let path = dir.join("settings.json");
        std::fs::write(&path, "{not json").expect("write fixture");
        let err = load_settings(&path, &dir.join("none.json")).expect_err("must fail");
        assert!(err.to_string().contains("settings.json"));
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("timo-cfg-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).expect("mkdir");
        d
    }
}
