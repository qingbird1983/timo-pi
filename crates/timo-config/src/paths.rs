//! 目录与文件路径解析（设计 05 §1）。
//!
//! 优先级：env `TIMO_AGENT_DIR` > 平台数据目录（`dirs::data_dir()/timo-pi`）。

use std::path::{Path, PathBuf};

use crate::Result;
use crate::error::ConfigError;

/// 返回 agent 数据目录（存放 settings/models/mcp/sessions）。
///
/// # Errors
/// 平台数据目录不可定时返回 [`ConfigError::NotFound`]。
pub fn agent_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("TIMO_AGENT_DIR")
        && !dir.is_empty()
    {
        return Ok(PathBuf::from(dir));
    }
    dirs::data_dir()
        .map(|d| d.join("timo-pi"))
        .ok_or_else(|| ConfigError::NotFound(PathBuf::from("<platform data dir>")))
}

/// 全局设置文件路径：`<agent_dir>/settings.json`。
///
/// # Errors
/// 数据目录不可定时向上传播。
pub fn settings_path() -> Result<PathBuf> {
    Ok(agent_dir()?.join("settings.json"))
}

/// 自定义供应商文件路径：`<agent_dir>/models.json`。
///
/// # Errors
/// 数据目录不可定时向上传播。
pub fn models_path() -> Result<PathBuf> {
    Ok(agent_dir()?.join("models.json"))
}

/// MCP server 配置路径：`<agent_dir>/mcp.json`。
///
/// # Errors
/// 数据目录不可定时向上传播。
pub fn mcp_path() -> Result<PathBuf> {
    Ok(agent_dir()?.join("mcp.json"))
}

/// 会话目录：显式覆盖 > `<agent_dir>/sessions`（设计 04 §1）。
///
/// # Errors
/// 数据目录不可定且无覆盖时向上传播。
pub fn sessions_dir(override_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(dir) = override_dir {
        return Ok(dir.to_path_buf());
    }
    Ok(agent_dir()?.join("sessions"))
}

/// 项目设置文件路径：`<cwd>/.timo/settings.json`（存在才生效，调用方判断）。
#[must_use]
pub fn project_settings_path(cwd: &Path) -> PathBuf {
    cwd.join(".timo").join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_settings_path_layout() {
        let p = project_settings_path(Path::new("D:/work/demo"));
        assert!(p.ends_with(".timo/settings.json"));
    }
}
