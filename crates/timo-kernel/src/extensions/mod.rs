//! 扩展装载（设计 02 §3、03 §4）：发现 → 打包 → 授权 → QuickJS 装载 → 报告。
//!
//! 内核只通过本模块得到"工具元数据 + 钩子 + 诊断"，不认识扩展内部结构。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::json;
use timo_config::Clock;
use timo_ext::{
    BasicServices, Capabilities, DiagnosticEntry, DiagnosticReport, Discovered, DiscoveryOptions,
    ErrorCode, ExtHooks, ExtensionDiagnostic, ExtensionHost, Grants, LoadedExtension, Manifest,
    PackOptions, PermissionStore, Scope, Status, discover, pack, validate_capabilities,
};

mod cli;
mod events;

/// 单 handler 预算（设计 04 §5）。
pub const HOOK_BUDGET: Duration = Duration::from_secs(30);

/// 装载选项。
pub struct ExtensionOptions {
    /// agent 数据目录（`permissions.json` 与用户级扩展的根）。
    pub data_dir: PathBuf,
    /// 工作目录。
    pub work_dir: PathBuf,
    /// 显式 `--extension` 路径。
    pub explicit: Vec<PathBuf>,
    /// 是否交互授权（TTY）。
    pub interactive: bool,
    /// 是否扫描 `<cwd>/.pi/extensions/`。
    pub include_project_pi: bool,
    /// 是否扫描项目级扩展目录（项目未受信任时为 false，phase-3.5/12）。
    pub include_project: bool,
    /// 共享取消标志（与内核 run 同一个 epoch）。
    pub abort: Arc<AtomicBool>,
    /// `ctx.isIdle` 依据。
    pub running: Arc<AtomicBool>,
    /// `--ext-flag` 值。
    pub flags: std::collections::BTreeMap<String, String>,
    /// 目录快照（`ctx.modelRegistry`；设计 10 §4.2）。
    pub models: Vec<serde_json::Value>,
    /// 项目信任共享槽（`ctx.isProjectTrusted()`；pre-trust 装载时为 false）。
    pub project_trusted: Arc<std::sync::atomic::AtomicBool>,
}

/// 装载结果。
pub struct ExtensionRuntime {
    /// 扩展宿主门面（宿主线程启动失败时为 None）。
    pub host: Option<Arc<ExtensionHost>>,
    /// 循环/供应商钩子桥（随宿主一同缺席）。
    pub hooks: Option<Arc<ExtHooks>>,
    /// 成功装载的扩展。
    pub loaded: Vec<LoadedExtension>,
    /// 诊断报告。
    pub report: DiagnosticReport,
    /// 全部已注册命令。
    pub commands: Vec<String>,
    grants_path: PathBuf,
    absorbed: std::collections::HashMap<String, usize>,
}

impl ExtensionRuntime {
    /// 把 run 期间新产生的降级诊断并入报告（逐扩展逐成员）。
    pub fn absorb_runtime_errors(&mut self) {
        let Some(host) = &self.host else {
            return;
        };
        for extension in &mut self.report.extensions {
            let entries = host.diagnostics_for(&extension.id);
            let seen = self.absorbed.entry(extension.id.clone()).or_insert(0);
            for entry in entries.iter().skip(*seen) {
                if entry.code == ErrorCode::E302 || entry.code == ErrorCode::E202 {
                    let member = entry.message.clone();
                    if !extension.degraded.iter().any(|item| item.member == member) {
                        extension.degraded.push(timo_ext::DegradedItem {
                            member,
                            class: timo_ext::DegradeClass::Unsupported,
                            reason: "member is a no-op in this phase".into(),
                        });
                    }
                } else if !extension
                    .errors
                    .iter()
                    .any(|item| item.message == entry.message)
                {
                    extension.errors.push(entry.clone());
                }
            }
            *seen = entries.len();
        }
    }

    /// 持久化诊断报告（`diagnostics.json`）并返回 JSON。
    pub fn save_report(&self) -> String {
        let json = self.report.to_json_pretty().unwrap_or_default();
        if let Some(path) = self.grants_path.parent() {
            let _ = std::fs::write(path.join("extensions-diagnostics.json"), &json);
        }
        json
    }
}

/// 装载全部扩展；单个失败不影响其他扩展。
pub async fn load_extensions(options: ExtensionOptions) -> ExtensionRuntime {
    load_extensions_with(options, None).await
}

