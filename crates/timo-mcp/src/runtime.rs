//! MCP 运行时：惰性连接、工具发现、统一调用（设计 06 §1.2~1.3）。

use std::collections::BTreeMap;
use std::sync::Arc;

use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use serde_json::Value;
use timo_config::ValueRef;

use crate::Result;
use crate::config::RegistryEntry;
use crate::convert::{BoundCall, rmcp_tool_to_definition};
use crate::error::McpError;

/// server 连接状态（诊断用；失败不阻断内核启动）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerState {
    /// 未连接（惰性）。
    Idle,
    /// 已连接。
    Connected,
    /// 连接/调用失败（下次调用重试）。
    Failed(String),
}

type Client = Arc<RunningService<RoleClient, ()>>;

/// MCP 运行时：全局唯一，内部按 server 惰性建连。
pub struct McpRuntime {
    entries: BTreeMap<String, RegistryEntry>,
    conns: tokio::sync::Mutex<BTreeMap<String, Client>>,
    failed: tokio::sync::Mutex<BTreeMap<String, String>>,
}

impl McpRuntime {
    /// 由注册表构建（不连接——惰性）。
    #[must_use]
    pub fn new(entries: BTreeMap<String, RegistryEntry>) -> Self {
        Self {
            entries,
            conns: tokio::sync::Mutex::new(BTreeMap::new()),
            failed: tokio::sync::Mutex::new(BTreeMap::new()),
        }
    }

    /// Arc 化（供工具闭包捕获）。
    #[must_use]
    pub fn shared(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// 启用的 server 名单（注册顺序）。
    #[must_use]
    pub fn enabled_servers(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(_, e)| e.enabled)
            .map(|(n, _)| n.clone())
            .collect()
    }

    /// 当前状态快照。
    pub async fn state(&self, server: &str) -> ServerState {
        if self.conns.lock().await.contains_key(server) {
            ServerState::Connected
        } else if let Some(err) = self.failed.lock().await.get(server) {
            ServerState::Failed(err.clone())
        } else {
            ServerState::Idle
        }
    }

    /// 取连接；无则建立（stdio 子进程 / streamable HTTP）。失败记入 `Failed`
    /// 状态，下次调用重试。
    ///
    /// # Errors
    /// 子进程启动 / HTTP 握手失败 → [`McpError::ConnectFailed`]。
    pub async fn connect(&self, server: &str) -> Result<Client> {
        {
            let conns = self.conns.lock().await;
            if let Some(c) = conns.get(server) {
                return Ok(c.clone());
            }
        }
        let opened = self.open(server).await;
        match &opened {
            Ok(client) => {
                self.conns
                    .lock()
                    .await
                    .insert(server.to_string(), client.clone());
                self.failed.lock().await.remove(server);
            }
            Err(e) => {
                self.failed
                    .lock()
                    .await
                    .insert(server.to_string(), e.to_string());
            }
        }
        opened
    }

    /// 建立新连接（不读写缓存；失败记录由 [`McpRuntime::connect`] 负责）。
    async fn open(&self, server: &str) -> Result<Client> {
        let entry = self
            .entries
            .get(server)
            .ok_or_else(|| McpError::ServerInvalid {
                server: server.to_string(),
                reason: "unknown server".into(),
            })?;
        let client: Client = match &entry.kind {
            crate::config::KindFields::Stdio {
                command,
                args,
                env,
                cwd,
            } => {
                let mut cmd = tokio::process::Command::new(command);
                cmd.args(args);
                for (k, v) in env {
                    let resolved = ValueRef::parse(v).resolve().await.map_err(|e| {
                        McpError::ConnectFailed {
                            server: server.to_string(),
                            detail: format!("env {k}: {e}"),
                        }
                    })?;
                    cmd.env(k, resolved);
                }
                if let Some(dir) = cwd {
                    cmd.current_dir(dir);
                }
                let transport = TokioChildProcess::new(cmd.configure(|_| {})).map_err(|e| {
                    McpError::ConnectFailed {
                        server: server.to_string(),
                        detail: e.to_string(),
                    }
                })?;
                Arc::new(
                    ().serve(transport)
                        .await
                        .map_err(|e| McpError::ConnectFailed {
                            server: server.to_string(),
                            detail: e.to_string(),
                        })?,
                )
            }
            crate::config::KindFields::Http { url, headers } => {
                let http = build_http_client(headers).map_err(|e| McpError::ConnectFailed {
                    server: server.to_string(),
                    detail: e.to_string(),
                })?;
                let transport = StreamableHttpClientTransport::with_client(
                    http,
                    rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(
                        url.as_str(),
                    ),
                );
                Arc::new(
                    ().serve(transport)
                        .await
                        .map_err(|e| McpError::ConnectFailed {
                            server: server.to_string(),
                            detail: e.to_string(),
                        })?,
                )
            }
        };
        Ok(client)
    }

