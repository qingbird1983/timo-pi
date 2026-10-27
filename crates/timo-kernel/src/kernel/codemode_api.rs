//! codemode 注册（设计 phase-3.5/11 §4.2）：先占位让 `--tools` 认得，最终按注册表重建。

use timo_tools::Exposure;

use super::Kernel;

impl Kernel {
    /// 按 `--tools` 与设置决定激活面（`apply_allowlist` 收敛之后调用）。
    pub(crate) fn apply_codemode_activation(&mut self) {
        let enabled = match &self.tool_allowlist {
            Some(allowlist) => allowlist
                .iter()
                .any(|pattern| timo_tools::pattern_matches(pattern, "codemode")),
            None => self.codemode_active,
        };
        self.register_codemode(enabled);
    }

    /// 注册/重建 `codemode`；`enabled = false` 时保持 `Hidden`。
    ///
    /// 描述与脚本可调集合来自**当前**注册表，所以要在 MCP/扩展注册与
    /// `--tools` 收敛之后调用。
    pub(crate) fn register_codemode(&mut self, enabled: bool) {
        let catalog = crate::codemode::catalog(&self.registry);
        let mut definition = crate::codemode::definition(catalog);
        definition.exposure = if enabled {
            Exposure::ModelOnly
        } else {
            Exposure::Hidden
        };
        self.registry.register(definition);
    }
}