/// 同 [`load_extensions`]，可复用 pre-trust 装载的宿主与已装载扩展：
/// 只补装新出现的项目扩展，用户级不重复初始化（对齐 pi 的两遍装载，设计 12 §6）。
pub async fn load_extensions_with(
    options: ExtensionOptions,
    pre: Option<ExtensionRuntime>,
) -> ExtensionRuntime {
    let (pre_host, mut report, mut loaded, absorbed, already) = match pre {
        Some(pre) => {
            let already: std::collections::HashSet<String> =
                pre.loaded.iter().map(|ext| ext.id.clone()).collect();
            (pre.host, pre.report, pre.loaded, pre.absorbed, already)
        }
        None => (
            None,
            DiagnosticReport::new(now(), env!("CARGO_PKG_VERSION")),
            Vec::new(),
            std::collections::HashMap::new(),
            std::collections::HashSet::new(),
        ),
    };
    let grants_path = options.data_dir.join("permissions.json");
    let mut grants = PermissionStore::load(&grants_path).unwrap_or_default();
    let outcome = discover(&DiscoveryOptions {
        work_dir: options.work_dir.clone(),
        data_dir: Some(options.data_dir.clone()),
        explicit: options.explicit.clone(),
        include_project_pi: options.include_project_pi,
        include_project: options.include_project,
    });
    for issue in &outcome.issues {
        report.push(ExtensionDiagnostic {
            id: issue
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "?".to_string()),
            path: issue.path.display().to_string(),
            status: Status::Failed,
            source_hash: None,
            registered: Default::default(),
            degraded: vec![],
            errors: vec![
                DiagnosticEntry::new(issue.code, issue.message.clone())
                    .with_path(issue.path.display().to_string()),
            ],
        });
    }
    let base = BasicServices::new(options.work_dir.clone(), Grants::default(), json!({}))
        .with_agent_dir(options.data_dir.clone())
        .with_abort_flag(Arc::clone(&options.abort))
        .with_running(Arc::clone(&options.running))
        .with_flags(options.flags.clone())
        .with_models(options.models.clone())
        .with_project_trusted(Arc::clone(&options.project_trusted));
    let budget = std::env::var("TIMO_EXT_BUDGET_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(HOOK_BUDGET);
    let host = match pre_host {
        Some(host) => host,
        None => match ExtensionHost::start(Arc::new(base), budget) {
            Ok(host) => Arc::new(host),
            Err(error) => {
                report.push(ExtensionDiagnostic {
                    id: "extensions".to_string(),
                    path: String::new(),
                    status: Status::Failed,
                    source_hash: None,
                    registered: Default::default(),
                    degraded: vec![],
                    errors: vec![DiagnosticEntry::new(ErrorCode::E202, error.to_string())],
                });
                return ExtensionRuntime {
                    host: None,
                    hooks: None,
                    loaded,
                    report,
                    commands: vec![],
                    grants_path,
                    absorbed,
                };
            }
        },
    };
    for discovered in outcome.extensions {
        if already.contains(&discovered.id) {
            continue;
        }
        let id = discovered.id.clone();
        let path = discovered.root.display().to_string();
        let mut diagnostic = ExtensionDiagnostic::new(&id, &path, Status::Loaded);
        match prepare(&discovered, &options.data_dir).await {
            Ok((manifest, root)) => {
                diagnostic.source_hash = Some(manifest.source_hash.clone());
                if let Err(error) = validate_capabilities(&id, &manifest.capabilities) {
                    diagnostic.status = Status::Failed;
                    diagnostic.errors.push(diag_from(&error));
                    report.push(diagnostic);
                    continue;
                }
                let (capabilities, grant_state) = grants.effective(&id, &manifest.source_hash);
                let capabilities = if matches!(grant_state, timo_ext::GrantState::Granted) {
                    capabilities
                } else {
                    if matches!(grant_state, timo_ext::GrantState::Stale) {
                        diagnostic.errors.push(DiagnosticEntry::new(
                            ErrorCode::E406,
                            "source changed; previous grants were revoked",
                        ));
                    }
                    let granted = approve(&manifest, options.interactive);
                    if granted {
                        grants.grant(&manifest, now());
                        grants.save(&grants_path).ok();
                        manifest.capabilities.clone()
                    } else {
                        diagnostic.status = Status::Degraded;
                        diagnostic.errors.push(DiagnosticEntry::new(
                            ErrorCode::E405,
                            "capabilities are not granted; extension runs with all capabilities denied",
                        ));
                        Capabilities::default()
                    }
                };
                let services = Arc::new(
                    BasicServices::new(
                        options.work_dir.clone(),
                        Grants {
                            exec: capabilities.exec,
                            net_domains: capabilities.net.domains.clone(),
                            fs_read: capabilities.fs.read.clone(),
                        },
                        json!({}),
                    )
                    .with_agent_dir(options.data_dir.clone())
                    .with_ui(options.interactive)
                    .with_abort_flag(Arc::clone(&options.abort))
                    .with_running(Arc::clone(&options.running))
                    .with_flags(options.flags.clone())
                    .with_models(options.models.clone())
                    .with_project_trusted(Arc::clone(&options.project_trusted)),
                );
                let bundle = root.join(&manifest.bundle);
                match host.load_with(&id, &bundle, services) {
                    Ok(extension) => {
                        for entry in host.diagnostics_for(&id) {
                            if entry.code == ErrorCode::E302 || entry.code == ErrorCode::E202 {
                                if !diagnostic
                                    .degraded
                                    .iter()
                                    .any(|item| item.member == entry.message)
                                {
                                    diagnostic.degraded.push(timo_ext::DegradedItem {
                                        member: entry.message.clone(),
                                        class: timo_ext::DegradeClass::Unsupported,
                                        reason: "member is a no-op in this phase".into(),
                                    });
                                }
                            } else {
                                diagnostic.errors.push(entry);
                            }
                        }
                        diagnostic.registered.tools =
                            extension.tools.iter().map(|t| t.name.clone()).collect();
                        diagnostic.registered.commands = extension.commands.clone();
                        diagnostic.registered.hooks = extension.hooks.clone();
                        loaded.push(extension);
                    }
                    Err(error) => {
                        diagnostic.status = Status::Failed;
                        diagnostic.errors.push(diag_from(&error));
                    }
                }
            }
            Err(error) => {
                diagnostic.status = Status::Failed;
                diagnostic
                    .errors
                    .push(DiagnosticEntry::new(ErrorCode::E202, error.to_string()));
            }
        }
        report.push(diagnostic);
    }
    let commands = host.commands();
    let hooks =
        Arc::new(ExtHooks::new(Arc::clone(&host)).with_abort_flag(Arc::clone(&options.abort)));
    let runtime = ExtensionRuntime {
        host: Some(host),
        hooks: Some(hooks),
        loaded,
        report,
        commands,
        grants_path,
        absorbed,
    };
    runtime.save_report();
    runtime
}

