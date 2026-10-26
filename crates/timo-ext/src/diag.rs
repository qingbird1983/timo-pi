//! 诊断报告与错误码（设计 02 §5/§6）。
//!
//! 错误码是诊断、CLI 与测试断言的**唯一编号来源**；新增错误先在这里登记。

use serde::Serialize;

/// 诊断阶段（与错误码区段一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// 发现（目录/入口）。
    Discover,
    /// 安装期打包。
    Pack,
    /// 加载期（manifest/工厂/注册）。
    Load,
    /// 运行期（钩子/工具/定时器）。
    Runtime,
    /// 权限拒绝。
    Permission,
}

/// 诊断错误码；`E1xx` 打包/发现，`E2xx` 加载，`E3xx` 运行期，`E4xx` 权限。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum ErrorCode {
    /// 入口无法确定。
    E101,
    /// npm 依赖安装失败。
    E102,
    /// 模块解析失败。
    E103,
    /// 引用 `node:*` 或 Node 内建。
    E104,
    /// 非法依赖源、非字面量动态导入或动态求值。
    E105,
    /// 语法超出目标。
    E106,
    /// 打包器内部错误。
    E107,
    /// 产物/目录超限。
    E108,
    /// manifest 与产物哈希不一致。
    E109,
    /// manifest schema 非法。
    E110,
    /// 扩展显式版本检查失败。
    E201,
    /// 模块求值/工厂抛错或超时。
    E202,
    /// shim 缺失导出。
    E203,
    /// id/工具/命令/flag 冲突。
    E204,
    /// manifest 缺失或损坏。
    E205,
    /// 扩展目录不可读。
    E206,
    /// 钩子超时。
    E301,
    /// 钩子抛错或返回非法结构。
    E302,
    /// 工具抛错或返回非法结果。
    E303,
    /// 载荷/内容块类型非法。
    E304,
    /// 宿主通道关闭或扩展已卸载。
    E305,
    /// 定时器数量超限。
    E306,
    /// 能力未声明。
    E401,
    /// 域名不在白名单。
    E402,
    /// 作用域非法（写/删全树）。
    E403,
    /// 硬拒路径。
    E404,
    /// 用户拒绝授权或非 TTY 无授予。
    E405,
    /// 授权随源码变化失效。
    E406,
}

impl ErrorCode {
    /// 线格式（如 `"E104"`）。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::E101 => "E101",
            Self::E102 => "E102",
            Self::E103 => "E103",
            Self::E104 => "E104",
            Self::E105 => "E105",
            Self::E106 => "E106",
            Self::E107 => "E107",
            Self::E108 => "E108",
            Self::E109 => "E109",
            Self::E110 => "E110",
            Self::E201 => "E201",
            Self::E202 => "E202",
            Self::E203 => "E203",
            Self::E204 => "E204",
            Self::E205 => "E205",
            Self::E206 => "E206",
            Self::E301 => "E301",
            Self::E302 => "E302",
            Self::E303 => "E303",
            Self::E304 => "E304",
            Self::E305 => "E305",
            Self::E306 => "E306",
            Self::E401 => "E401",
            Self::E402 => "E402",
            Self::E403 => "E403",
            Self::E404 => "E404",
            Self::E405 => "E405",
            Self::E406 => "E406",
        }
    }

    /// 从线格式解析（如 `"E104"`）；未知码返回 `None`。
    #[must_use]
    pub fn parse(code: &str) -> Option<Self> {
        let code = match code {
            "E101" => Self::E101,
            "E102" => Self::E102,
            "E103" => Self::E103,
            "E104" => Self::E104,
            "E105" => Self::E105,
            "E106" => Self::E106,
            "E107" => Self::E107,
            "E108" => Self::E108,
            "E109" => Self::E109,
            "E110" => Self::E110,
            "E201" => Self::E201,
            "E202" => Self::E202,
            "E203" => Self::E203,
            "E204" => Self::E204,
            "E205" => Self::E205,
            "E206" => Self::E206,
            "E301" => Self::E301,
            "E302" => Self::E302,
            "E303" => Self::E303,
            "E304" => Self::E304,
            "E305" => Self::E305,
            "E306" => Self::E306,
            "E401" => Self::E401,
            "E402" => Self::E402,
            "E403" => Self::E403,
            "E404" => Self::E404,
            "E405" => Self::E405,
            "E406" => Self::E406,
            _ => return None,
        };
        Some(code)
    }

    /// 所属阶段。
    #[must_use]
    pub fn phase(self) -> Phase {
        match self {
            Self::E101 => Phase::Discover,
            Self::E102
            | Self::E103
            | Self::E104
            | Self::E105
            | Self::E106
            | Self::E107
            | Self::E108
            | Self::E109
            | Self::E110 => Phase::Pack,
            Self::E201 | Self::E202 | Self::E203 | Self::E204 | Self::E205 | Self::E206 => {
                Phase::Load
            }
            Self::E301 | Self::E302 | Self::E303 | Self::E304 | Self::E305 | Self::E306 => {
                Phase::Runtime
            }
            Self::E401 | Self::E402 | Self::E403 | Self::E404 | Self::E405 | Self::E406 => {
                Phase::Permission
            }
        }
    }
}

/// 扩展装载状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// 全部生效。
    Loaded,
    /// 有降级但可用。
    Degraded,
    /// 未装载。
    Failed,
}

/// 降级原因分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DegradeClass {
    /// TUI 面（no-op + 诊断）。
    Tui,
    /// 矩阵内缓做（v2）。
    Deferred,
    /// 矩阵外、本版不支持。
    Unsupported,
}

