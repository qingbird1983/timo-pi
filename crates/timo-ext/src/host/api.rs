//! 宿主服务契约（设计 01 §5、03 §5）：扩展触达 OS/用户的唯一通道。

use serde::Serialize;
use serde_json::{Value as JsonValue, json};

/// 已授予能力（v1 子集；完整规则见设计 03）。
#[derive(Debug, Clone, Default)]
pub struct Grants {
    /// 是否允许 `pi.exec`。
    pub exec: bool,
    /// 允许出网的域名（空 = 无网）。
    pub net_domains: Vec<String>,
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
}
