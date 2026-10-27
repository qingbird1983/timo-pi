//! timo-config：数据目录、设置文件、自定义供应商与凭证引用（设计：docs/design/phase-1/05）。
//!
//! 本 crate 是依赖图的最底层，不依赖任何内部 crate；`ThinkingLevel` 与
//! `ValueRef` 作为共享词汇在此定义，供 timo-ai 及上层使用。

// 测试代码自由使用 unwrap/expect（standards/04 §2）。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod clock;
mod credentials;
mod error;
mod models_file;
mod paths;
mod settings;
mod thinking;

pub use clock::{Clock, FixedClock, SystemClock};
pub use credentials::{
    CredentialSource, KEYCHAIN_SERVICE, ValueRef, keychain_delete, keychain_get, keychain_set,
};
pub use error::ConfigError;
pub use models_file::{
    CostRates, CostTier, CustomModel, CustomProvider, load_custom_providers, set_provider_api_key,
};
pub use paths::{
    agent_dir, encode_cwd, mcp_path, models_path, project_settings_path, session_dir_for,
    sessions_dir, settings_path,
};
pub use settings::{ImageSettings, RetrySettings, Settings, load_settings, resolve_default_tools};
pub use thinking::ThinkingLevel;

/// 结果别名：本 crate 一律以 [`ConfigError`] 报错。
pub type Result<T> = std::result::Result<T, ConfigError>;
