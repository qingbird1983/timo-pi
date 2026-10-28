//! 项目信任门控（设计 phase-3.5/12）。
//!
//! 决策顺序：命令行覆盖 → 资源扫描 → 扩展事件（批 B）→ 最近保存决策 →
//! 全局缺省 → 交互询问 → 无 UI 拒绝。信任只决定"加载哪些项目资源"，
//! 不是安全边界，也不替代审阅（pi `docs/security.md` 口径）。

use std::future::Future;
use std::io::Write as _;
use std::path::Path;
use std::pin::Pin;

pub(crate) mod gate;
pub(crate) mod store;

pub use gate::{ProjectResource, ResourceKind};
pub use store::{TrustStore, TrustStoreEntry};

use store::TrustUpdate;

/// 决策来源（诊断与告警用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustSource {
    /// 命令行 `--approve` / `--no-approve`。
    Override,
    /// 没有任何需要门控的项目资源。
    NoResources,
    /// 保存过的目录决策（最近祖先）。
    Store,
    /// 全局设置 `default_project_trust`。
    Default,
    /// 扩展 `project_trust` 事件（批 B）。
    Extension,
    /// 交互询问（CLI 菜单或桌面回调）。
    Prompt,
    /// 需要询问但没有交互界面，按拒绝。
    NoUi,
}

impl TrustSource {
    /// 面向用户/日志的短名。
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Override => "command-line override",
            Self::NoResources => "no project resources",
            Self::Store => "saved decision",
            Self::Default => "default setting",
            Self::Extension => "extension decision",
            Self::Prompt => "interactive choice",
            Self::NoUi => "no interactive prompt",
        }
    }
}

/// 一次装配的信任结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTrust {
    /// 是否加载项目资源。
    pub trusted: bool,
    /// 决定从哪来。
    pub source: TrustSource,
    /// 不信任时被跳过的已存在资源（信任时为空）。
    pub skipped: Vec<ProjectResource>,
    /// 存储读写问题（降级为无决策或保存失败时的一条说明）。
    pub notice: Option<String>,
}

/// 交互选择的作用范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustScopeChoice {
    /// 只记当前目录。
    Directory,
    /// 记父目录（并清除当前目录条目）。
    Parent,
    /// 只对本次进程生效，不落盘。
    Session,
}

/// 扩展 `project_trust` 事件的决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtensionDecision {
    /// 扩展给出的结论。
    pub trusted: bool,
    /// 是否请求持久化（`remember: true`）。
    pub remember: bool,
}

/// 交互结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustChoice {
    /// 信任与否。
    pub trusted: bool,
    /// 是否落盘及落在哪。
    pub scope: TrustScopeChoice,
}

/// 询问所需的上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustRequest {
    /// 工作目录。
    pub cwd: std::path::PathBuf,
    /// 触发的项目资源。
    pub resources: Vec<ProjectResource>,
    /// 已保存的最近决策（若有）。
    pub saved: Option<TrustStoreEntry>,
}

/// 桌面等宿主提供的信任询问回调。
pub trait ProjectTrustPrompt: Send + Sync {
    /// 返回 `None` = 界面不可用/用户取消；调用方按"不信任（本次）"处理。
    fn ask<'a>(
        &'a self,
        request: TrustRequest,
    ) -> Pin<Box<dyn Future<Output = Option<TrustChoice>> + Send + 'a>>;
}

/// 决策输入。
pub struct ResolveInput<'a> {
    /// 工作目录。
    pub cwd: &'a Path,
    /// 数据目录（`trust.json` 所在）。
    pub data_dir: &'a Path,
    /// 全局设置（只读 `default_project_trust`）。
    pub global: &'a timo_config::Settings,
    /// 命令行覆盖（本进程，不落盘）。
    pub override_decision: Option<bool>,
    /// 扩展 `project_trust` 事件的决定（批 B；无扩展或未回答时为 `None`）。
    pub extension: Option<ExtensionDecision>,
    /// 宿主询问回调（桌面）；`None` 时按 `interactive` 决定是否用 stdin 菜单。
    pub prompt: Option<&'a dyn ProjectTrustPrompt>,
    /// 是否允许 stdin 询问（CLI 交互路径）。
    pub interactive: bool,
}

