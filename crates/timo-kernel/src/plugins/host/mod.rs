//! TSX 宿主 API v1 的契约层（设计 03 §2~§3、04）。
//!
//! 只做「名字 → 挡位 → 参数」的 fail-closed 判定与只读数据快照，不链接 NSH；
//! 桌面隔离层把本模块的结论接到 `HostChannel` 上。未声明、未授予、未知一律拒绝。

mod data;
mod params;
mod path;
mod storage;
mod theme;

pub use data::{
    ModelSummary, SessionSummary, get_session, list_models, list_sessions, search_sessions,
};
pub use path::check_open_path;
pub use storage::PluginStorage;
pub use theme::{ThemePatch, ThemeWarning, sanitize_theme};

use serde_json::Value;
use timo_ext::PermissionTier;

/// 宿主 API 主版本（与清单 `api` 对齐）。
pub const API_VERSION: u32 = 1;

/// v1 只读查询（设计 04 §2）。
pub const QUERIES: &[&str] = &[
    "sessions.list",
    "sessions.search",
    "sessions.get",
    "models.list",
    "theme.get",
    "plugin.info",
    "storage.get",
];

/// v1 动作（设计 03 §3）。
pub const ACTIONS: &[&str] = &[
    "notify",
    "openSession",
    "openPath",
    "sendPrompt",
    "runTool",
    "openPanel",
    "clipboardWrite",
    "storage.set",
];

/// 插件的声明面（来自清单，不是授予）。
#[derive(Debug, Clone, Copy)]
pub struct Caller<'a> {
    /// 实际挡位。
    pub tier: PermissionTier,
    /// 清单声明的查询。
    pub queries: &'a [String],
    /// 清单声明的动作。
    pub actions: &'a [String],
    /// 清单声明的工具（`runTool` 用）。
    pub tools: &'a [String],
}

/// 通过校验的一次查询。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    /// 会话列表。
    SessionsList {
        /// 上限（1..=100）。
        limit: usize,
        /// 偏移。
        offset: usize,
    },
    /// 按 id 与首条消息匹配。
    SessionsSearch {
        /// 搜索词。
        query: String,
        /// 上限（1..=50）。
        limit: usize,
    },
    /// 单个会话元数据。
    SessionsGet {
        /// 会话 id。
        id: String,
    },
    /// 模型目录快照。
    ModelsList,
    /// 当前主题 token。
    ThemeGet,
    /// 本插件信息。
    PluginInfo,
    /// 读插件私有存储。
    StorageGet {
        /// 键。
        key: String,
    },
}

/// 通过校验的一次动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// 应用内通知。
    Notify {
        /// 级别。
        level: NotifyLevel,
        /// 文本。
        message: String,
    },
    /// 切换到已有会话。
    OpenSession {
        /// 会话 id。
        session_id: String,
    },
    /// 用系统默认应用打开路径。
    OpenPath {
        /// 路径。
        path: String,
    },
    /// 插入（或提交）提示词。
    SendPrompt {
        /// 文本。
        text: String,
        /// 是否直接提交。
        submit: bool,
    },
    /// 经宿主代理执行工具。
    RunTool {
        /// 工具名。
        name: String,
        /// 参数对象。
        arguments: Value,
    },
    /// 打开插件自己的面板。
    OpenPanel {
        /// 面板 id。
        panel_id: String,
    },
    /// 写剪贴板。
    ClipboardWrite {
        /// 文本（≤ 64KB）。
        text: String,
    },
    /// 写插件私有存储。
    StorageSet {
        /// 键。
        key: String,
        /// 任意 JSON 值。
        value: Value,
    },
}

/// 通知级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyLevel {
    /// 普通信息。
    Info,
    /// 警告。
    Warn,
    /// 错误。
    Error,
}

impl NotifyLevel {
    /// 清单里使用的级别名。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// 宿主拒绝一次调用的原因（E740/E741/E722；E742/E744 由运行时产生）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostError {
    code: &'static str,
    message: String,
}

impl HostError {
    /// 构造错误。
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// 错误码。
    #[must_use]
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// 人读原因。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::new("E741", message)
    }
}

impl std::fmt::Display for HostError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for HostError {}

