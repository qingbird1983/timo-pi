//! 插件源码树指纹（设计 06 §3 实现注记）。
//!
//! 与 A 轨 `compute_source_hash` 同构（路径、长度、内容依次入哈希），但跳过
//! 构建产物、签名包、清单与工具生成的目录，保证同一份源码重复打包得到同一指纹。

use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

/// 目录名：不是源码，跳过整棵子树。
const SKIP_DIRS: [&str; 4] = [".git", "node_modules", ".nsh", "target"];
/// 文件名：清单与打包产物不参与指纹。
const SKIP_FILES: [&str; 2] = ["timo-plugin.json", "manifest.json"];

/// 计算 `root` 的源码指纹（`sha256:<64 hex>`）。
pub(crate) fn source_hash(root: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    collect(root, root, &mut files)?;
    if files.is_empty() {
        return Err(format!("no source files under {}", root.display()));
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    for (relative, absolute) in files {
        let bytes = std::fs::read(&absolute)
            .map_err(|error| format!("cannot read {}: {error}", absolute.display()))?;
        hasher.update(relative.as_bytes());
        hasher.update([0u8]);
        hasher.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(format!(
        "sha256:{}",
        super::nsh::hex_lower(&hasher.finalize())
    ))
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|error| format!("cannot list {}: {error}", dir.display()))?;
    let mut entries = entries
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot list {}: {error}", dir.display()))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot stat {}: {error}", path.display()))?;
        if meta.file_type().is_symlink() {
            return Err(format!(
                "symbolic links are not supported in plugin sources: {}",
                path.display()
            ));
        }
        if meta.is_dir() {
            if SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            collect(root, &path, out)?;
        } else if meta.is_file() {
            if SKIP_FILES.contains(&name.as_ref())
                || name.ends_with(".nshpkg")
                || name.ends_with(".map")
            {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|error| format!("cannot relativize {}: {error}", path.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, path));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, relative: &str, text: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn build_outputs_and_manifests_do_not_change_the_hash() {
        let source = tempfile::tempdir().unwrap();
        write(source.path(), "src/App.tsx", "export {};\n");
        let first = source_hash(source.path()).unwrap();

        write(source.path(), "timo-plugin.json", "{}");
        write(source.path(), "plugin.nshpkg", "package");
        write(source.path(), ".nsh/types/nsh.d.ts", "types");
        write(source.path(), "node_modules/x/index.js", "module");
        assert_eq!(source_hash(source.path()).unwrap(), first);

        write(
            source.path(),
            "src/App.tsx",
            "export const changed = true;\n",
        );
        assert_ne!(source_hash(source.path()).unwrap(), first);
    }

    #[test]
    fn paths_are_normalized_with_forward_slashes() {
        let source = tempfile::tempdir().unwrap();
        write(source.path(), "src/a.ts", "a");
        let hash = source_hash(source.path()).unwrap();
        assert!(hash.starts_with("sha256:"));
        assert_eq!(hash.len(), "sha256:".len() + 64);
    }
}
