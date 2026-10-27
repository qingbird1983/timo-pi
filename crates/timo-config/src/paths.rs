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

/// 会话目录的项目分区名（phase-3.5/07：`--<encoded-cwd>--`，对齐 pi）。
///
/// 去掉开头分隔符，把 `/`、`\`、`:` 换成 `-`。不同 cwd 可能编码相同（与 pi 同限）。
#[must_use]
pub fn encode_cwd(cwd: &Path) -> String {
    let raw = cwd.to_string_lossy();
    let trimmed = raw.trim_start_matches(['/', '\\']);
    let encoded: String = trimmed
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') {
                '-'
            } else {
                c
            }
        })
        .collect();
    format!("--{encoded}--")
}

/// 会话文件的项目分区目录：`<sessions_root>/--<encoded-cwd>--`。
#[must_use]
pub fn session_dir_for(sessions_root: &Path, cwd: &Path) -> PathBuf {
    sessions_root.join(encode_cwd(cwd))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_partition_names_encode_the_cwd() {
        assert_eq!(encode_cwd(Path::new(r"C:\work\demo")), "--C--work-demo--");
        assert_eq!(encode_cwd(Path::new(r"D:\a\b-c")), "--D--a-b-c--");
        assert_eq!(encode_cwd(Path::new("/home/me/proj")), "--home-me-proj--");
        assert_eq!(encode_cwd(Path::new(r"C:\")), "--C----");
        assert_eq!(
            session_dir_for(Path::new("root"), Path::new(r"C:\work\demo")),
            Path::new("root").join("--C--work-demo--")
        );
    }

    #[test]
    fn project_settings_path_layout() {
        let p = project_settings_path(Path::new("D:/work/demo"));
        assert!(p.ends_with(".timo/settings.json"));
    }
}