/// 校验一次查询：未知/未声明 → E740，参数非法 → E741。
///
/// # Errors
/// 见函数说明。
pub fn parse_query(caller: &Caller<'_>, name: &str, params: &Value) -> Result<Query, HostError> {
    if !QUERIES.contains(&name) {
        return Err(HostError::new("E740", format!("unknown query `{name}`")));
    }
    if !caller.queries.iter().any(|declared| declared == name) {
        return Err(HostError::new(
            "E740",
            format!("query `{name}` is not declared in the manifest"),
        ));
    }
    params::query(name, params)
}

/// 校验一次动作：未知/未声明/工具未列 → E740，参数非法 → E741。
///
/// 挡位由 [`authorize`] 判定：`runTool` 的目标挡位取决于工具自身。
///
/// # Errors
/// 见函数说明。
pub fn parse_action(caller: &Caller<'_>, name: &str, params: &Value) -> Result<Action, HostError> {
    if !ACTIONS.contains(&name) {
        return Err(HostError::new("E740", format!("unknown action `{name}`")));
    }
    if !caller.actions.iter().any(|declared| declared == name) {
        return Err(HostError::new(
            "E740",
            format!("action `{name}` is not declared in the manifest"),
        ));
    }
    let action = params::action(name, params)?;
    if let Action::RunTool { name: tool, .. } = &action
        && !caller.tools.iter().any(|declared| declared == tool)
    {
        return Err(HostError::new(
            "E740",
            format!("tool `{tool}` is not listed in the manifest"),
        ));
    }
    Ok(action)
}

/// 挡位裁决（E722）。`tool_read_only_hint` 来自注册表注解，缺省按名字推断。
///
/// # Errors
/// 动作所需挡位高于调用方挡位。
pub fn authorize(
    caller: &Caller<'_>,
    action: &Action,
    tool_read_only_hint: Option<bool>,
) -> Result<(), HostError> {
    let needed = action.required_tier(tool_read_only_hint);
    if caller.tier < needed {
        return Err(HostError::new(
            "E722",
            format!("action needs tier {needed}, plugin runs at {}", caller.tier),
        ));
    }
    Ok(())
}

impl Action {
    /// 动作所需的最低挡位。
    #[must_use]
    pub fn required_tier(&self, tool_read_only_hint: Option<bool>) -> PermissionTier {
        match self {
            Self::Notify { .. }
            | Self::OpenSession { .. }
            | Self::OpenPanel { .. }
            | Self::ClipboardWrite { .. }
            | Self::StorageSet { .. } => PermissionTier::ReadOnly,
            Self::OpenPath { .. } | Self::SendPrompt { submit: false, .. } => {
                PermissionTier::Workspace
            }
            Self::SendPrompt { submit: true, .. } => PermissionTier::Trusted,
            Self::RunTool { name, .. } => required_tier_for_tool(name, tool_read_only_hint),
        }
    }
}