/// 确保存在 `bundle.cjs + manifest.json`；必要时安装期打包。
async fn prepare(discovered: &Discovered, data_dir: &Path) -> crate::Result<(Manifest, PathBuf)> {
    if let Some(manifest) = &discovered.manifest
        && discovered.root.join(&manifest.bundle).is_file()
        && manifest.verify_source_hash(&discovered.root).is_ok()
    {
        return Ok((manifest.clone(), discovered.root.clone()));
    }
    let root = if discovered.scope == Scope::Explicit && !discovered.root.join("source").is_dir() {
        stage_explicit(discovered, data_dir)?
    } else {
        discovered.root.clone()
    };
    let entry = if root == discovered.root {
        discovered.entry.clone()
    } else {
        root.join("source").join(
            discovered
                .entry
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("index.ts")),
        )
    };
    let out = pack(PackOptions {
        root: root.clone(),
        entry,
        id: discovered.id.clone(),
        out_dir: root.clone(),
    })
    .await
    .map_err(|error| crate::KernelError::Extension(error.to_string()))?;
    Ok((out.manifest, root))
}

/// 显式文件装入缓存目录（`cache/ext/<sha12>/source/<name>`）。
fn stage_explicit(discovered: &Discovered, data_dir: &Path) -> crate::Result<PathBuf> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(&discovered.entry).map_err(|error| {
        crate::KernelError::Extension(format!(
            "cannot read {}: {error}",
            discovered.entry.display()
        ))
    })?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let root = data_dir.join("cache").join("ext").join(&digest[..12]);
    let source = root.join("source");
    std::fs::create_dir_all(&source)
        .map_err(|error| crate::KernelError::Extension(error.to_string()))?;
    let name = discovered
        .entry
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("index.ts"));
    std::fs::write(source.join(name), bytes)
        .map_err(|error| crate::KernelError::Extension(error.to_string()))?;
    Ok(root)
}

fn approve(manifest: &Manifest, interactive: bool) -> bool {
    let capabilities = &manifest.capabilities;
    if !capabilities.exec && capabilities.net.domains.is_empty() {
        return true;
    }
    if !interactive {
        return false;
    }
    eprintln!(
        "[ext] {} requests:{}",
        manifest.id,
        if capabilities.exec { " exec" } else { "" }
    );
    if !capabilities.net.domains.is_empty() {
        eprintln!("      net: {}", capabilities.net.domains.join(", "));
    }
    eprint!("grant these capabilities? [y/N] ");
    let _ = std::io::Write::flush(&mut std::io::stderr());
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).is_ok()
        && matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn diag_from(error: &timo_ext::ExtError) -> DiagnosticEntry {
    DiagnosticEntry::new(error.code().unwrap_or(ErrorCode::E202), error.to_string())
}

fn now() -> String {
    format!("unix:{}", (timo_config::SystemClock).now_ms())
}

pub use cli::{ExtensionSummary, install_extension, remove_extension, summarize, trust_extension};
pub use events::{forward_events, split_command, tool_definition};

/// 授权文件路径（CLI 复用）。
#[must_use]
pub fn permissions_path(data_dir: &Path) -> PathBuf {
    data_dir.join("permissions.json")
}
