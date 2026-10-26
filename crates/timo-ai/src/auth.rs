//! 认证（设计 01 §7；对齐 pi applyAuth 优先级）。

use timo_config::ValueRef;

use crate::errors::AiError;

/// 认证方式枚举（OAuth 为 Phase 5+ 占位，构造即报 [`AiError::Unsupported`]）。
#[derive(Debug, Clone)]
pub enum AuthMethod {
    /// API key（三形态引用）。
    ApiKey {
        /// 值引用（字面量/`$ENV`/`!cmd`）。
        value_ref: ValueRef,
    },
    /// 指名环境变量。
    EnvVar {
        /// 变量名。
        name: String,
    },
    /// 命令输出。
    Command {
        /// 命令行。
        cmd: String,
    },
    /// OAuth（Phase 5+，占位不实现）。
    OAuth,
}

impl AuthMethod {
    /// 解析为最终凭证字符串。
    ///
    /// # Errors
    /// OAuth 返回 [`AiError::Unsupported`]；引用解析失败透传 [`AiError::Config`]。
    pub async fn resolve(&self) -> Result<String, AiError> {
        match self {
            Self::ApiKey { value_ref } => value_ref.resolve().await.map_err(AiError::Config),
            Self::EnvVar { name } => {
                std::env::var(name).map_err(|_| AiError::NoApiKey(format!("env {name} unset")))
            }
            Self::Command { cmd } => ValueRef::Command(cmd.clone())
                .resolve()
                .await
                .map_err(AiError::Config),
            Self::OAuth => Err(AiError::Unsupported("provider OAuth".to_string())),
        }
    }
}

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
    /// 见 [`AuthMethod::resolve`] 与 [`AiError::NoApiKey`]。
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