/// 一条降级项（按"扩展 × 成员"去重）。
#[derive(Debug, Clone, Serialize)]
pub struct DegradedItem {
    /// 成员路径（如 `ctx.ui.setWidget`）。
    pub member: String,
    /// 原因分类。
    pub class: DegradeClass,
    /// 人读原因。
    pub reason: String,
}

/// 该扩展成功注册的产物清单。
#[derive(Debug, Clone, Default, Serialize)]
pub struct RegisteredItems {
    /// 工具名（含命名空间前缀）。
    pub tools: Vec<String>,
    /// 命令名（含命名空间前缀）。
    pub commands: Vec<String>,
    /// 已订阅事件名。
    pub hooks: Vec<String>,
}

/// 一条错误诊断。
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticEntry {
    /// 错误码。
    pub code: ErrorCode,
    /// 所属阶段。
    pub phase: Phase,
    /// 人读原因。
    pub message: String,
    /// 定位（文件、行列或 URL）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// 补充信息（系统错误、栈摘要）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl DiagnosticEntry {
    /// 由错误码与原因构造（阶段自动推导）。
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            phase: code.phase(),
            message: message.into(),
            path: None,
            detail: None,
        }
    }

    /// 附加定位。
    #[must_use]
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// 附加补充信息。
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// 单个扩展的诊断。
#[derive(Debug, Clone, Serialize)]
pub struct ExtensionDiagnostic {
    /// 扩展 id。
    pub id: String,
    /// 扩展根路径。
    pub path: String,
    /// 装载状态。
    pub status: Status,
    /// 源码指纹（加载成功后填写）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    /// 成功注册的产物。
    pub registered: RegisteredItems,
    /// 降级项。
    pub degraded: Vec<DegradedItem>,
    /// 错误项。
    pub errors: Vec<DiagnosticEntry>,
}

impl ExtensionDiagnostic {
    /// 新建一条扩展诊断。
    #[must_use]
    pub fn new(id: impl Into<String>, path: impl Into<String>, status: Status) -> Self {
        Self {
            id: id.into(),
            path: path.into(),
            status,
            source_hash: None,
            registered: RegisteredItems::default(),
            degraded: Vec::new(),
            errors: Vec::new(),
        }
    }
}

/// 汇总计数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// 全部生效的扩展数。
    pub loaded: u32,
    /// 有降级的扩展数。
    pub degraded: u32,
    /// 装载失败的扩展数。
    pub failed: u32,
}

/// 整份诊断报告（覆盖式，非流水）。
#[derive(Debug, Clone)]
pub struct DiagnosticReport {
    /// 生成时间（ISO-8601，由调用方注入）。
    pub generated_at: String,
    /// 内核包版本。
    pub kernel_version: String,
    /// 逐扩展诊断。
    pub extensions: Vec<ExtensionDiagnostic>,
}

/// 序列化视图（`summary` 由 `extensions` 现算，避免双份事实）。
#[derive(Serialize)]
struct ReportView<'a> {
    generated_at: &'a str,
    kernel_version: &'a str,
    extensions: &'a [ExtensionDiagnostic],
    summary: Summary,
}

impl Serialize for DiagnosticReport {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ReportView {
            generated_at: &self.generated_at,
            kernel_version: &self.kernel_version,
            extensions: &self.extensions,
            summary: self.summary(),
        }
        .serialize(serializer)
    }
}

impl DiagnosticReport {
    /// 新建空报告。
    #[must_use]
    pub fn new(generated_at: impl Into<String>, kernel_version: impl Into<String>) -> Self {
        Self {
            generated_at: generated_at.into(),
            kernel_version: kernel_version.into(),
            extensions: Vec::new(),
        }
    }

    /// 追加一条扩展诊断。
    pub fn push(&mut self, extension: ExtensionDiagnostic) {
        self.extensions.push(extension);
    }

    /// 现算汇总。
    #[must_use]
    pub fn summary(&self) -> Summary {
        let mut summary = Summary::default();
        for ext in &self.extensions {
            match ext.status {
                Status::Loaded => summary.loaded += 1,
                Status::Degraded => summary.degraded += 1,
                Status::Failed => summary.failed += 1,
            }
        }
        summary
    }

    /// 序列化为 JSON（含现算的 `summary`）。
    ///
    /// # Errors
    /// 仅当内部结构无法序列化时失败（当前类型不会发生）。
    pub fn to_json_pretty(&self) -> std::result::Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// 人读表格（`timo ext doctor` 的正文）。
    #[must_use]
    pub fn render_human(&self) -> String {
        if self.extensions.is_empty() {
            return "（无扩展）\n".to_string();
        }
        let mut out = format!(
            "{:<16} {:<9} {:>5} {:>6} {:>9}  {}\n",
            "ID", "STATUS", "TOOLS", "HOOKS", "DEGRADED", "ERRORS"
        );
        for ext in &self.extensions {
            let errors = if ext.errors.is_empty() {
                "-".to_string()
            } else {
                ext.errors
                    .iter()
                    .map(|e| format!("{} {}", e.code.as_str(), truncate(&e.message, 48)))
                    .collect::<Vec<_>>()
                    .join("; ")
            };
            out.push_str(&format!(
                "{:<16} {:<9} {:>5} {:>6} {:>9}  {}\n",
                truncate(&ext.id, 16),
                status_label(ext.status),
                ext.registered.tools.len(),
                ext.registered.hooks.len(),
                ext.degraded.len(),
                errors
            ));
        }
        let summary = self.summary();
        out.push_str(&format!(
            "summary: loaded={} degraded={} failed={}\n",
            summary.loaded, summary.degraded, summary.failed
        ));
        out
    }
}

fn status_label(status: Status) -> &'static str {
    match status {
        Status::Loaded => "loaded",
        Status::Degraded => "degraded",
        Status::Failed => "failed",
    }
}

fn truncate(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
