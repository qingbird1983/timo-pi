//! 查询/动作的参数 schema（设计 03 §3、04 §2~§3）。
//!
//! 只做结构校验：未知/未声明由父模块判 E740，这里一律 E741。
//! 位置参数有上限，避免插件把宿主内存当草稿纸。

use serde_json::{Map, Value};

use super::{Action, HostError, NotifyLevel, Query};

/// 单次剪贴板/提示词文本上限。
const MAX_TEXT_BYTES: usize = 64 * 1024;
/// 通知正文上限。
const MAX_NOTICE_CHARS: usize = 1024;
/// 路径、会话 id 等标识串上限。
const MAX_IDENT_CHARS: usize = 1024;

pub(super) fn query(name: &str, params: &Value) -> Result<Query, HostError> {
    let object = fields(params)?;
    match name {
        "sessions.list" => Ok(Query::SessionsList {
            limit: number(object, "limit", 50, 1, 100)?,
            offset: number(object, "offset", 0, 0, 100_000)?,
        }),
        "sessions.search" => Ok(Query::SessionsSearch {
            query: text(object, "query", MAX_IDENT_CHARS)?,
            limit: number(object, "limit", 20, 1, 50)?,
        }),
        "sessions.get" => Ok(Query::SessionsGet {
            id: text(object, "id", MAX_IDENT_CHARS)?,
        }),
        "models.list" => Ok(Query::ModelsList),
        "theme.get" => Ok(Query::ThemeGet),
        "plugin.info" => Ok(Query::PluginInfo),
        "storage.get" => {
            let key = text(object, "key", 64)?;
            if !super::storage::is_valid_key(&key) {
                return Err(HostError::invalid(format!("invalid storage key `{key}`")));
            }
            Ok(Query::StorageGet { key })
        }
        other => Err(HostError::new("E740", format!("unknown query `{other}`"))),
    }
}

pub(super) fn action(name: &str, params: &Value) -> Result<Action, HostError> {
    let object = fields(params)?;
    match name {
        "notify" => Ok(Action::Notify {
            level: level(object)?,
            message: text(object, "message", MAX_NOTICE_CHARS)?,
        }),
        "openSession" => Ok(Action::OpenSession {
            session_id: text(object, "sessionId", MAX_IDENT_CHARS)?,
        }),
        "openPath" => Ok(Action::OpenPath {
            path: text(object, "path", MAX_IDENT_CHARS)?,
        }),
        "sendPrompt" => Ok(Action::SendPrompt {
            text: text(object, "text", MAX_TEXT_BYTES)?,
            submit: flag(object, "submit", false)?,
        }),
        "runTool" => Ok(Action::RunTool {
            name: text(object, "name", 64)?,
            arguments: match object.and_then(|map| map.get("arguments")) {
                None | Some(Value::Null) => Value::Object(Map::new()),
                Some(value @ Value::Object(_)) => value.clone(),
                Some(_) => return Err(HostError::invalid("`arguments` must be an object")),
            },
        }),
        "openPanel" => Ok(Action::OpenPanel {
            panel_id: text(object, "panelId", MAX_IDENT_CHARS)?,
        }),
        "clipboardWrite" => Ok(Action::ClipboardWrite {
            text: text(object, "text", MAX_TEXT_BYTES)?,
        }),
        "storage.set" => {
            let key = text(object, "key", 64)?;
            if !super::storage::is_valid_key(&key) {
                return Err(HostError::invalid(format!("invalid storage key `{key}`")));
            }
            let value = object
                .and_then(|map| map.get("value"))
                .cloned()
                .ok_or_else(|| HostError::invalid("missing `value`"))?;
            Ok(Action::StorageSet { key, value })
        }
        other => Err(HostError::new("E740", format!("unknown action `{other}`"))),
    }
}

/// 参数必须是对象（`null` 视作空对象）。
fn fields(params: &Value) -> Result<Option<&Map<String, Value>>, HostError> {
    if params.is_null() {
        return Ok(None);
    }
    params
        .as_object()
        .map(Some)
        .ok_or_else(|| HostError::invalid("params must be an object"))
}

