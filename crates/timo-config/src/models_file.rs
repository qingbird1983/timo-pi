//! models.json：自定义供应商与模型（设计 05 §3）。
//!
//! 字段缺省值对齐 pi：`context_window` 128000、`max_tokens` 16384、`reasoning` false。

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::Result;
use crate::error::ConfigError;

/// USD/百万 token 费率（可选 tiers 阶梯，对齐 pi cost 结构）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostRates {
    /// 输入费率。
    #[serde(default)]
    pub input: f64,
    /// 输出费率。
    #[serde(default)]
    pub output: f64,
    /// 缓存读费率。
    #[serde(default)]
    pub cache_read: f64,
    /// 缓存写费率。
    #[serde(default)]
    pub cache_write: f64,
    /// 阶梯费率（`input_tokens_above` 高者胜出，对齐 pi tiers 语义）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tiers: Vec<CostTier>,
}

/// 阶梯费率：累计输入 token 超过阈值时整体适用本档。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostTier {
    /// 触发阈值（输入 token 数）。
    #[serde(rename = "inputTokensAbove")]
    pub input_tokens_above: u64,
    /// 本档输入费率。
    #[serde(default)]
    pub input: f64,
    /// 本档输出费率。
    #[serde(default)]
    pub output: f64,
    /// 本档缓存读费率。
    #[serde(default)]
    pub cache_read: f64,
    /// 本档缓存写费率。
    #[serde(default)]
    pub cache_write: f64,
}

/// models.json 中的自定义模型定义（字段缺省值与 pi 一致）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomModel {
    /// 模型 id（协议 `model` 字段取值）。
    pub id: String,
    /// 展示名（缺省用 id）。
    #[serde(default)]
    pub name: Option<String>,
    /// 是否推理模型（支持 thinking）。
    #[serde(default)]
    pub reasoning: bool,
    /// thinking 档位 → 供应商原生字符串映射（缺省直透档位名）。
    #[serde(default)]
    pub thinking_level_map: Option<std::collections::BTreeMap<String, String>>,
    /// 上下文窗口（缺省 128000）。
    #[serde(default = "default_context_window")]
    pub context_window: u64,
    /// 最大输出 token（缺省 16384）。
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u64,
    /// 输入模态（缺省 `["text"]`）。
    #[serde(default = "default_input")]
    pub input: Vec<String>,
    /// 费率。
    #[serde(default)]
    pub cost: CostRates,
    /// 覆盖供应商 baseUrl。
    #[serde(default)]
    pub base_url: Option<String>,
    /// 额外请求头。
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// 前缀缓存 TTL（秒；None = 未知/不支持预热）。
    #[serde(default)]
    pub cache_ttl_seconds: Option<u64>,
}

fn default_context_window() -> u64 {
    128_000
}

fn default_max_tokens() -> u64 {
    16_384
}

fn default_input() -> Vec<String> {
    vec!["text".to_string()]
}

/// models.json 中的自定义供应商定义。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomProvider {
    /// 供应商 id（models.json 键名；反序列化时允许缺省，由键名回填）。
    #[serde(default)]
    pub name: String,
    /// API 端点基底。
    #[serde(default)]
    pub base_url: Option<String>,
    /// 协议名：`anthropic-messages` | `openai-completions`（Phase 1）。
    #[serde(default)]
    pub api: Option<String>,
    /// 凭证引用（三形态，见 [`crate::ValueRef`]）。
    #[serde(default)]
    pub api_key: Option<crate::ValueRef>,
    /// 额外请求头。
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// 模型清单。
    #[serde(default)]
    pub models: Vec<CustomModel>,
}

