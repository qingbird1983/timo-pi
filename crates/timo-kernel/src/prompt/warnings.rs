//! 资源装载告警（设计 01/02；项目信任两类见 12 §3.5）。

use std::path::PathBuf;

/// 资源装载告警。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningKind {
    /// 文件读取失败。
    Io,
    /// 技能名不合规（仍加载）。
    InvalidName,
    /// 描述缺失或超长。
    InvalidDescription,
    /// frontmatter 解析问题。
    Parse,
    /// 同名技能冲突（先到先得）。
    Collision,
    /// `--tools` 列表项没有命中任何已注册工具。
    UnknownTool,
    /// 项目信任存储读写问题（降级为无决策或保存失败）。
    TrustStore,
    /// 项目未受信任，部分项目资源被跳过。
    UntrustedProject,
}

/// 一条资源告警：装载不中止，逐条上报（`Kernel::resource_warnings`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceWarning {
    /// 告警种类。
    pub kind: WarningKind,
    /// 人读说明。
    pub message: String,
    /// 相关路径。
    pub path: PathBuf,
}
