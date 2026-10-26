//! 凭证引用三形态（设计 05 §4）：字面量 / `$ENV` / `!command`。

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::Result;
use crate::error::ConfigError;

/// 凭证值的引用形态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueRef {
    /// 字面量（明文，仅建议本地临时使用）。
    Literal(String),
    /// `$ENV_NAME`：读环境变量。
    Env(String),
    /// `!command`：执行命令取 stdout（trim）。
    Command(String),
}

impl ValueRef {
    /// 按前缀解析：`$NAME` → [`ValueRef::Env`]，`!cmd` → [`ValueRef::Command`]，其余字面量。
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        if let Some(name) = raw.strip_prefix('$') {
            Self::Env(name.to_string())
        } else if let Some(cmd) = raw.strip_prefix('!') {
            Self::Command(cmd.to_string())
        } else {
            Self::Literal(raw.to_string())
        }
    }

    /// 解析为最终凭证值；命令执行取 stdout 首段（trim），不落日志。
    ///
    /// # Errors
    /// 环境变量缺失或命令失败时返回对应 [`ConfigError`]。
    pub async fn resolve(&self) -> Result<String> {
        match self {
            Self::Literal(v) => Ok(v.clone()),
            Self::Env(name) => {
                std::env::var(name).map_err(|_| ConfigError::EnvVarMissing(name.clone()))
            }
            Self::Command(cmd) => {
                let out = tokio::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
                    .args(if cfg!(windows) {
                        ["/C", cmd]
                    } else {
                        ["-c", cmd]
                    })
                    .output()
                    .await
                    .map_err(|e| ConfigError::CommandFailed {
                        cmd: cmd.clone(),
                        status: e.to_string(),
                    })?;
                if !out.status.success() {
                    return Err(ConfigError::CommandFailed {
                        cmd: cmd.clone(),
                        status: out.status.to_string(),
                    });
                }
                Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
            }
        }
    }
}

impl Serialize for ValueRef {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Literal(v) => s.serialize_str(v),
            Self::Env(n) => s.serialize_str(&format!("${n}")),
            Self::Command(c) => s.serialize_str(&format!("!{c}")),
        }
    }
}

impl<'de> Deserialize<'de> for ValueRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Ok(Self::parse(&String::deserialize(d)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_forms() {
        assert_eq!(
            ValueRef::parse("sk-abc"),
            ValueRef::Literal("sk-abc".into())
        );
        assert_eq!(ValueRef::parse("$MY_KEY"), ValueRef::Env("MY_KEY".into()));
        assert_eq!(
            ValueRef::parse("!op read key"),
            ValueRef::Command("op read key".into())
        );
    }

    #[tokio::test]
    async fn resolve_literal_and_missing_env() {
        assert_eq!(
            ValueRef::Literal("v".into()).resolve().await.expect("ok"),
            "v"
        );
        let err = ValueRef::Env("TIMO_TEST surely_unset_xyz".into())
            .resolve()
            .await
            .expect_err("unset env");
        assert!(matches!(err, crate::ConfigError::EnvVarMissing(_)));
    }

    #[tokio::test]
    async fn resolve_command() {
        let v = ValueRef::Command("echo hi".into())
            .resolve()
            .await
            .expect("echo works");
        assert_eq!(v, "hi");
    }
}
