//! 项目上下文文件与 SYSTEM/APPEND 文件（设计 phase-3.5/01 §5~§6）。

use std::path::{Path, PathBuf};

use super::{ResourceWarning, WarningKind};

/// 每目录的候选名，取第一个存在的普通文件（顺序即优先级）。
const CANDIDATES: [&str; 5] = [
    "AGENTS.override.md",
    "AGENTS.md",
    "AGENTS.MD",
    "CLAUDE.md",
    "CLAUDE.MD",
];

/// 一个已读取的项目上下文文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContextFile {
    /// 文件路径（渲染时使用）。
    pub path: PathBuf,
    /// 文件内容（已去 BOM）。
    pub content: String,
}

/// 发现上下文文件：全局在前，然后从根到 cwd；真实路径去重。
pub(crate) fn discover_context_files(
    cwd: &Path,
    data_dir: &Path,
) -> (Vec<ContextFile>, Vec<ResourceWarning>) {
    let mut warnings = Vec::new();
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if let Some(file) = read_first_in(data_dir, &mut warnings)
        && seen.insert(dedupe_key(&file.path))
    {
        files.push(file);
    }

    let start = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut ancestors = Vec::new();
    let mut current = Some(start.as_path());
    while let Some(dir) = current {
        if let Some(file) = read_first_in(dir, &mut warnings) {
            ancestors.push(file);
        }
        current = dir.parent();
    }
    for file in ancestors.into_iter().rev() {
        if seen.insert(dedupe_key(&file.path)) {
            files.push(file);
        }
    }

    (files, warnings)
}

/// 自定义提示词：项目 `<cwd>/.timo/SYSTEM.md` 优先，其次全局 `<data_dir>/SYSTEM.md`。
#[must_use]
pub(crate) fn custom_prompt(data_dir: &Path, cwd: &Path) -> Option<String> {
    read_prompt_file(&cwd.join(".timo").join("SYSTEM.md"))
        .or_else(|| read_prompt_file(&data_dir.join("SYSTEM.md")))
}

/// 追加系统提示：全局在前、项目在后，以空行连接；都缺失时为 None。
#[must_use]
pub(crate) fn append_system_prompt(data_dir: &Path, cwd: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for path in [
        data_dir.join("APPEND_SYSTEM.md"),
        cwd.join(".timo").join("APPEND_SYSTEM.md"),
    ] {
        if let Some(content) = read_prompt_file(&path) {
            parts.push(content.trim().to_string());
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

fn read_prompt_file(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let content = strip_bom(&raw).to_string();
    if content.trim().is_empty() {
        None
    } else {
        Some(content)
    }
}

fn read_first_in(dir: &Path, warnings: &mut Vec<ResourceWarning>) -> Option<ContextFile> {
    for name in CANDIDATES {
        let path = dir.join(name);
        if !path.is_file() {
            continue;
        }
        return match std::fs::read_to_string(&path) {
            Ok(raw) => Some(ContextFile {
                content: strip_bom(&raw).to_string(),
                path,
            }),
            Err(error) => {
                warnings.push(ResourceWarning {
                    kind: WarningKind::Io,
                    message: format!("read context file: {error}"),
                    path,
                });
                None
            }
        };
    }
    None
}

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

fn dedupe_key(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    canonical.display().to_string().to_lowercase()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(path, content).expect("write");
    }

    #[test]
    fn global_first_then_root_to_cwd() {
        let temp = tempfile::tempdir().expect("tempdir");
        let data = temp.path().join("data");
        let root = temp.path().join("repo");
        let work = root.join("work");
        write(&data.join("AGENTS.md"), "global");
        write(&root.join("AGENTS.md"), "root");
        write(&work.join("AGENTS.md"), "work");

        let (files, warnings) = discover_context_files(&work, &data);
        assert!(warnings.is_empty());
        let contents: Vec<&str> = files.iter().map(|f| f.content.as_str()).collect();
        assert_eq!(contents, ["global", "root", "work"]);
    }

    #[test]
    fn first_candidate_per_dir_wins() {
        let temp = tempfile::tempdir().expect("tempdir");
        let work = temp.path().join("w");
        write(&work.join("AGENTS.md"), "plain");
        write(&work.join("AGENTS.override.md"), "override");

        let (files, _) = discover_context_files(&work, &temp.path().join("none"));
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].content, "override");
    }

    #[test]
    fn bom_is_stripped() {
        let temp = tempfile::tempdir().expect("tempdir");
        let work = temp.path().join("w");
        write(&work.join("AGENTS.md"), "\u{feff}body");

        let (files, _) = discover_context_files(&work, &temp.path().join("none"));
        assert_eq!(files[0].content, "body");
    }

    #[test]
    fn duplicate_real_path_is_loaded_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let work = temp.path().join("w");
        write(&work.join("AGENTS.md"), "shared");

        // 全局目录与 cwd 相同：同一路径只能出现一次。
        let (files, _) = discover_context_files(&work, &work);
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn project_system_md_beats_global() {
        let temp = tempfile::tempdir().expect("tempdir");
        let data = temp.path().join("data");
        let work = temp.path().join("w");
        write(&data.join("SYSTEM.md"), "global prompt");
        write(&work.join(".timo/SYSTEM.md"), "project prompt");
        assert_eq!(
            custom_prompt(&data, &work).as_deref(),
            Some("project prompt")
        );

        std::fs::remove_file(work.join(".timo/SYSTEM.md")).expect("remove");
        assert_eq!(
            custom_prompt(&data, &work).as_deref(),
            Some("global prompt")
        );
    }

    #[test]
    fn append_joins_global_then_project() {
        let temp = tempfile::tempdir().expect("tempdir");
        let data = temp.path().join("data");
        let work = temp.path().join("w");
        write(&data.join("APPEND_SYSTEM.md"), "one");
        write(&work.join(".timo/APPEND_SYSTEM.md"), "two");
        assert_eq!(
            append_system_prompt(&data, &work).as_deref(),
            Some("one\n\ntwo")
        );
    }

    #[test]
    fn missing_files_yield_none() {
        let temp = tempfile::tempdir().expect("tempdir");
        let data = temp.path().join("data");
        let work = temp.path().join("w");
        assert_eq!(custom_prompt(&data, &work), None);
        assert_eq!(append_system_prompt(&data, &work), None);
        let (files, warnings) = discover_context_files(&work, &data);
        assert!(files.is_empty() && warnings.is_empty());
    }
}
