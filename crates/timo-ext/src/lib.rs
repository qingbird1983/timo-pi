//! timo-ext：pi 编排扩展宿主（Phase 2；设计见 `docs/design/phase-2/`）。
//!
//! 当前落地：发现、manifest 与诊断骨架。打包、QuickJS 宿主与钩子按
//! `design/phase-2/00-overview.md` §8 的提交序列逐步接入。

// 测试代码自由使用 unwrap/expect（standards/04 §2）。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod bridge;
mod diag;
mod discovery;
mod error;
mod fs_scope;
mod gui_manifest;
mod host;
mod manifest;
#[cfg(feature = "pack")]
mod pack;
mod permissions;

pub use bridge::{ExtHooks, MAX_EVENT_BYTES};
pub use diag::{
    DegradeClass, DegradedItem, DiagnosticEntry, DiagnosticReport, ErrorCode, ExtensionDiagnostic,
    Phase, RegisteredItems, Status, Summary,
};
pub use discovery::{
    Discovered, DiscoveryIssue, DiscoveryOptions, DiscoveryOutcome, Scope, discover,
};
pub use error::ExtError;
pub use fs_scope::dir_scope;
pub use gui_manifest::{
    GUI_MANIFEST_SCHEMA, GuiManifest, GuiManifestError, HOST_API_VERSION, PermissionTier,
    PluginKind, SLOT_KINDS, SlotDecl, is_semver, is_valid_nsh_id,
};
pub use host::{
    BasicServices, DirItem, ExecOutput, ExecRequest, ExtensionHost, FetchOutput, Grants,
    HostServices, LoadedExtension, ToolCallResult, ToolMeta, VirtualModelMeta,
};
pub use manifest::{
    Capabilities, FsCapability, MANIFEST_SCHEMA, Manifest, NetCapability, compute_source_hash,
    is_valid_id,
};
#[cfg(feature = "pack")]
pub use pack::{MAX_BUNDLE_BYTES, PackOptions, PackOutput, pack};
pub use permissions::{
    Grant, GrantState, PERMISSIONS_SCHEMA, PermissionStore, domain_allowed, is_valid_domain,
    validate_capabilities,
};

/// 结果别名：本 crate 一律以 [`ExtError`] 报错。
pub type Result<T> = std::result::Result<T, ExtError>;
