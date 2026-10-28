//! 认证（设计 01 §7；对齐 pi applyAuth 优先级）。

use timo_config::ValueRef;

use crate::errors::AiError;

/// 认证解析器：按优先级（显式 > 供应商引用 > 内置环境变量表）产出 api key。
#[derive(Debug, Clone, Default)]
pub struct AuthResolver {
    /// 调用方显式提供的 key（CLI/内核注入），最高优先。
    pub explicit: Option<String>,
    /// 供应商配置中的引用（models.json api_key / 内置默认）。
    pub provider_ref: Option<ValueRef>,
    /// 内置环境变量名（按尝试顺序，对齐 pi env-api-keys 表）。
    pub env_names: Vec<String>,
}

impl AuthResolver {
    /// 构造：仅环境变量表。
    #[must_use]
    pub fn from_env(names: &[&str]) -> Self {
        Self {
            explicit: None,
            provider_ref: None,
            env_names: names.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    /// 按优先级解析凭证；全部未命中返回 [`AiError::NoApiKey`]。
    ///
    /// # Errors
    /// 全部未命中 → [`AiError::NoApiKey`]；引用解析失败按未命中处理。
    pub async fn resolve(&self) -> Result<String, AiError> {
        if let Some(k) = &self.explicit {
            return Ok(k.clone());
        }
        if let Some(r) = &self.provider_ref
            && let Ok(k) = r.resolve().await
        {
            return Ok(k);
        }
        for name in &self.env_names {
            if let Ok(v) = std::env::var(name)
                && !v.is_empty()
            {
                return Ok(v);
            }
        }
        Err(AiError::NoApiKey(
            self.env_names.first().cloned().unwrap_or_default(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn explicit_wins_over_env() {
        let r = AuthResolver {
            explicit: Some("k1".to_string()),
            provider_ref: None,
            env_names: vec!["TIMO_TEST_UNSET".to_string()],
        };
        assert_eq!(r.resolve().await.expect("ok"), "k1");
    }

    #[tokio::test]
    async fn no_credential_errors() {
        let r = AuthResolver::from_env(&["TIMO_TEST_UNSET_A", "TIMO_TEST_UNSET_B"]);
        let err = r.resolve().await.expect_err("none");
        assert!(matches!(err, AiError::NoApiKey(_)));
    }
}
