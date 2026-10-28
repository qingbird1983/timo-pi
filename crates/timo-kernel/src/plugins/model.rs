//! 统一插件模型与错误（设计 01 §2）。

use std::path::PathBuf;

use timo_ext::{Capabilities, GuiManifestError, PermissionTier, PluginKind, SlotDecl};

/// 插件来源分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginOrigin {
    /// 内置功能配置入口。
    BuiltInConfig,
    /// 实验性产品开关。
    Experimental,
    /// 第三方已安装插件。
    Installed,
}

/// 安装来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginSource {
    /// 内置。
    BuiltIn,
    /// 本地文件/目录。
    LocalFile,
    /// 远程链接。
    Url,
    /// 市场安装。
    Market,
}

/// 插件状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    /// 已启用。
    Enabled,
    /// 已禁用。
    Disabled,
    /// 装载/校验失败。
    Error,
    /// 有可用更新。
    UpdateAvailable,
}

/// 清单声明的能力（申请，不等于授予）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Declared {
    /// 能力声明。
    pub capabilities: Capabilities,
    /// 界面槽位（仅 B 轨）。
    pub slots: Vec<SlotDecl>,
    /// 只读查询白名单。
    pub queries: Vec<String>,
    /// 动作白名单。
    pub actions: Vec<String>,
    /// 可调用工具名。
    pub tools: Vec<String>,
    /// 宿主 API 主版本。
    pub api: Option<u32>,
}

/// 列表中的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginEntry {
    /// 命名空间 id。
    pub id: String,
    /// 轨道。
    pub kind: PluginKind,
    /// 来源分组。
    pub origin: PluginOrigin,
    /// 展示名。
    pub name: String,
    /// 版本（A 轨旧清单为空）。
    pub version: String,
    /// 是否启用。
    pub enabled: bool,
    /// 实际档位。
    pub tier: PermissionTier,
    /// 清单声明。
    pub declared: Declared,
    /// 状态。
    pub state: PluginState,
    /// 安装来源。
    pub source: PluginSource,
    /// 指纹（A 轨源码 / B 轨清单声明）。
    pub source_hash: String,
    /// 是否有配置页。
    pub configurable: bool,
    /// 最近诊断。
    pub diagnostics: Vec<PluginDiagnostic>,
    /// 插件根目录（内置项为空）。
    pub path: PathBuf,
}

/// 一条插件诊断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDiagnostic {
    /// E7xx 错误码。
    pub code: String,
    /// 人读原因。
    pub message: String,
}

/// 安装结果摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    /// 插件 id。
    pub id: String,
    /// 轨道。
    pub kind: PluginKind,
    /// 采用的挡位。
    pub tier: PermissionTier,
    /// 版本（A 轨旧清单为空）。
    pub version: String,
}

/// 插件操作错误（E7xx）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginError {
    code: &'static str,
    message: String,
}

impl PluginError {
    /// 构造错误。
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// 错误码。
    #[must_use]
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// 人读原因。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PluginError {}

impl From<GuiManifestError> for PluginError {
    fn from(error: GuiManifestError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}
