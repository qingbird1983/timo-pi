//! `openPath` 的目标校验（设计 03 §3）：解析后必须落在插件的 `fs.read` 作用域内。
//!
//! 能力判定留在 `timo-ext` 的路径检查里；这里只把错误统一成宿主错误码。

use std::path::{Path, PathBuf};

use super::HostError;

/// 校验 `openPath` 的目标：解析相对路径后必须落在插件的 `fs.read` 作用域内。
///
/// # Errors
/// 未授予、硬拒路径或越界（E4xx）→ 统一包装为 E722，原文保留在消息里。
pub fn check_open_path(
    work_dir: &Path,
    read_scopes: &[String],
    raw: &str,
) -> Result<PathBuf, HostError> {
    timo_ext::check_read(work_dir, read_scopes, raw).map_err(|error| HostError::new("E722", error))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn open_path_stays_inside_the_read_scope() {
        let cwd = std::env::temp_dir().join("timo-open-path");
        let scope = timo_ext::dir_scope(&cwd.join("notes"));
        let inside = check_open_path(&cwd, &[scope], "notes/today.md").unwrap();
        assert!(inside.ends_with("today.md"));
        let outside = check_open_path(
            &cwd,
            &[timo_ext::dir_scope(&cwd.join("notes"))],
            "secret.txt",
        )
        .unwrap_err();
        assert_eq!(outside.code(), "E722");
        assert!(outside.message().starts_with("E403"));
        let none = check_open_path(&cwd, &[], "notes/today.md").unwrap_err();
        assert!(none.message().starts_with("E401"));
    }
}