/// 读取 models.json；文件不存在返回 `Ok(vec![])`（可选项）。
///
/// 文件形态：`{"providers": {"<name>": {…}}}`——键名即供应商 id（设计 05 §3）。
///
/// # Errors
/// 文件存在但非法时返回 [`ConfigError::Invalid`]。
pub fn load_custom_providers(path: &Path) -> Result<Vec<CustomProvider>> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => {
            return Err(ConfigError::Invalid {
                path: path.to_path_buf(),
                reason: e.to_string(),
            });
        }
    };
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        providers: std::collections::BTreeMap<String, CustomProvider>,
    }
    let file: File = serde_json::from_str(&raw).map_err(|e| ConfigError::Invalid {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    let mut out: Vec<CustomProvider> = file
        .providers
        .into_iter()
        .map(|(name, mut p)| {
            p.name = name;
            p
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// 原子改写某供应商的 `api_key` 引用（保留其余字段与未知字段）。
///
/// # Errors
/// 文件缺失/非法、供应商不存在或写入失败时返回错误；失败不留下半写文件。
pub fn set_provider_api_key(path: &Path, provider: &str, value: &crate::ValueRef) -> Result<()> {
    let raw = std::fs::read_to_string(path).map_err(|error| ConfigError::Invalid {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })?;
    let mut root: serde_json::Value =
        serde_json::from_str(&raw).map_err(|error| ConfigError::Invalid {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
    let Some(providers) = root
        .get_mut("providers")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return Err(ConfigError::Invalid {
            path: path.to_path_buf(),
            reason: "providers object is missing".into(),
        });
    };
    let Some(entry) = providers
        .get_mut(provider)
        .and_then(serde_json::Value::as_object_mut)
    else {
        return Err(ConfigError::Invalid {
            path: path.to_path_buf(),
            reason: format!("provider not found: {provider}"),
        });
    };
    let serialized = serde_json::to_value(value).map_err(|error| ConfigError::Invalid {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })?;
    entry.insert("api_key".into(), serialized);

    let body = serde_json::to_string_pretty(&root).map_err(|error| ConfigError::Invalid {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|error| ConfigError::Invalid {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })?;
    std::fs::rename(&tmp, path).map_err(|error| ConfigError::Invalid {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_model_defaults() {
        let m: CustomModel = serde_json::from_str(r#"{"id": "m1"}"#).expect("ok");
        assert_eq!(m.context_window, 128_000);
        assert_eq!(m.max_tokens, 16_384);
        assert!(!m.reasoning);
        assert_eq!(m.input, vec!["text"]);
        assert_eq!(m.cache_ttl_seconds, None);
        let m: CustomModel =
            serde_json::from_str(r#"{"id": "m1", "cache_ttl_seconds": 300}"#).expect("ok");
        assert_eq!(m.cache_ttl_seconds, Some(300));
    }

    #[test]
    fn parse_file_with_providers() {
        let text = r#"{"providers": {"p1": {"api": "openai-completions",
            "api_key": "$P1_KEY", "models": [{"id": "a", "cost": {"input": 1.0}}]}}}"#;
        let dir = std::env::temp_dir().join(format!("timo-models-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("models.json");
        std::fs::write(&path, text).expect("write");
        let ps = load_custom_providers(&path).expect("parse");
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].models[0].cost.input, 1.0);
    }

    #[test]
    fn set_provider_api_key_preserves_other_fields() {
        let dir = std::env::temp_dir().join(format!("timo-models-set-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("models.json");
        std::fs::write(
            &path,
            r#"{"providers":{"deepseek":{"api":"openai-completions","name":"DeepSeek",
                "models":[{"id":"m1"}],"extra":"keep"}},"top":"keep"}"#,
        )
        .expect("write");
        let value = crate::ValueRef::Keychain {
            service: "timo-pi".into(),
            account: "deepseek".into(),
        };
        set_provider_api_key(&path, "deepseek", &value).expect("write ref");
        let raw = std::fs::read_to_string(&path).expect("read");
        assert!(raw.contains("keychain:timo-pi/deepseek"));
        assert!(raw.contains("\"extra\": \"keep\""));
        assert!(raw.contains("\"top\": \"keep\""));
        let providers = load_custom_providers(&path).expect("parse");
        assert_eq!(providers[0].api_key, Some(value));
        assert!(set_provider_api_key(&path, "missing", &crate::ValueRef::Env("X".into())).is_err());
    }

    #[test]
    fn missing_file_is_empty() {
        let path = std::env::temp_dir()
            .join("timo-config-missing-dir")
            .join("x.json");
        let ps = load_custom_providers(&path).expect("missing = empty");
        assert!(ps.is_empty());
    }
}
