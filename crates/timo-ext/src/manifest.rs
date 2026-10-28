//! manifest：能力声明与来源指纹（设计 02 §2.1、03 §3）。

use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Result;
use crate::error::ExtError;

/// 当前 manifest schema 版本。
pub const MANIFEST_SCHEMA: u32 = 1;

/// 扩展声明的能力集合（语义见设计 03 §2；本模块只承载数据）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Capabilities {
    /// 是否请求子进程执行能力。
    pub exec: bool,
    /// 网络能力（域名白名单）。
    pub net: NetCapability,
    /// 文件能力（作用域 glob）。
    pub fs: FsCapability,
}

/// 网络能力：域名白名单，空 = 无网。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct NetCapability {
    /// 允许的域名（`api.example.com` 或最左 `*.example.com`）。
    pub domains: Vec<String>,
}

/// 文件能力：读/写/删各自的作用域。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct FsCapability {
    /// 读作用域。
    pub read: Vec<String>,
    /// 写作用域（禁止全树）。
    pub write: Vec<String>,
    /// 删作用域（禁止全树）。
    pub delete: Vec<String>,
}

/// 扩展 manifest（安装期写入，加载期校验）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    /// schema 版本（当前为 [`MANIFEST_SCHEMA`]）。
    pub schema: u32,
    /// 扩展 id（`[a-z0-9][a-z0-9_-]{0,31}`）。
    pub id: String,
    /// 源码入口（相对扩展根）。
    pub entry: String,
    /// 打包产物（相对扩展根）。
    pub bundle: String,
    /// 源码指纹（`sha256:<64 hex>`）。
    pub source_hash: String,
    /// 打包器版本标记。
    pub bundler: String,
    /// 安装时间（ISO-8601；可选）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// 声明的能力。
    #[serde(default)]
    pub capabilities: Capabilities,
}

impl Manifest {
    /// 读取并校验 manifest；不存在返回 `None`。
    ///
    /// # Errors
    /// 读取失败 → [`ExtError::Read`]；JSON/schema 非法 → [`ExtError::InvalidManifest`]。
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(ExtError::Read {
                    path: path.to_path_buf(),
                    reason: error.to_string(),
                });
            }
        };
        let manifest: Self =
            serde_json::from_str(&text).map_err(|error| ExtError::InvalidManifest {
                path: path.to_path_buf(),
                reason: error.to_string(),
            })?;
        manifest.validate(path)?;
        Ok(Some(manifest))
    }

    /// 校验字段值域（schema、id、相对路径、指纹格式）。
    ///
    /// # Errors
    /// 任一字段非法 → [`ExtError::InvalidManifest`]。
    pub fn validate(&self, path: &Path) -> Result<()> {
        let invalid = |reason: String| ExtError::InvalidManifest {
            path: path.to_path_buf(),
            reason,
        };
        if self.schema != MANIFEST_SCHEMA {
            return Err(invalid(format!(
                "unsupported schema {} (expected {MANIFEST_SCHEMA})",
                self.schema
            )));
        }
        if !is_valid_id(&self.id) {
            return Err(invalid(format!("invalid id `{}`", self.id)));
        }
        if !is_safe_relative(&self.entry) {
            return Err(invalid(format!(
                "entry must be a safe relative path: `{}`",
                self.entry
            )));
        }
        if !is_safe_relative(&self.bundle) {
            return Err(invalid(format!(
                "bundle must be a safe relative path: `{}`",
                self.bundle
            )));
        }
        if !is_sha256_ref(&self.source_hash) {
            return Err(invalid(format!(
                "source_hash must look like `sha256:<64 hex>`: `{}`",
                self.source_hash
            )));
        }
        if self.bundler.trim().is_empty() {
            return Err(invalid("bundler must not be empty".to_string()));
        }
        crate::permissions::validate_capabilities(&self.id, &self.capabilities)?;
        Ok(())
    }

    /// 校验源码目录是否仍与 `source_hash` 一致。
    ///
    /// # Errors
    /// 目录不可读/含符号链接 → [`ExtError::Hash`]。
    pub fn verify_source_hash(&self, source_dir: &Path) -> Result<bool> {
        Ok(compute_source_hash(source_dir)? == self.source_hash)
    }
}

