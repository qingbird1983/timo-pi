//! 宿主服务契约（设计 01 §5、03 §5）：扩展触达 OS/用户的唯一通道。

use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

/// 已授予能力（v1 子集；完整规则见设计 03）。
#[derive(Debug, Clone, Default)]
pub struct Grants {
    /// 是否允许 `pi.exec`。
    pub exec: bool,
    /// 允许出网的域名（空 = 无网）。
    pub net_domains: Vec<String>,
    /// `fs.read` 作用域（空 = 不可读；设计 08 §4.1）。
    pub fs_read: Vec<String>,
}

/// 子进程执行结果。
#[derive(Debug, Clone, Serialize)]
pub struct ExecOutput {
    /// stdout。
    pub stdout: String,
    /// stderr。
    pub stderr: String,
    /// 退出码。
    pub code: i32,
}

/// 目录条目（`pi.listDir` 返回项）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirItem {
    /// 条目名（不含路径）。
    pub name: String,
    /// 是否目录。
    pub is_dir: bool,
}

/// 并发执行的一条子请求（`pi.execParallel`）。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ExecRequest {
    /// 可执行文件名。
    pub command: String,
    /// 参数表。
    pub args: Vec<String>,
    /// 超时（毫秒；None = 不设限）。
    pub timeout_ms: Option<u64>,
}

/// HTTP 获取结果。
#[derive(Debug, Clone, Serialize)]
pub struct FetchOutput {
    /// 状态码。
    pub status: u16,
    /// 响应头。
    pub headers: Vec<(String, String)>,
    /// 响应正文（文本）。
    pub body: String,
}

/// 宿主服务：扩展触达 OS/用户的唯一通道（由 CLI 或测试实现）。
pub trait HostServices: Send + Sync {
    /// 运行子进程；未授权返回含 `E401` 的错误文案。
    fn exec(
        &self,
        command: &str,
        args: &[String],
        options: &JsonValue,
    ) -> std::result::Result<ExecOutput, String>;
    /// 发起 HTTP 请求（默认未实现）。
    fn fetch(&self, url: &str) -> std::result::Result<FetchOutput, String> {
        let _ = url;
        Err("E401 network capability is not available".to_string())
    }
    /// 读文本文件（`fs.read` 作用域内；默认未实现）。
    fn read_file(&self, path: &str) -> std::result::Result<String, String> {
        Err(format!("E401 fs.read is not available: `{path}`"))
    }
    /// 列目录（`fs.read` 作用域内；默认未实现）。
    fn list_dir(&self, path: &str) -> std::result::Result<Vec<DirItem>, String> {
        Err(format!("E401 fs.read is not available: `{path}`"))
    }
    /// 并发执行子进程（默认未实现）。
    fn exec_parallel(
        &self,
        requests: Vec<ExecRequest>,
    ) -> std::result::Result<Vec<ExecOutput>, String> {
        let _ = requests;
        Err("E401 exec capability is not available".to_string())
    }
    /// 用户通知。
    fn notify(&self, message: &str, kind: &str);
    /// 选择对话框；无 UI 返回 `None`。
    fn select(&self, title: &str, options: &[String]) -> Option<String>;
    /// 确认对话框；无 UI 返回 false。
    fn confirm(&self, title: &str, message: &str) -> bool;
    /// 文本输入；无 UI 返回 `None`。
    fn input(&self, title: &str, placeholder: &str) -> Option<String>;
    /// `pi.*` 动作（getSettings/getAllTools/...）。
    fn action(&self, name: &str, payload: &JsonValue) -> JsonValue {
        let _ = (name, payload);
        JsonValue::Null
    }
    /// 传给 JS 的上下文配置（cwd/hasUI/model/...）。
    fn config(&self) -> JsonValue {
        json!({})
    }
    /// 当前 run 是否已取消。
    fn is_aborted(&self) -> bool {
        false
    }
    /// 请求取消当前 run。
    fn abort(&self) {}
    /// 是否有 run 在进行。
    fn is_running(&self) -> bool {
        false
    }
    /// 当前项目是否受信任（`ctx.isProjectTrusted()`；设计 phase-3.5/12 §6）。
    fn is_project_trusted(&self) -> bool {
        true
    }
}