/// 解析本次装配的项目信任。
pub async fn resolve(input: ResolveInput<'_>) -> ProjectTrust {
    let resources = gate::scan(input.cwd);
    if let Some(trusted) = input.override_decision {
        return outcome(trusted, TrustSource::Override, resources, None);
    }
    if resources.is_empty() {
        return outcome(true, TrustSource::NoResources, resources, None);
    }

    let store = TrustStore::new(input.data_dir);
    let mut notice = None;
    if let Some(decision) = input.extension {
        if decision.remember
            && let Err(message) = save_choice(
                &store,
                input.cwd,
                TrustChoice {
                    trusted: decision.trusted,
                    scope: TrustScopeChoice::Directory,
                },
            )
        {
            notice = Some(message);
        }
        return outcome(decision.trusted, TrustSource::Extension, resources, notice);
    }
    let saved = match store.nearest(input.cwd) {
        Ok(entry) => entry,
        Err(message) => {
            notice = Some(message);
            None
        }
    };
    if let Some(entry) = &saved {
        return outcome(entry.decision, TrustSource::Store, resources, notice);
    }
    match input.global.default_project_trust {
        timo_config::DefaultProjectTrust::Always => {
            return outcome(true, TrustSource::Default, resources, notice);
        }
        timo_config::DefaultProjectTrust::Never => {
            return outcome(false, TrustSource::Default, resources, notice);
        }
        timo_config::DefaultProjectTrust::Ask => {}
    }

    let request = TrustRequest {
        cwd: input.cwd.to_path_buf(),
        resources: resources.clone(),
        saved,
    };
    let choice = match input.prompt {
        Some(prompt) => prompt.ask(request).await,
        None if input.interactive => ask_stdin(&request),
        None => None,
    };
    let Some(choice) = choice else {
        return outcome(false, TrustSource::NoUi, resources, notice);
    };
    if let Err(message) = save_choice(&store, input.cwd, choice) {
        notice = Some(message);
    }
    outcome(choice.trusted, TrustSource::Prompt, resources, notice)
}

fn outcome(
    trusted: bool,
    source: TrustSource,
    resources: Vec<ProjectResource>,
    notice: Option<String>,
) -> ProjectTrust {
    ProjectTrust {
        trusted,
        source,
        skipped: if trusted { Vec::new() } else { resources },
        notice,
    }
}

/// 是否值得为 `project_trust` 事件启动 pre-trust 扩展（设计 12 §6）。
///
/// 只有"有项目资源、无命令行覆盖、无保存决策、全局缺省为 ask"时才需要。
/// 存储损坏按无决策处理（与 [`resolve`] 的降级一致）。
#[must_use]
pub fn needs_extension_decision(
    cwd: &Path,
    data_dir: &Path,
    global: &timo_config::Settings,
    override_decision: Option<bool>,
) -> bool {
    if override_decision.is_some() || gate::scan(cwd).is_empty() {
        return false;
    }
    let store = TrustStore::new(data_dir);
    if matches!(store.nearest(cwd), Ok(Some(_))) {
        return false;
    }
    matches!(
        global.default_project_trust,
        timo_config::DefaultProjectTrust::Ask
    )
}

/// 按选择落盘；`Session` 不写。供 `/trust` 与交互路径共用。
///
/// # Errors
/// 存储读不懂或写入失败时返回说明。
pub fn save_choice(store: &TrustStore, cwd: &Path, choice: TrustChoice) -> Result<(), String> {
    let updates: Vec<TrustUpdate> = match choice.scope {
        TrustScopeChoice::Session => Vec::new(),
        TrustScopeChoice::Directory => vec![(cwd.to_path_buf(), Some(choice.trusted))],
        TrustScopeChoice::Parent => match cwd.parent() {
            Some(parent) if choice.trusted => vec![
                (parent.to_path_buf(), Some(true)),
                (cwd.to_path_buf(), None),
            ],
            _ => vec![(cwd.to_path_buf(), Some(choice.trusted))],
        },
    };
    if updates.is_empty() {
        return Ok(());
    }
    store.set_many(&updates)
}

/// CLI 行式菜单（对齐 pi 的五个选项；空行/EOF 视为不信任）。
#[must_use]
pub fn ask_stdin(request: &TrustRequest) -> Option<TrustChoice> {
    eprintln!("Trust project folder?");
    eprintln!("{}", request.cwd.display());
    eprintln!(
        "This allows timo to load .timo settings and resources, and execute project extensions."
    );
    for resource in &request.resources {
        eprintln!("  {}: {}", resource.kind.label(), resource.path.display());
    }
    let parent = request.cwd.parent().map(Path::to_path_buf);
    eprintln!("  1) Trust");
    if let Some(parent) = &parent {
        eprintln!("  2) Trust parent folder ({})", parent.display());
    }
    eprintln!("  3) Trust (this session only)");
    eprintln!("  4) Do not trust");
    eprintln!("  5) Do not trust (this session only)");
    eprint!("choice [1-5, default 4]: ");
    let _ = std::io::stderr().flush();

    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return None;
    }
    let choice = match line.trim() {
        "1" => TrustChoice {
            trusted: true,
            scope: TrustScopeChoice::Directory,
        },
        "2" if parent.is_some() => TrustChoice {
            trusted: true,
            scope: TrustScopeChoice::Parent,
        },
        "3" => TrustChoice {
            trusted: true,
            scope: TrustScopeChoice::Session,
        },
        "5" => TrustChoice {
            trusted: false,
            scope: TrustScopeChoice::Session,
        },
        _ => TrustChoice {
            trusted: false,
            scope: TrustScopeChoice::Directory,
        },
    };
    Some(choice)
}

#[cfg(test)]
mod tests;