/// 计算源码目录的确定性指纹（按相对路径排序，逐文件 `路径\0长度\0内容`）。
///
/// # Errors
/// 目录不可读、含符号链接或为空 → [`ExtError::Hash`]。
pub fn compute_source_hash(source_dir: &Path) -> Result<String> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    collect_files(source_dir, source_dir, &mut files)?;
    if files.is_empty() {
        return Err(ExtError::Hash {
            path: source_dir.to_path_buf(),
            reason: "no files to hash".to_string(),
        });
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    for (relative, absolute) in files {
        let bytes = fs::read(&absolute).map_err(|error| ExtError::Hash {
            path: absolute.clone(),
            reason: error.to_string(),
        })?;
        hasher.update(relative.as_bytes());
        hasher.update([0u8]);
        hasher.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(format!("sha256:{}", hex_lower(&hasher.finalize())))
}

/// 扩展 id 规则：小写字母/数字开头，仅含 `[a-z0-9_-]`，长度 1~32。
#[must_use]
pub fn is_valid_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 32 {
        return false;
    }
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    id.chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
}

pub(crate) fn is_safe_relative(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let path = Path::new(value);
    path.is_relative()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

pub(crate) fn is_sha256_ref(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let entries = fs::read_dir(dir).map_err(|error| ExtError::Hash {
        path: dir.to_path_buf(),
        reason: error.to_string(),
    })?;
    let mut entries: Vec<_> = entries
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| ExtError::Hash {
            path: dir.to_path_buf(),
            reason: error.to_string(),
        })?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|error| ExtError::Hash {
            path: path.clone(),
            reason: error.to_string(),
        })?;
        if meta.file_type().is_symlink() {
            return Err(ExtError::Hash {
                path,
                reason: "symbolic links are not supported in source dirs".to_string(),
            });
        }
        if meta.is_dir() {
            collect_files(root, &path, out)?;
        } else if meta.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| ExtError::Hash {
                    path: path.clone(),
                    reason: error.to_string(),
                })?
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, path));
        }
    }
    Ok(())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn sample() -> Manifest {
        Manifest {
            schema: MANIFEST_SCHEMA,
            id: "demo".into(),
            entry: "source/index.ts".into(),
            bundle: "bundle.cjs".into(),
            source_hash: format!("sha256:{}", "a".repeat(64)),
            bundler: "rolldown-test".into(),
            created_at: None,
            capabilities: Capabilities::default(),
        }
    }

    #[test]
    fn roundtrip_and_defaults() {
        let json = r#"{
            "schema": 1,
            "id": "demo",
            "entry": "source/index.ts",
            "bundle": "bundle.cjs",
            "source_hash": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "bundler": "rolldown-test"
        }"#;
        let manifest: Manifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.capabilities, Capabilities::default());
        assert_eq!(manifest, sample());
    }

    #[test]
    fn validate_rejects_bad_values() {
        let path = Path::new("m.json");
        let mut bad = sample();
        bad.schema = 2;
        assert!(bad.validate(path).is_err());

        let mut bad = sample();
        bad.id = "Demo!".into();
        assert!(bad.validate(path).is_err());

        let mut bad = sample();
        bad.entry = "../escape.ts".into();
        assert!(bad.validate(path).is_err());

        let mut bad = sample();
        bad.source_hash = "sha256:xyz".into();
        assert!(bad.validate(path).is_err());
    }

    #[test]
    fn load_missing_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            Manifest::load(&dir.path().join("manifest.json"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn source_hash_is_deterministic_and_sensitive() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        write(&source.join("index.ts"), "export default () => {};\n");
        write(&source.join("nested/util.ts"), "export const x = 1;\n");
        let first = compute_source_hash(&source).unwrap();
        let second = compute_source_hash(&source).unwrap();
        assert_eq!(first, second);
        assert!(first.starts_with("sha256:"));

        write(&source.join("nested/util.ts"), "export const x = 2;\n");
        let changed = compute_source_hash(&source).unwrap();
        assert_ne!(first, changed);
    }

    #[test]
    fn source_hash_rejects_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        fs::create_dir_all(&source).unwrap();
        assert!(compute_source_hash(&source).is_err());
    }

    #[test]
    fn verify_source_hash_compares() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        write(&source.join("index.ts"), "export default () => {};\n");
        let hash = compute_source_hash(&source).unwrap();
        let manifest = Manifest {
            source_hash: hash,
            ..sample()
        };
        assert!(manifest.verify_source_hash(&source).unwrap());
        write(&source.join("index.ts"), "export default () => 1;\n");
        assert!(!manifest.verify_source_hash(&source).unwrap());
    }

    #[test]
    fn id_rules() {
        assert!(is_valid_id("demo"));
        assert!(is_valid_id("demo-2_x"));
        assert!(!is_valid_id(""));
        assert!(!is_valid_id("Demo"));
        assert!(!is_valid_id("-demo"));
        assert!(!is_valid_id(&"a".repeat(33)));
    }
}
