//! mcp.json 配置（设计 06 §1.1；对齐 pi mcp-servers.ts 结构）。

use std::path::Path;

use serde::{Deserialize, Serialize};

/// 默认每请求超时（秒；进度重置为 Phase 1 不做）。
pub const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// server 名规则：`^[A-Za-z0-9_-]+$`（pi 同构）。
#[must_use]
pub fn valid_server_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// 展平的传输字段（与 stdio/http JSON 形态一一对应；`type` 可省 → stdio）。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum KindFields {
    /// 本地子进程（stdio JSON-RPC）。
    Stdio {
        /// 可执行程序。
        command: String,
        /// 参数。
        #[serde(default)]
        args: Vec<String>,
        /// 环境变量（值支持 `${NAME}` / `!cmd`，经 timo-config ValueRef 解析）。
        #[serde(default)]
        env: std::collections::BTreeMap<String, String>,
        /// 工作目录。
        #[serde(default)]
        cwd: Option<String>,
    },
    /// streamable HTTP 端点。
    Http {
        /// URL（http(s)；SSE 旧端点不支持）。
        url: String,
        /// 附加请求头。
        #[serde(default)]
        headers: std::collections::BTreeMap<String, String>,
    },
}

impl<'de> Deserialize<'de> for KindFields {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        let ty = v
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("stdio");
        match ty {
            "stdio" => serde_json::from_value(v)
                .map(|s: StdioShim| Self::Stdio {
                    command: s.command,
                    args: s.args,
                    env: s.env,
                    cwd: s.cwd,
                })
                .map_err(serde::de::Error::custom),
            "http" => serde_json::from_value(v)
                .map(|h: HttpShim| Self::Http {
                    url: h.url,
                    headers: h.headers,
                })
                .map_err(serde::de::Error::custom),
            other => Err(serde::de::Error::custom(format!(
                "unknown server type: {other}"
            ))),
        }
    }
}

#[derive(serde::Deserialize)]
struct StdioShim {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(serde::Deserialize)]
struct HttpShim {
    url: String,
    #[serde(default)]
    headers: std::collections::BTreeMap<String, String>,
}

/// 注册表项：传输 + 生命周期 + 暴露面（默认 direct，D9；hidden=注册不可达）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    /// 传输形态。
    #[serde(flatten)]
    pub kind: KindFields,
    /// 是否启用（默认 true）。
    #[serde(default = "yes")]
    pub enabled: bool,
    /// 每请求超时（秒）。
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    /// 暴露面：`direct`（默认）| `hidden`。
    #[serde(default)]
    pub exposure: String,
    /// 每工具暴露面覆盖（精确名或 `*`）。
    #[serde(default)]
    pub tool_exposure: std::collections::BTreeMap<String, String>,
}

fn yes() -> bool {
    true
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECS
}

/// 解析单个 server 条目（校验 command/url）。
fn parse_entry(v: &serde_json::Value) -> std::result::Result<RegistryEntry, String> {
    let entry: RegistryEntry =
        serde_json::from_value(v.clone()).map_err(|e| format!("entry parse: {e}"))?;
    match &entry.kind {
        KindFields::Stdio { command, .. } if command.trim().is_empty() => {
            Err("stdio command must not be empty".into())
        }
        KindFields::Http { url, .. }
            if !url.starts_with("http://") && !url.starts_with("https://") =>
        {
            Err(format!("http url must be http(s): {url}"))
        }
        _ => Ok(entry),
    }
}

