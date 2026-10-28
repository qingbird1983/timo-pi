//! `trust.json` 读取、最近祖先查找与原子写入（设计 phase-3.5/12 §4）。
//!
//! 格式与 pi 对齐：`{ "<canonical path>": true | false }`，键排序、两空格、末尾换行。
//! 读取失败由调用方降级为"无决策 + 告警"，本模块**从不**覆盖读不懂的文件（D82）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::prompt::resources::canonical_key;

/// 一条生效的保存决策。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustStoreEntry {
    /// 决策对应的目录（保存时的规范化路径）。
    pub path: PathBuf,
    /// 信任与否。
    pub decision: bool,
}

/// 一条待写入的更新：`None` = 删除该目录的条目。
pub type TrustUpdate = (PathBuf, Option<bool>);

/// 项目信任存储。
pub struct TrustStore {
    path: PathBuf,
}

impl TrustStore {
    /// 存储位于 `<data_dir>/trust.json`。
    #[must_use]
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("trust.json"),
        }
    }

    /// 存储文件路径（告警与诊断用）。
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 从 cwd 逐级向上找最近的保存决策；没有返回 `None`。
    ///
    /// # Errors
    /// 文件存在但无法解析/结构非法时返回结构化错误文本。
    pub fn nearest(&self, cwd: &Path) -> Result<Option<TrustStoreEntry>, String> {
        let data = self.read()?;
        let start = normalize(cwd);
        let mut current = Some(start.as_path());
        while let Some(dir) = current {
            let key = canonical_key(dir);
            if let Some((path, decision)) = data
                .iter()
                .find(|(path, _)| canonical_key(path) == key)
                .map(|(path, decision)| (path, *decision))
            {
                return Ok(Some(TrustStoreEntry {
                    path: path.clone(),
                    decision,
                }));
            }
            current = dir.parent();
        }
        Ok(None)
    }

    /// 读-改-写：`None` 删除条目，其余写入布尔值；键排序后原子替换。
    ///
    /// # Errors
    /// 现有文件读不懂（不覆盖原文件）或写入失败。
    pub fn set_many(&self, updates: &[TrustUpdate]) -> Result<(), String> {
        let mut data = self.read()?;
        for (path, decision) in updates {
            let key = normalize(path);
            match decision {
                Some(value) => {
                    data.insert(key, *value);
                }
                None => {
                    data.remove(&key);
                }
            }
        }
        self.write(&data)
    }

    fn read(&self) -> Result<BTreeMap<PathBuf, bool>, String> {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeMap::new());
            }
            Err(error) => {
                return Err(format!(
                    "failed to read trust store {}: {error}",
                    self.path.display()
                ));
            }
        };
        let value: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|error| format!("invalid trust store {}: {error}", self.path.display()))?;
        let Some(object) = value.as_object() else {
            return Err(format!(
                "invalid trust store {}: expected an object",
                self.path.display()
            ));
        };
        let mut data = BTreeMap::new();
        for (key, value) in object {
            match value {
                serde_json::Value::Bool(decision) => {
                    data.insert(PathBuf::from(key), *decision);
                }
                serde_json::Value::Null => {}
                _ => {
                    return Err(format!(
                        "invalid trust store {}: value for {key:?} must be true or false",
                        self.path.display()
                    ));
                }
            }
        }
        Ok(data)
    }

    fn write(&self, data: &BTreeMap<PathBuf, bool>) -> Result<(), String> {
        let mut object = serde_json::Map::new();
        for (path, decision) in data {
            object.insert(path.to_string_lossy().into_owned(), (*decision).into());
        }
        let mut body = serde_json::to_string_pretty(&serde_json::Value::Object(object))
            .map_err(|error| error.to_string())?;
        body.push('\n');

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
        }
        let temp = self.path.with_extension("json.tmp");
        std::fs::write(&temp, body)
            .map_err(|error| format!("failed to write {}: {error}", temp.display()))?;
        std::fs::rename(&temp, &self.path)
            .map_err(|error| format!("failed to replace {}: {error}", self.path.display()))
    }
}

/// 写盘用的规范化路径：能 canonical 就用 canonical（剥 Windows verbatim 前缀），否则原样。
fn normalize(path: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    strip_verbatim(canonical)
}

/// `\\?\C:\...` → `C:\...`；`\\?\UNC\server\share` → `\\server\share`。
/// 保证存储键与用户给出的路径同形，便于阅读与跨工具比对。
fn strip_verbatim(path: PathBuf) -> PathBuf {
    let raw = path.to_string_lossy();
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match raw.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("timo-trust-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_has_no_decision() {
        let data = temp("missing");
        let store = TrustStore::new(&data);
        assert!(store.nearest(&data).unwrap().is_none());
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn nearest_ancestor_wins() {
        let data = temp("nearest");
        let parent = data.join("work");
        let child = parent.join("crate");
        fs::create_dir_all(&child).unwrap();
        let store = TrustStore::new(&data);
        store
            .set_many(&[(parent.clone(), Some(true)), (child.clone(), Some(false))])
            .unwrap();
        assert!(!store.nearest(&child).unwrap().unwrap().decision);
        assert!(store.nearest(&parent).unwrap().unwrap().decision);
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn writes_sorted_keys_and_round_trips() {
        let data = temp("sorted");
        let store = TrustStore::new(&data);
        store
            .set_many(&[(data.join("b"), Some(true)), (data.join("a"), Some(false))])
            .unwrap();
        let raw = fs::read_to_string(store.path()).unwrap();
        assert!(raw.ends_with('\n'));
        let sep = std::path::MAIN_SEPARATOR;
        let a = raw.find(&format!("{sep}a\"")).unwrap();
        let b = raw.find(&format!("{sep}b\"")).unwrap();
        assert!(a < b);
        assert!(!store.nearest(&data.join("a")).unwrap().unwrap().decision);
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn null_values_are_ignored() {
        let data = temp("null");
        let body = serde_json::to_string(&std::collections::BTreeMap::from([(
            data.display().to_string(),
            Option::<bool>::None,
        )]))
        .unwrap();
        fs::write(TrustStore::new(&data).path(), body).unwrap();
        let store = TrustStore::new(&data);
        assert!(store.nearest(&data).unwrap().is_none());
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn broken_file_is_reported_and_not_overwritten() {
        let data = temp("broken");
        let store = TrustStore::new(&data);
        fs::write(store.path(), "{not json").unwrap();
        assert!(store.nearest(&data).is_err());
        assert!(store.set_many(&[(data.clone(), Some(true))]).is_err());
        assert_eq!(fs::read_to_string(store.path()).unwrap(), "{not json");
        let _ = fs::remove_dir_all(&data);
    }
}
