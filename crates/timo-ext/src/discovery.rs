//! 扩展发现与入口解析（设计 02 §2/§3.1；对齐 pi loader 的一层扫描语义）。
//!
//! 发现只做"找得到、读得懂"：不打包、不执行、不校验权限。任何单点失败都进
//! [`DiscoveryIssue`]，不影响其他扩展。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::Result;
use crate::diag::ErrorCode;
use crate::error::ExtError;
use crate::manifest::Manifest;

/// 发现来源（优先级从高到低：显式 > 项目 > 项目 pi 目录 > 用户）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    /// `--extension` 显式路径。
    Explicit,
    /// 项目级 `<cwd>/.timo/extensions/`。
    Project,
    /// 项目级 pi 兼容目录 `<cwd>/.pi/extensions/`。
    ProjectPi,
    /// 用户级 `<agent_dir>/extensions/`。
    User,
}

/// 发现选项。
#[derive(Debug, Clone)]
pub struct DiscoveryOptions {
    /// 工作目录（项目级扫描的基准）。
    pub work_dir: PathBuf,
    /// agent 数据目录（用户级扩展位于其下 `extensions/`）。
    pub data_dir: Option<PathBuf>,
    /// 显式路径（文件或目录）。
    pub explicit: Vec<PathBuf>,
    /// 是否扫描 `<cwd>/.pi/extensions/`（pi 兼容目录）。
    pub include_project_pi: bool,
    /// 是否扫描项目级目录（`.timo/extensions` 与按上一开关的 `.pi/extensions`）；
    /// 项目未受信任时为 false（设计 phase-3.5/12）。
    pub include_project: bool,
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            work_dir: PathBuf::from("."),
            data_dir: None,
            explicit: Vec::new(),
            include_project_pi: true,
            include_project: true,
        }
    }
}

/// 一个可装载的扩展候选（尚未执行）。
#[derive(Debug, Clone)]
pub struct Discovered {
    /// 扩展 id（manifest 优先，否则由目录/文件名派生）。
    pub id: String,
    /// 扩展根目录（loose 文件为其父目录）。
    pub root: PathBuf,
    /// 入口文件（绝对或相对发现基准的路径）。
    pub entry: PathBuf,
    /// 发现来源。
    pub scope: Scope,
    /// 已读取的 manifest（未安装时为 `None`）。
    pub manifest: Option<Manifest>,
}

/// 发现过程中的单点问题。
#[derive(Debug, Clone)]
pub struct DiscoveryIssue {
    /// 相关路径。
    pub path: PathBuf,
    /// 错误码（E101/E204/E205/E206）。
    pub code: ErrorCode,
    /// 人读原因。
    pub message: String,
}

/// 发现结果。
#[derive(Debug, Clone, Default)]
pub struct DiscoveryOutcome {
    /// 按 id 排序的候选。
    pub extensions: Vec<Discovered>,
    /// 发现期问题（不阻断其他扩展）。
    pub issues: Vec<DiscoveryIssue>,
}

/// 扫描全部发现源。
#[must_use]
pub fn discover(options: &DiscoveryOptions) -> DiscoveryOutcome {
    let mut collector = Collector::default();
    for path in &options.explicit {
        collector.path(path, Scope::Explicit);
    }
    if options.include_project {
        collector.dir(
            &options.work_dir.join(".timo").join("extensions"),
            Scope::Project,
        );
    }
    if options.include_project && options.include_project_pi {
        collector.dir(
            &options.work_dir.join(".pi").join("extensions"),
            Scope::ProjectPi,
        );
    }
    if let Some(data_dir) = &options.data_dir {
        collector.dir(&data_dir.join("extensions"), Scope::User);
    }
    collector.finish()
}

#[derive(Default)]
struct Collector {
    candidates: Vec<Discovered>,
    by_id: HashMap<String, usize>,
    issues: Vec<DiscoveryIssue>,
}

impl Collector {
    fn issue(&mut self, path: &Path, code: ErrorCode, message: impl Into<String>) {
        self.issues.push(DiscoveryIssue {
            path: path.to_path_buf(),
            code,
            message: message.into(),
        });
    }

    fn path(&mut self, path: &Path, scope: Scope) {
        let meta = match fs::metadata(path) {
            Ok(meta) => meta,
            Err(error) => {
                self.issue(path, ErrorCode::E206, error.to_string());
                return;
            }
        };
        if meta.is_file() {
            self.file(path, scope);
        } else if meta.is_dir() {
            self.load_dir(path, scope);
        } else {
            self.issue(path, ErrorCode::E206, "not a file or directory");
        }
    }