/// 加载全局 + 项目 mcp.json：全局为基底；项目文件仅可覆盖同名 server 的
/// `enabled/exposure/tool_exposure`（pi 同构）。文件缺失返回空注册表。
///
/// # Errors
/// 文件非法 JSON / server 名违规 / 项目文件新增 server（越权）→ [`crate::McpError`]。
pub fn load_server_registry(
    global: &Path,
    project: &Path,
) -> crate::Result<std::collections::BTreeMap<String, RegistryEntry>> {
    let mut out = parse_file(global)?;
    if project.exists() {
        for (name, entry) in parse_file(project)? {
            match out.get_mut(&name) {
                Some(base) => {
                    base.enabled = entry.enabled;
                    base.exposure = entry.exposure;
                    base.tool_exposure = entry.tool_exposure;
                }
                None => {
                    return Err(crate::McpError::ServerInvalid {
                        server: name,
                        reason: "project mcp.json may only override a global server".into(),
                    });
                }
            }
        }
    }
    for name in out.keys() {
        if !valid_server_name(name) {
            return Err(crate::McpError::ServerInvalid {
                server: name.clone(),
                reason: "server name must match ^[A-Za-z0-9_-]+$".into(),
            });
        }
    }
    Ok(out)
}

fn parse_file(path: &Path) -> crate::Result<std::collections::BTreeMap<String, RegistryEntry>> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(std::collections::BTreeMap::new());
        }
        Err(e) => {
            return Err(crate::McpError::ConfigInvalid {
                path: path.to_path_buf(),
                reason: e.to_string(),
            });
        }
    };
    let v: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| crate::McpError::ConfigInvalid {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
    let servers = v.get("servers").cloned().unwrap_or(serde_json::Value::Null);
    let map: std::collections::BTreeMap<String, serde_json::Value> =
        serde_json::from_value(servers).map_err(|e| crate::McpError::ConfigInvalid {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
    let mut out = std::collections::BTreeMap::new();
    for (name, entry_v) in map {
        let entry = parse_entry(&entry_v).map_err(|reason| crate::McpError::ServerInvalid {
            server: name.clone(),
            reason,
        })?;
        out.insert(name, entry);
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn server_name_rules() {
        assert!(valid_server_name("docs-server_1"));
        assert!(!valid_server_name("bad name"));
        assert!(!valid_server_name(""));
    }

    #[test]
    fn parse_stdio_and_http_entries() {
        let stdio: RegistryEntry = serde_json::from_value(serde_json::json!({
            "command": "npx", "args": ["-y", "srv"]
        }))
        .unwrap();
        assert!(matches!(stdio.kind, KindFields::Stdio { .. }));
        assert!(stdio.enabled && stdio.timeout == 60);
        let http: RegistryEntry = serde_json::from_value(serde_json::json!({
            "type": "http", "url": "https://example.com/mcp"
        }))
        .unwrap();
        assert!(matches!(http.kind, KindFields::Http { .. }));
        let bad: Result<RegistryEntry, _> =
            serde_json::from_value(serde_json::json!({"type": "http", "url": "ftp://x"}));
        assert!(
            bad.is_err()
                || parse_entry(&serde_json::json!({"type": "http", "url": "ftp://x"})).is_err()
        );
    }

    #[test]
    fn missing_files_are_empty() {
        let base = std::env::temp_dir().join(format!("timo-mcp-missing-{}", std::process::id()));
        let reg = load_server_registry(&base.join("a.json"), &base.join("b.json")).unwrap();
        assert!(reg.is_empty());
    }

    #[test]
    fn project_may_only_override_existing() {
        let dir = std::env::temp_dir().join(format!("timo-mcp-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let g = dir.join("global.json");
        let p = dir.join("project.json");
        std::fs::write(
            &g,
            r#"{"servers": {"docs": {"command": "npx", "args": ["x"], "timeout": 30}}}"#,
        )
        .unwrap();
        std::fs::write(
            &p,
            r#"{"servers": {"docs": {"command": "npx", "args": ["x"], "enabled": false}}}"#,
        )
        .unwrap();
        let reg = load_server_registry(&g, &p).unwrap();
        assert!(!reg["docs"].enabled);
        assert_eq!(reg["docs"].timeout, 30); // 项目不可覆盖 timeout
    }
}
