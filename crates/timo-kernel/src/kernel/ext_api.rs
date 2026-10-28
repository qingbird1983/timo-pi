//! 扩展装载面的只读 API（诊断 / 命令发现 / 命令执行）。
//!
//! 从 `kernel.rs` 拆出以守住单文件行数预算（标准 08 §4）。

use super::Kernel;
use crate::Result;
use crate::error::KernelError;

impl Kernel {
    /// 扩展诊断报告（`timo ext doctor` / 会话开头摘要）。
    #[must_use]
    pub fn extension_report(&self) -> Option<&timo_ext::DiagnosticReport> {
        self.ext.as_ref().map(|runtime| &runtime.report)
    }

    /// 扩展诊断人读表。
    #[must_use]
    pub fn extension_report_human(&self) -> Option<String> {
        self.ext
            .as_ref()
            .map(|runtime| runtime.report.render_human())
    }

    /// 扩展目录快照（`ctx.modelRegistry`；设计 10 §4.2）。
    pub(crate) fn extension_models(&self) -> Vec<serde_json::Value> {
        self.catalog
            .all()
            .into_iter()
            .filter_map(|model| serde_json::to_value(model).ok())
            .collect()
    }

    /// 已注册扩展命令名（含 `plugin_<id>_` 前缀）。
    #[must_use]
    pub fn extension_commands(&self) -> Vec<String> {
        self.ext
            .as_ref()
            .map(|runtime| runtime.commands.clone())
            .unwrap_or_default()
    }

    /// 执行扩展命令。
    ///
    /// # Errors
    /// 未装载/命令缺失/执行失败 → [`KernelError::Extension`]。
    pub fn run_extension_command(&self, name: &str, args: serde_json::Value) -> Result<()> {
        let runtime = self
            .ext
            .as_ref()
            .ok_or_else(|| KernelError::Extension("extensions are not loaded".into()))?;
        runtime
            .host
            .as_ref()
            .ok_or_else(|| KernelError::Extension("extension host is not available".into()))?
            .run_command(name, args)
            .map(|_| ())
            .map_err(|error| KernelError::Extension(error.to_string()))
    }
}
