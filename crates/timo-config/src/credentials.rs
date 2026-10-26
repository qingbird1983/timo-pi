//! 凭证引用四形态（设计 05 §4 + 设计 02 §3）：字面量 / `$ENV` / `!command` /
//! `keychain:<service>/<account>`。

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::Result;
use crate::error::ConfigError;

/// 钥匙串默认服务名（与数据目录同名；设计 02 §2）。
pub const KEYCHAIN_SERVICE: &str = "timo-pi";

/// 凭证值的引用形态。
#[derive(Clone, PartialEq, Eq)]
pub enum ValueRef {
    /// 字面量（明文，仅建议本地临时使用）。
    Literal(String),
    /// `$ENV_NAME`：读环境变量。
    Env(String),
    /// `!command`：执行命令取 stdout（trim）。
    Command(String),
    /// `keychain:<service>/<account>`：读系统钥匙串。
    Keychain {
        /// 钥匙串服务名。
        service: String,
        /// 钥匙串账号（provider id）。
        account: String,
    },
}

impl ValueRef {
    /// 按前缀解析：`$NAME` → [`ValueRef::Env`]，`!cmd` → [`ValueRef::Command`]，
    /// `keychain:<service>/<account>` → [`ValueRef::Keychain`]，其余字面量。
    ///
    /// # Errors
    /// `keychain:` 缺少服务名/账号或含多余分隔符时返回面向用户的错误文本。
    pub fn parse(raw: &str) -> std::result::Result<Self, String> {
        if let Some(name) = raw.strip_prefix('$') {
            Ok(Self::Env(name.to_string()))
        } else if let Some(cmd) = raw.strip_prefix('!') {
            Ok(Self::Command(cmd.to_string()))
        } else if let Some(rest) = raw.strip_prefix("keychain:") {
            let mut parts = rest.split('/');
            match (parts.next(), parts.next(), parts.next()) {
                (Some(service), Some(account), None)
                    if !service.is_empty() && !account.is_empty() =>
                {
                    Ok(Self::Keychain {
                        service: service.to_string(),
                        account: account.to_string(),
                    })
                }
                _ => {
                    Err("invalid keychain reference: expected keychain:<service>/<account>".into())
                }
            }
        } else {
            Ok(Self::Literal(raw.to_string()))
        }
    }

    /// 凭证来源（设置页展示用；不读取、不泄露值）。
    #[must_use]
    pub fn source(&self) -> CredentialSource {
        match self {
            Self::Literal(_) => CredentialSource::Literal,
            Self::Env(name) => CredentialSource::Env(name.clone()),
            Self::Command(_) => CredentialSource::Command,
            Self::Keychain { service, account } => CredentialSource::Keychain {
                service: service.clone(),
                account: account.clone(),
            },
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
            Self::Keychain { service, account } => {
                let (blocking_service, blocking_account) = (service.clone(), account.clone());
                tokio::task::spawn_blocking(move || {
                    keychain_get(&blocking_service, &blocking_account)
                })
                .await
                .map_err(|error| ConfigError::Keychain {
                    service: service.clone(),
                    account: account.clone(),
                    reason: error.to_string(),
                })?
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
            Self::Keychain { service, account } => {
                s.serialize_str(&format!("keychain:{service}/{account}"))
            }
        }
    }
}

/// 凭证来源标签（不含值）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialSource {
    /// 系统钥匙串条目。
    Keychain {
        /// 服务名。
        service: String,
        /// 账号。
        account: String,
    },
    /// 环境变量名。
    Env(String),
    /// 命令引用。
    Command,
    /// 配置文件中的明文。
    Literal,
}

impl std::fmt::Debug for ValueRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Literal(_) => f.write_str("Literal(<redacted>)"),
            Self::Command(_) => f.write_str("Command(<redacted>)"),
            Self::Env(name) => f.debug_tuple("Env").field(name).finish(),
            Self::Keychain { service, account } => f
                .debug_struct("Keychain")
                .field("service", service)
                .field("account", account)
                .finish(),
        }
    }
}

/// 写入钥匙串（阻塞；调用方放到 `spawn_blocking` 或非异步上下文）。
///
/// # Errors
/// 后端不可用、权限拒绝或条目写入失败时返回带原因的错误。
pub fn keychain_set(service: &str, account: &str, secret: &str) -> crate::Result<()> {
    let entry = keyring::Entry::new(service, account)
        .map_err(|error| keychain_error(service, account, error))?;
    entry
        .set_password(secret)
        .map_err(|error| keychain_error(service, account, error))
}

