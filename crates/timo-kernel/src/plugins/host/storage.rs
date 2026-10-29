//! 插件私有 KV（设计 04 §4）：`<data_dir>/plugins/<id>/storage.json`。
//!
//! 键 `[A-Za-z0-9._-]{1,64}`；单条序列化 ≤ 64KB，总量 ≤ 1MB；超限 E744。
//! 写盘走「临时文件 + rename」，避免半条 JSON；不提供枚举与删除（v1）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::HostError;

/// 单条值的序列化上限。
pub const MAX_VALUE_BYTES: usize = 64 * 1024;
/// 整个文件的序列化上限。
pub const MAX_TOTAL_BYTES: usize = 1024 * 1024;

/// 一个插件的私有存储句柄。
#[derive(Debug, Clone)]
pub struct PluginStorage {
    path: PathBuf,
}

impl PluginStorage {
    /// 绑定到指定插件；id 非法（可穿越目录）→ E720。
    ///
    /// # Errors
    /// 插件 id 不符合 [`timo_ext::is_valid_id`]。
    pub fn new(data_dir: &Path, plugin_id: &str) -> Result<Self, HostError> {
        if !timo_ext::is_valid_id(plugin_id) {
            return Err(HostError::new(
                "E720",
                format!("invalid plugin id `{plugin_id}`"),
            ));
        }
        Ok(Self {
            path: data_dir
                .join("plugins")
                .join(plugin_id)
                .join("storage.json"),
        })
    }

    /// 存储文件位置（诊断用）。
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 读一个键；不存在返回 `None`（JSON `null`）。
    ///
    /// # Errors
    /// 文件不可读或损坏 → E744。
    pub fn get(&self, key: &str) -> Result<Option<Value>, HostError> {
        if !is_valid_key(key) {
            return Err(HostError::invalid(format!("invalid storage key `{key}`")));
        }
        Ok(self.load()?.get(key).cloned())
    }

    /// 写一个键；超限返回 E744 且旧文件不变。
    ///
    /// # Errors
    /// 键非法 → E741；超限/写盘失败 → E744。
    pub fn set(&self, key: &str, value: Value) -> Result<(), HostError> {
        if !is_valid_key(key) {
            return Err(HostError::invalid(format!("invalid storage key `{key}`")));
        }
        let value_bytes = serde_json::to_vec(&value)
            .map_err(|error| HostError::new("E744", error.to_string()))?;
        if value_bytes.len() > MAX_VALUE_BYTES {
            return Err(HostError::new(
                "E744",
                format!(
                    "value for `{key}` is {} bytes (limit {MAX_VALUE_BYTES})",
                    value_bytes.len()
                ),
            ));
        }
        let mut map = self.load()?;
        map.insert(key.to_string(), value);
        let bytes =
            serde_json::to_vec(&map).map_err(|error| HostError::new("E744", error.to_string()))?;
        if bytes.len() > MAX_TOTAL_BYTES {
            return Err(HostError::new(
                "E744",
                format!(
                    "plugin storage is {} bytes (limit {MAX_TOTAL_BYTES})",
                    bytes.len()
                ),
            ));
        }
        self.write(&bytes)
    }

    fn load(&self) -> Result<BTreeMap<String, Value>, HostError> {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeMap::new());
            }
            Err(error) => {
                return Err(HostError::new(
                    "E744",
                    format!("cannot read {}: {error}", self.path.display()),
                ));
            }
        };
        serde_json::from_str(&raw).map_err(|error| {
            HostError::new(
                "E744",
                format!("corrupt storage at {}: {error}", self.path.display()),
            )
        })
    }

    fn write(&self, bytes: &[u8]) -> Result<(), HostError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                HostError::new(
                    "E744",
                    format!("cannot create {}: {error}", parent.display()),
                )
            })?;
        }
        let temp = self.path.with_extension("json.tmp");
        if let Err(error) = std::fs::write(&temp, bytes) {
            let _ = std::fs::remove_file(&temp);
            return Err(HostError::new(
                "E744",
                format!("cannot write {}: {error}", temp.display()),
            ));
        }
        std::fs::rename(&temp, &self.path).map_err(|error| {
            let _ = std::fs::remove_file(&temp);
            HostError::new(
                "E744",
                format!("cannot replace {}: {error}", self.path.display()),
            )
        })
    }
}

/// 键规则：`[A-Za-z0-9._-]{1,64}`。
pub(super) fn is_valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use serde_json::json;

    use super::*;

    fn storage() -> (tempfile::TempDir, PluginStorage) {
        let dir = tempfile::tempdir().unwrap();
        let storage = PluginStorage::new(dir.path(), "demo-plugin").unwrap();
        (dir, storage)
    }

    #[test]
    fn roundtrip_and_overwrite() {
        let (_dir, storage) = storage();
        assert_eq!(storage.get("theme").unwrap(), None);
        storage.set("theme", json!({"mode": "dark"})).unwrap();
        assert_eq!(storage.get("theme").unwrap(), Some(json!({"mode": "dark"})));
        storage.set("theme", json!("light")).unwrap();
        assert_eq!(storage.get("theme").unwrap(), Some(json!("light")));
        assert!(storage.path().ends_with("plugins/demo-plugin/storage.json"));
    }

    #[test]
    fn keys_follow_the_contract() {
        let (dir, storage) = storage();
        let valid = ["a".to_string(), "A.b_c-9".to_string(), "x".repeat(64)];
        for key in &valid {
            storage.set(key, json!(1)).unwrap();
        }
        let invalid = [
            String::new(),
            "bad key".to_string(),
            "x/y".to_string(),
            "x".repeat(65),
        ];
        for key in &invalid {
            assert_eq!(storage.set(key, json!(1)).unwrap_err().code(), "E741");
            assert_eq!(storage.get(key).unwrap_err().code(), "E741");
        }
        assert!(PluginStorage::new(dir.path(), "../escape").is_err());
    }

    #[test]
    fn single_value_limit_keeps_old_data() {
        let (_dir, storage) = storage();
        storage.set("keep", json!("old")).unwrap();
        let big = "x".repeat(MAX_VALUE_BYTES);
        assert_eq!(storage.set("blob", json!(big)).unwrap_err().code(), "E744");
        assert_eq!(storage.get("keep").unwrap(), Some(json!("old")));
    }

    #[test]
    fn total_limit_is_enforced() {
        let (_dir, storage) = storage();
        let chunk = "y".repeat(65_000);
        let mut written = 0;
        let mut error = None;
        for index in 0..20 {
            match storage.set(&format!("k{index}"), json!(chunk)) {
                Ok(()) => written += 1,
                Err(reason) => {
                    error = Some(reason);
                    break;
                }
            }
        }
        assert_eq!(error.unwrap().code(), "E744");
        assert!(
            written >= 15,
            "limit should allow most of 1MB, wrote {written}"
        );
    }

    #[test]
    fn corrupt_file_is_reported() {
        let (dir, storage) = storage();
        std::fs::create_dir_all(dir.path().join("plugins/demo-plugin")).unwrap();
        std::fs::write(storage.path(), "{not json").unwrap();
        assert_eq!(storage.get("a").unwrap_err().code(), "E744");
    }
}