    /// 枚举 server 工具（原始 rmcp Tool）。
    ///
    /// # Errors
    /// 连接/枚举失败 → [`McpError::ConnectFailed`]。
    pub async fn list_tools(&self, server: &str) -> Result<Vec<rmcp::model::Tool>> {
        let client = self.connect(server).await?;
        let res =
            client
                .list_tools(Default::default())
                .await
                .map_err(|e| McpError::ConnectFailed {
                    server: server.to_string(),
                    detail: e.to_string(),
                })?;
        Ok(res.tools)
    }

    /// server 工具 → 统一 ToolDefinition（hidden 工具也注册但不可达，pi 同构）。
    ///
    /// # Errors
    /// 连接失败 → [`McpError`]。
    pub async fn definitions(
        self: &Arc<Self>,
        server: &str,
    ) -> Result<Vec<timo_tools::ToolDefinition>> {
        let entry = self
            .entries
            .get(server)
            .ok_or_else(|| McpError::ServerInvalid {
                server: server.to_string(),
                reason: "unknown server".into(),
            })?;
        let timeout = entry.timeout;
        let mut out = vec![];
        for tool in self.list_tools(server).await? {
            let ex = match tool_exposure(entry, &tool.name).as_str() {
                "hidden" => timo_tools::Exposure::Hidden,
                "deferred" | "codemode" => timo_tools::Exposure::Deferred,
                _ => timo_tools::Exposure::Direct,
            };
            let runtime = Arc::clone(self);
            let server2 = server.to_string();
            let tool_name = tool.name.to_string();
            let call: BoundCall = Arc::new(move |args: Value| {
                let runtime = Arc::clone(&runtime);
                let server2 = server2.clone();
                let tool_name = tool_name.clone();
                Box::pin(async move { runtime.call(&server2, &tool_name, args, timeout).await })
            });
            out.push(rmcp_tool_to_definition(server, &tool, ex, call));
        }
        Ok(out)
    }

    /// 统一调用入口（带超时；进度重置 Phase 1 不做）。
    ///
    /// # Errors
    /// 连接/调用失败 → [`McpError::CallFailed`]；超时 → [`McpError::Timeout`]。
    pub async fn call(
        &self,
        server: &str,
        tool: &str,
        args: Value,
        timeout_secs: u64,
    ) -> Result<CallToolResult> {
        let client = self.connect(server).await?;
        let mut params = CallToolRequestParams::new(tool.to_string());
        params.arguments = match args {
            Value::Object(m) => Some(m),
            Value::Null => None,
            other => {
                return Err(McpError::CallFailed {
                    tool: tool.to_string(),
                    detail: format!("arguments must be object: {other}"),
                });
            }
        };
        let fut = client.call_tool(params);
        tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), fut)
            .await
            .map_err(|_| McpError::Timeout {
                server: server.to_string(),
                secs: timeout_secs,
            })?
            .map_err(|e| McpError::CallFailed {
                tool: tool.to_string(),
                detail: e.to_string(),
            })
    }
}

/// 每工具暴露面：精确名覆盖 > `*` 覆盖 > server 默认（缺省 direct，D9）。
fn tool_exposure(entry: &RegistryEntry, tool_name: &str) -> String {
    if let Some(v) = entry.tool_exposure.get(tool_name) {
        return v.clone();
    }
    if let Some(v) = entry.tool_exposure.get("*") {
        return v.clone();
    }
    if entry.exposure.is_empty() {
        "direct".to_string()
    } else {
        entry.exposure.clone()
    }
}

/// 构造带默认头的 reqwest 客户端（rmcp 的 StreamableHttpClient 实现）。
fn build_http_client(
    headers: &std::collections::BTreeMap<String, String>,
) -> std::result::Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder();
    if !headers.is_empty() {
        let mut hm = reqwest::header::HeaderMap::new();
        for (k, v) in headers {
            let name = reqwest::header::HeaderName::from_bytes(k.as_bytes())
                .map_err(|e| format!("bad header name {k}: {e}"))?;
            let value = reqwest::header::HeaderValue::from_str(v)
                .map_err(|e| format!("bad header value {k}: {e}"))?;
            hm.insert(name, value);
        }
        builder = builder.default_headers(hm);
    }
    builder.build().map_err(|e| e.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn exposure_precedence() {
        let entry: RegistryEntry = serde_json::from_value(serde_json::json!({
            "command": "x",
            "exposure": "hidden",
            "tool_exposure": {"safe": "direct"}
        }))
        .unwrap();
        assert_eq!(tool_exposure(&entry, "safe"), "direct");
        assert_eq!(tool_exposure(&entry, "other"), "hidden");
        let plain: RegistryEntry =
            serde_json::from_value(serde_json::json!({"command": "x"})).unwrap();
        assert_eq!(tool_exposure(&plain, "any"), "direct"); // D9 默认 direct
    }
}
