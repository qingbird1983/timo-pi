//! 错误类型（设计 05 §6）。

use std::path::PathBuf;

/// timo-config 的全部错误形态；`Display` 输出面向用户的一句话。
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// 显式要求的配置文件/目录不存在。
    #[error("config not found: {0}")]
    NotFound(PathBuf),
    /// 配置文件存在但无法解析（路径 + 原因；配置错误不静默降级）。
    #[error("invalid config {path}: {reason}")]
    Invalid {
        /// 出错文件路径。
        path: PathBuf,
        /// 失败原因（JSON/类型校验文案）。
        reason: String,
    },
    /// `$ENV` 引用的环境变量未设置。
    #[error("environment variable not set: {0}")]
    EnvVarMissing(String),
    /// `!command` 引用的命令非零退出。
    #[error("credential command failed ({status}): {cmd}")]
    CommandFailed {
        /// 命令行。
        cmd: String,
        /// 退出状态描述。
        status: String,
    },
    /// 模型解析未命中（`provider/model` 规格）。
    #[error("model not found: {0}")]
    ModelNotFound(String),
    /// 选中的模型所在供应商没有可用凭证。
    #[error("no credential for provider: {0}")]
    NoCredential(String),
    /// 系统钥匙串读取/写入失败（只含服务名/账号，不含密钥值）。
    #[error("keychain {service}/{account}: {reason}")]
    Keychain {
        /// 钥匙串服务名。
        service: String,
        /// 钥匙串账号。
        account: String,
        /// 后端给出的原因。
        reason: String,
    },
}