/// 工具执行所需挡位：只读注解 → ReadOnly；进程执行 → Trusted；其余 → Workspace。
#[must_use]
pub fn required_tier_for_tool(name: &str, read_only_hint: Option<bool>) -> PermissionTier {
    if read_only_hint == Some(true) {
        return PermissionTier::ReadOnly;
    }
    if matches!(name, "bash" | "powershell" | "shell") {
        return PermissionTier::Trusted;
    }
    PermissionTier::Workspace
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use serde_json::json;

    use super::*;

    fn caller<'a>(
        tier: PermissionTier,
        queries: &'a [String],
        actions: &'a [String],
        tools: &'a [String],
    ) -> Caller<'a> {
        Caller {
            tier,
            queries,
            actions,
            tools,
        }
    }

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn unknown_and_undeclared_queries_are_refused() {
        let declared = names(&["sessions.list"]);
        let actions = names(&[]);
        let tools = names(&[]);
        let who = caller(PermissionTier::ReadOnly, &declared, &actions, &tools);
        let error = parse_query(&who, "sessions.nope", &json!({})).unwrap_err();
        assert_eq!(error.code(), "E740");
        let error = parse_query(&who, "sessions.search", &json!({"query": "x"})).unwrap_err();
        assert_eq!(error.code(), "E740");
        let query = parse_query(&who, "sessions.list", &json!({})).unwrap();
        assert_eq!(
            query,
            Query::SessionsList {
                limit: 50,
                offset: 0
            }
        );
    }

    #[test]
    fn query_params_are_bounded() {
        let declared = names(&["sessions.list", "sessions.search", "storage.get"]);
        let actions = names(&[]);
        let tools = names(&[]);
        let who = caller(PermissionTier::ReadOnly, &declared, &actions, &tools);
        assert_eq!(
            parse_query(&who, "sessions.list", &json!({"limit": 101}))
                .unwrap_err()
                .code(),
            "E741"
        );
        assert_eq!(
            parse_query(&who, "sessions.search", &json!({"query": "  "}))
                .unwrap_err()
                .code(),
            "E741"
        );
        assert_eq!(
            parse_query(&who, "storage.get", &json!({"key": "bad key"}))
                .unwrap_err()
                .code(),
            "E741"
        );
        let query = parse_query(&who, "sessions.search", &json!({"query": "needle"})).unwrap();
        assert_eq!(
            query,
            Query::SessionsSearch {
                query: "needle".into(),
                limit: 20
            }
        );
    }

    #[test]
    fn actions_need_declaration_and_listed_tools() {
        let queries = names(&[]);
        let declared = names(&["notify", "runTool"]);
        let tools = names(&["read"]);
        let who = caller(PermissionTier::Trusted, &queries, &declared, &tools);
        assert_eq!(
            parse_action(&who, "openSession", &json!({"sessionId": "x"}))
                .unwrap_err()
                .code(),
            "E740"
        );
        assert_eq!(
            parse_action(&who, "runTool", &json!({"name": "write", "arguments": {}}))
                .unwrap_err()
                .code(),
            "E740"
        );
        let action = parse_action(&who, "runTool", &json!({"name": "read"})).unwrap();
        assert!(matches!(action, Action::RunTool { .. }));
    }

    #[test]
    fn tiers_follow_the_tool_class() {
        let queries = names(&[]);
        let actions = names(&["sendPrompt", "openPath", "runTool", "notify"]);
        let tools = names(&["read", "write", "bash"]);
        let read_only = caller(PermissionTier::ReadOnly, &queries, &actions, &tools);
        let workspace = caller(PermissionTier::Workspace, &queries, &actions, &tools);

        let prompt = parse_action(&workspace, "sendPrompt", &json!({"text": "hi"})).unwrap();
        authorize(&workspace, &prompt, None).unwrap();
        let submit = parse_action(
            &workspace,
            "sendPrompt",
            &json!({"text": "hi", "submit": true}),
        )
        .unwrap();
        assert_eq!(
            authorize(&workspace, &submit, None).unwrap_err().code(),
            "E722"
        );

        let open = parse_action(&read_only, "openPath", &json!({"path": "a.txt"})).unwrap();
        assert_eq!(
            authorize(&read_only, &open, None).unwrap_err().code(),
            "E722"
        );

        let read = parse_action(&read_only, "runTool", &json!({"name": "read"})).unwrap();
        authorize(&read_only, &read, Some(true)).unwrap();
        let write = parse_action(&read_only, "runTool", &json!({"name": "write"})).unwrap();
        assert_eq!(
            authorize(&read_only, &write, None).unwrap_err().code(),
            "E722"
        );
        let bash = parse_action(&workspace, "runTool", &json!({"name": "bash"})).unwrap();
        assert_eq!(
            authorize(&workspace, &bash, None).unwrap_err().code(),
            "E722"
        );
        let notify = parse_action(
            &read_only,
            "notify",
            &json!({"level": "info", "message": "ok"}),
        )
        .unwrap();
        authorize(&read_only, &notify, None).unwrap();
    }

    #[test]
    fn action_params_are_strict() {
        let queries = names(&[]);
        let actions = names(&["notify", "clipboardWrite"]);
        let tools = names(&[]);
        let who = caller(PermissionTier::ReadOnly, &queries, &actions, &tools);
        assert_eq!(
            parse_action(&who, "notify", &json!({"level": "loud", "message": "x"}))
                .unwrap_err()
                .code(),
            "E741"
        );
        assert_eq!(
            parse_action(&who, "notify", &json!({"level": "info"}))
                .unwrap_err()
                .code(),
            "E741"
        );
        let big = "x".repeat(64 * 1024 + 1);
        assert_eq!(
            parse_action(&who, "clipboardWrite", &json!({"text": big}))
                .unwrap_err()
                .code(),
            "E741"
        );
        assert_eq!(
            parse_action(&who, "notify", &json!(["not", "an", "object"]))
                .unwrap_err()
                .code(),
            "E741"
        );
    }
}
