//! 桌面壳的功能模块（会话侧栏、聊天区）。

pub(crate) mod chat;
mod chat_rows;
mod chat_view;
#[cfg(feature = "nsh-poc")]
pub(crate) mod nsh_poc;
pub(crate) mod palette;
pub(crate) mod review;
pub(crate) mod settings_credentials;
pub(crate) mod settings_view;
mod sidebar;
pub(crate) mod trust;