fn value<'a>(object: Option<&'a Map<String, Value>>, name: &str) -> Option<&'a Value> {
    object.and_then(|map| map.get(name))
}

/// 必填非空字符串；长度按字符计。
fn text(object: Option<&Map<String, Value>>, name: &str, max: usize) -> Result<String, HostError> {
    let raw = value(object, name)
        .and_then(Value::as_str)
        .ok_or_else(|| HostError::invalid(format!("`{name}` is required and must be a string")))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(HostError::invalid(format!("`{name}` must not be empty")));
    }
    if trimmed.chars().count() > max {
        return Err(HostError::invalid(format!(
            "`{name}` exceeds {max} characters"
        )));
    }
    Ok(trimmed.to_string())
}

/// 可选布尔，缺省取 `default`。
fn flag(object: Option<&Map<String, Value>>, name: &str, default: bool) -> Result<bool, HostError> {
    match value(object, name) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(flag)) => Ok(*flag),
        Some(_) => Err(HostError::invalid(format!("`{name}` must be a boolean"))),
    }
}

/// 有界整数，缺省取 `default`。
fn number(
    object: Option<&Map<String, Value>>,
    name: &str,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize, HostError> {
    let parsed = match value(object, name) {
        None | Some(Value::Null) => default,
        Some(raw) => raw
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| {
                HostError::invalid(format!("`{name}` must be a non-negative integer"))
            })?,
    };
    if parsed < min || parsed > max {
        return Err(HostError::invalid(format!(
            "`{name}` must be within {min}..={max}"
        )));
    }
    Ok(parsed)
}

fn level(object: Option<&Map<String, Value>>) -> Result<NotifyLevel, HostError> {
    let raw = value(object, "level")
        .and_then(Value::as_str)
        .ok_or_else(|| HostError::invalid("`level` is required"))?;
    match raw {
        "info" => Ok(NotifyLevel::Info),
        "warn" => Ok(NotifyLevel::Warn),
        "error" => Ok(NotifyLevel::Error),
        other => Err(HostError::invalid(format!(
            "`level` must be info|warn|error, got `{other}`"
        ))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn list_bounds_and_defaults() {
        assert_eq!(number(None, "limit", 50, 1, 100).unwrap(), 50);
        let params = json!({"limit": 10, "offset": 3});
        let object = params.as_object();
        assert_eq!(number(object, "limit", 50, 1, 100).unwrap(), 10);
        assert_eq!(number(object, "offset", 0, 0, 100_000).unwrap(), 3);
        let negative = json!({"limit": -1});
        assert_eq!(
            number(negative.as_object(), "limit", 50, 1, 100)
                .unwrap_err()
                .code(),
            "E741"
        );
    }

    #[test]
    fn null_params_mean_empty() {
        assert_eq!(query("theme.get", &Value::Null).unwrap(), Query::ThemeGet);
        assert_eq!(
            query("models.list", &json!(null)).unwrap(),
            Query::ModelsList
        );
        assert_eq!(
            query("sessions.list", &json!({"limit": 7})).unwrap(),
            Query::SessionsList {
                limit: 7,
                offset: 0
            }
        );
    }

    #[test]
    fn storage_write_keeps_any_json_value() {
        let parsed = action("storage.set", &json!({"key": "a.b_c-1", "value": [1, 2]})).unwrap();
        assert_eq!(
            parsed,
            Action::StorageSet {
                key: "a.b_c-1".into(),
                value: json!([1, 2])
            }
        );
        assert_eq!(
            action("storage.set", &json!({"key": "a.b"}))
                .unwrap_err()
                .code(),
            "E741"
        );
    }

    #[test]
    fn tool_arguments_default_to_empty_object() {
        let action = action("runTool", &json!({"name": "read"})).unwrap();
        assert_eq!(
            action,
            Action::RunTool {
                name: "read".into(),
                arguments: json!({})
            }
        );
    }
}