    fn dir(&mut self, dir: &Path, scope: Scope) {
        if !dir.is_dir() {
            return;
        }
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) => {
                self.issue(dir, ErrorCode::E206, error.to_string());
                return;
            }
        };
        let mut entries: Vec<_> = match entries.collect::<std::result::Result<Vec<_>, _>>() {
            Ok(entries) => entries,
            Err(error) => {
                self.issue(dir, ErrorCode::E206, error.to_string());
                return;
            }
        };
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            let path = entry.path();
            if path.is_file() {
                if is_extension_file(name) {
                    self.file(&path, scope);
                }
            } else if path.is_dir() {
                self.load_dir(&path, scope);
            }
        }
    }

    fn file(&mut self, path: &Path, scope: Scope) {
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            self.issue(path, ErrorCode::E101, "file name is not valid UTF-8");
            return;
        };
        let id = derive_id(stem);
        let root = path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        self.add(Discovered {
            id,
            root,
            entry: path.to_path_buf(),
            scope,
            manifest: None,
        });
    }

    fn load_dir(&mut self, dir: &Path, scope: Scope) {
        let (id, manifest) = match Manifest::load(&dir.join("manifest.json")) {
            Ok(Some(manifest)) => (manifest.id.clone(), Some(manifest)),
            Ok(None) => (
                derive_id(
                    dir.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("ext"),
                ),
                None,
            ),
            Err(error) => {
                let code = error.code().unwrap_or(ErrorCode::E205);
                self.issue(dir, code, error.to_string());
                return;
            }
        };
        let installed = manifest
            .as_ref()
            .map(|manifest| dir.join(&manifest.entry))
            .filter(|entry| entry.is_file());
        let entry = match installed {
            Some(entry) => entry,
            None => match resolve_entry(dir) {
                Ok(Some(entry)) => entry,
                Ok(None) => {
                    self.issue(
                        dir,
                        ErrorCode::E101,
                        "no entry (index.ts/index.js or package.json pi.extensions)",
                    );
                    return;
                }
                Err(error) => {
                    self.issue(dir, ErrorCode::E101, error.to_string());
                    return;
                }
            },
        };
        self.add(Discovered {
            id,
            root: dir.to_path_buf(),
            entry,
            scope,
            manifest,
        });
    }

    fn add(&mut self, candidate: Discovered) {
        if let Some(&index) = self.by_id.get(&candidate.id) {
            let existing_scope = self.candidates[index].scope;
            let existing_root = self.candidates[index].root.display().to_string();
            let replace = candidate.scope < existing_scope;
            let message = format!(
                "id `{}` already provided by {existing_root}; {}",
                candidate.id,
                if replace {
                    "higher-precedence source wins"
                } else {
                    "earlier source wins"
                }
            );
            self.issue(&candidate.root, ErrorCode::E204, message);
            if replace {
                self.candidates[index] = candidate;
            }
            return;
        }
        self.by_id
            .insert(candidate.id.clone(), self.candidates.len());
        self.candidates.push(candidate);
    }

    fn finish(mut self) -> DiscoveryOutcome {
        self.candidates.sort_by(|a, b| a.id.cmp(&b.id));
        DiscoveryOutcome {
            extensions: self.candidates,
            issues: self.issues,
        }
    }
}

/// 解析目录入口：`package.json` 的 `pi.extensions` → `index.ts/js` → 同名文件。
fn resolve_entry(dir: &Path) -> Result<Option<PathBuf>> {
    let package_json = dir.join("package.json");
    if package_json.is_file() {
        let text = fs::read_to_string(&package_json).map_err(|error| ExtError::Read {
            path: package_json.clone(),
            reason: error.to_string(),
        })?;
        let value: Value =
            serde_json::from_str(&text).map_err(|error| ExtError::InvalidManifest {
                path: package_json.clone(),
                reason: error.to_string(),
            })?;
        if let Some(entries) = value
            .get("pi")
            .and_then(|pi| pi.get("extensions"))
            .and_then(Value::as_array)
        {
            for entry in entries {
                if let Some(relative) = entry.as_str() {
                    let candidate = dir.join(relative);
                    if candidate.is_file() {
                        return Ok(Some(candidate));
                    }
                }
            }
        }
    }
    for name in ["index.ts", "index.js"] {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
    }
    let stem = dir.file_name().and_then(|name| name.to_str()).unwrap_or("");
    for extension in ["ts", "js"] {
        let candidate = dir.join(format!("{stem}.{extension}"));
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn is_extension_file(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("ts") || extension.eq_ignore_ascii_case("js")
        })
}

/// 把目录/文件名规范成合法 id（不合法字符折叠为 `-`）。
fn derive_id(name: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for character in name.chars().flat_map(char::to_lowercase) {
        let keep = character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || character == '_'
            || character == '-';
        let character = if keep { character } else { '-' };
        if character == '-' {
            if last_dash {
                continue;
            }
            last_dash = true;
        } else {
            last_dash = false;
        }
        out.push(character);
    }
    let trimmed = out.trim_matches(|character| character == '-' || character == '_');
    let mut id: String = trimmed.chars().take(32).collect();
    if id.is_empty() {
        return "ext".to_string();
    }
    if !id
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric())
    {
        id = format!("ext-{id}");
        id.truncate(32);
    }
    id
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn derive_id_normalizes_names() {
        assert_eq!(derive_id("Hello World"), "hello-world");
        assert_eq!(derive_id("my__ext"), "my__ext");
        assert_eq!(derive_id(".."), "ext");
        assert_eq!(derive_id("日本"), "ext");
        assert_eq!(derive_id("-lead"), "lead");
        assert!(derive_id(&"a".repeat(40)).len() <= 32);
    }
}
