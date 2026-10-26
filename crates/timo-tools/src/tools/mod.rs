//! 内置工具装配（设计 03 §1：默认激活集 = read/bash/edit/write，pi DEFAULT_TOOL_NAMES）。

pub mod edit;
pub mod find;
pub mod grep;
pub mod read;
pub mod shell;
pub mod write;

use std::path::Path;
use std::sync::Arc;

/// 全部内置工具定义（pi allToolNames 子集，D4：无 `ls`——见设计 00 §5）。
#[must_use]
pub fn builtin_definitions(cwd: &Path) -> Vec<crate::ToolDefinition> {
    let c: Arc<Path> = Arc::from(cwd);
    vec![
        read::definition(c.clone()),
        write::definition(c.clone()),
        edit::definition(c.clone()),
        shell::bash_definition(c.clone()),
        shell::powershell_definition(c.clone()),
        grep::definition(c.clone()),
        find::definition(c),
    ]
}

/// pi DEFAULT_TOOL_NAMES：Phase 1 默认激活集。
pub const DEFAULT_ACTIVE: &[&str] = &["read", "bash", "edit", "write"];