/// 删除钥匙串条目（阻塞）；条目不存在按成功处理。
///
/// # Errors
/// 后端不可用或删除失败（非“不存在”）时返回错误。
pub fn keychain_delete(service: &str, account: &str) -> crate::Result<()> {
    let entry = keyring::Entry::new(service, account)
        .map_err(|error| keychain_error(service, account, error))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(keychain_error(service, account, error)),
    }
}

/// 读取钥匙串条目（阻塞）。
///
/// # Errors
/// 条目不存在或后端不可用时返回 [`ConfigError::Keychain`]。
pub fn keychain_get(service: &str, account: &str) -> crate::Result<String> {
    let entry = keyring::Entry::new(service, account)
        .map_err(|error| keychain_error(service, account, error))?;
    entry
        .get_password()
        .map_err(|error| keychain_error(service, account, error))
}

fn keychain_error(service: &str, account: &str, error: keyring::Error) -> ConfigError {
    ConfigError::Keychain {
        service: service.to_string(),
        account: account.to_string(),
        reason: error.to_string(),
    }
}

impl<'de> Deserialize<'de> for ValueRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_forms() {
        assert_eq!(
            ValueRef::parse("sk-abc").expect("literal"),
            ValueRef::Literal("sk-abc".into())
        );
        assert_eq!(
            ValueRef::parse("$MY_KEY").expect("env"),
            ValueRef::Env("MY_KEY".into())
        );
        assert_eq!(
            ValueRef::parse("!op read key").expect("command"),
            ValueRef::Command("op read key".into())
        );
        assert_eq!(
            ValueRef::parse("keychain:timo-pi/deepseek").expect("keychain"),
            ValueRef::Keychain {
                service: "timo-pi".into(),
                account: "deepseek".into(),
            }
        );
    }

    #[test]
    fn keychain_roundtrip_and_serde() {
        let value = ValueRef::Keychain {
            service: "timo-pi".into(),
            account: "deepseek".into(),
        };
        let json = serde_json::to_string(&value).expect("serialize");
        assert_eq!(json, "\"keychain:timo-pi/deepseek\"");
        let parsed: ValueRef = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, value);
    }

    #[test]
    fn malformed_keychain_is_rejected() {
        for raw in [
            "keychain:",
            "keychain:timo-pi",
            "keychain:timo-pi/",
            "keychain:/deepseek",
            "keychain:a/b/c",
        ] {
            assert!(ValueRef::parse(raw).is_err(), "{raw} should fail");
        }
        let err = serde_json::from_str::<ValueRef>("\"keychain:timo-pi\"").expect_err("invalid");
        assert!(err.to_string().contains("keychain"));
    }

    #[test]
    fn debug_redacts_literal_and_command() {
        let literal = format!("{:?}", ValueRef::Literal("sk-secret-123".into()));
        assert!(!literal.contains("sk-secret-123"));
        let command = format!("{:?}", ValueRef::Command("print secret".into()));
        assert!(!command.contains("print secret"));
        let env = format!("{:?}", ValueRef::Env("MY_KEY".into()));
        assert!(env.contains("MY_KEY"));
        let keychain = format!(
            "{:?}",
            ValueRef::Keychain {
                service: "timo-pi".into(),
                account: "deepseek".into()
            }
        );
        assert!(keychain.contains("deepseek"));
    }

    #[test]
    fn source_never_contains_value() {
        assert_eq!(
            ValueRef::Literal("sk-secret".into()).source(),
            CredentialSource::Literal
        );
        assert_eq!(
            ValueRef::Env("MY_KEY".into()).source(),
            CredentialSource::Env("MY_KEY".into())
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
    async fn keychain_roundtrip_when_backend_available() {
        let account = format!("timo-test-{}", std::process::id());
        let service = "timo-pi-test";
        let secret = "sk-test-value-123";
        if keychain_set(service, &account, secret).is_err() {
            // 无钥匙串后端的平台（设计 02 §7：标记 skipped 并记录原因）。
            return;
        }
        let value = ValueRef::Keychain {
            service: service.into(),
            account: account.clone(),
        };
        assert_eq!(value.resolve().await.expect("read back"), secret);
        keychain_delete(service, &account).expect("cleanup");
        let err = value.resolve().await.expect_err("removed");
        let text = err.to_string();
        assert!(text.contains(service) && text.contains(&account));
        assert!(!text.contains(secret));
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
