//! 内核桥接：专用 tokio 线程独占 `Kernel`，UI 只收发值类型（设计 01 §4~§5）。
//!
//! UI 线程不做阻塞 IO；run 期间只接受取消，会话切换在运行中会被拒绝。
//! 工具审阅经 [`crate::bridge::ReviewBroker`] 转成事件，等待 UI 的 oneshot 决定。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use timo_kernel::plugins::PermissionTier;
use timo_kernel::{
    Kernel, KernelOptions, ReviewDecision, ReviewKind, ToolReview, ToolReviewRequest,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;

use crate::settings::ReviewPrefs;

#[path = "bridge_dispatch.rs"]
mod dispatch;
#[path = "bridge_plugin_api.rs"]
mod plugin_api;
#[path = "bridge_providers.rs"]
mod providers;

#[path = "bridge_events.rs"]
mod bridge_events;
#[path = "bridge_plugins.rs"]
mod bridge_plugins;
#[allow(unused_imports)] // 测试与外部只通过门面取类型
pub use bridge_events::{BridgeEvent, MarketSnapshot};
pub use plugin_api::{PluginRequest, ToolCall};

/// UI 发给内核的命令。
#[derive(Debug, Clone)]
pub enum BridgeCommand {
    /// 重新扫描会话列表。
    RefreshSessions,
    /// 新建未落盘草稿。
    NewSession,
    /// 恢复会话并回填历史。
    OpenSession(String),
    /// 删除会话文件。
    DeleteSession(String),
    /// 运行一次 prompt。
    Run(String),
    /// 取消当前 run。
    Cancel,
    /// 更新审阅开关（只影响后续工具请求）。
    SetReviewPrefs {
        /// 文件改动前审阅。
        files: bool,
        /// 命令执行前审阅。
        commands: bool,
    },
    /// 读取已配置供应商与凭证来源。
    ListProviders,
    /// 保存某供应商的 API key 到钥匙串并改写引用。
    SaveProviderKey {
        /// 供应商 id。
        provider: String,
        /// API key 原文（只在内部通道传递，不回显）。
        secret: String,
    },
    /// 删除某供应商的钥匙串条目（配置引用保留）。
    RemoveProviderKey {
        /// 供应商 id。
        provider: String,
    },
    /// 重新扫描统一插件清单。
    ListPlugins,
    /// 启用/禁用插件。
    SetPluginEnabled {
        /// 插件 id。
        id: String,
        /// 目标状态。
        enabled: bool,
    },
    /// 收紧插件挡位。
    SetPluginTier {
        /// 插件 id。
        id: String,
        /// 目标挡位。
        tier: PermissionTier,
    },
    /// 卸载插件。
    UninstallPlugin {
        /// 插件 id。
        id: String,
    },
    /// 撤销插件授权（保留文件，置为禁用）。
    RevokePlugin {
        /// 插件 id。
        id: String,
    },
    /// 读取市场缓存与本地状态。
    LoadMarket,
    /// 拉取市场索引（失败保留缓存）。
    RefreshMarket,
    /// 保存市场索引地址（`None` = 清除）。
    SaveMarketIndex {
        /// HTTPS 地址。
        url: Option<String>,
    },
    /// 切换收藏。
    ToggleMarketFavorite {
        /// 插件 id。
        id: String,
    },
    /// 切换屏蔽。
    ToggleMarketBlocked {
        /// 插件 id。
        id: String,
    },
    /// 从市场下载到暂存目录（验签与挡位确认仍在桌面层完成）。
    InstallMarketPlugin {
        /// 插件 id。
        id: String,
    },
}

/// 供应商凭证来源（不含值；设计 02 §5）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialInfo {
    /// 系统钥匙串。
    Keychain {
        /// 服务名。
        service: String,
        /// 账号。
        account: String,
    },
    /// 环境变量引用。
    Env(String),
    /// 命令引用。
    Command,
    /// 配置文件中的明文。
    Literal,
    /// 内置供应商（环境变量，不在此处改写）。
    Builtin,
    /// 未配置。
    None,
}

/// 设置页展示的供应商信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderInfo {
    /// 供应商 id。
    pub id: String,
    /// 模型 id 列表。
    pub models: Vec<String>,
    /// 凭证来源。
    pub credential: CredentialInfo,
    /// 是否来自 models.json（可写钥匙串引用）。
    pub editable: bool,
}

/// 桥接两端：命令发送端 + 插件请求端 + 事件接收端。
pub struct Bridge {
    /// 发送命令。
    pub commands: UnboundedSender<BridgeCommand>,
    /// 插件宿主 API 的内核侧请求（`runTool`/`models.list`）。
    pub plugin: UnboundedSender<PluginRequest>,
    /// 接收事件。
    pub events: UnboundedReceiver<BridgeEvent>,
}

/// 启动专用内核线程；装配失败也会发 [`BridgeEvent::Fatal`]。
pub fn spawn(options: KernelOptions, review: ReviewPrefs) -> Bridge {
    let (command_tx, command_rx) = unbounded_channel();
    let (plugin_tx, plugin_rx) = unbounded_channel();
    let (event_tx, event_rx) = unbounded_channel();
    let prefs = Arc::new(Mutex::new(review));
    let data_dir = options.data_dir.clone();

    let mut options = options;
    options.tool_review = Some(Arc::new(ReviewBroker {
        events: event_tx.clone(),
        prefs: prefs.clone(),
    }));

    let thread_events = event_tx.clone();
    let spawned = std::thread::Builder::new()
        .name("timo-kernel".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run(
                    options,
                    data_dir,
                    prefs,
                    command_rx,
                    plugin_rx,
                    thread_events.clone(),
                )),
                Err(error) => {
                    let _ = thread_events.send(BridgeEvent::Fatal(format!(
                        "failed to start the kernel runtime: {error}"
                    )));
                }
            }
        });
    if let Err(error) = spawned {
        let _ = event_tx.send(BridgeEvent::Fatal(format!(
            "failed to start the kernel thread: {error}"
        )));
    }
    Bridge {
        commands: command_tx,
        plugin: plugin_tx,
        events: event_rx,
    }
}

/// UI 侧审阅器：把请求交给界面并等待 `oneshot` 决定。
struct ReviewBroker {
    events: UnboundedSender<BridgeEvent>,
    prefs: Arc<Mutex<ReviewPrefs>>,
}

#[async_trait::async_trait]
impl ToolReview for ReviewBroker {
    async fn review(&self, request: ToolReviewRequest) -> ReviewDecision {
        let enabled = {
            let Ok(prefs) = self.prefs.lock() else {
                return ReviewDecision::Reject {
                    reason: "review unavailable".into(),
                };
            };
            match &request.kind {
                ReviewKind::FileChange { .. } => prefs.files,
                ReviewKind::Command { .. } => prefs.commands,
            }
        };
        if !enabled {
            return ReviewDecision::Allow;
        }
        let (respond, decision) = oneshot::channel();
        if self
            .events
            .send(BridgeEvent::Review { request, respond })
            .is_err()
        {
            return ReviewDecision::Reject {
                reason: "review unavailable".into(),
            };
        }
        match decision.await {
            Ok(decision) => decision,
            Err(_) => ReviewDecision::Reject {
                reason: "run cancelled".into(),
            },
        }
    }
}

async fn run(
    options: KernelOptions,
    data_dir: Option<PathBuf>,
    prefs: Arc<Mutex<ReviewPrefs>>,
    mut commands: UnboundedReceiver<BridgeCommand>,
    mut requests: UnboundedReceiver<PluginRequest>,
    event_tx: UnboundedSender<BridgeEvent>,
) {
    let mut kernel = match Kernel::new(options).await {
        Ok(kernel) => kernel,
        Err(error) => {
            let _ = event_tx.send(BridgeEvent::Fatal(error.to_string()));
            return;
        }
    };

    forward_events(&kernel, event_tx.clone());

    let sessions = kernel.list_sessions();
    let _ = event_tx.send(BridgeEvent::Ready {
        model: kernel.model_label(),
        sessions,
    });
    // 只建提示与技能（不触发 MCP 连接），让面板在首轮前就有命令可列。
    kernel.reload_resources();
    let _ = event_tx.send(BridgeEvent::Commands(kernel.slash_commands()));
    let _ = event_tx.send(BridgeEvent::Usage(kernel.usage_summary()));

    let mut requests_open = true;
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(BridgeCommand::Run(prompt)) => {
                    run_once(&mut kernel, prompt, &mut commands, &event_tx).await;
                }
                Some(command) => {
                    dispatch::dispatch(command, &mut kernel, data_dir.as_deref(), &prefs, &event_tx)
                        .await;
                }
                None => break,
            },
            // 插件请求在 run 期间排队；被放弃的调用由 `serve` 跳过副作用。
            request = requests.recv(), if requests_open => match request {
                Some(request) => plugin_api::serve(&mut kernel, request).await,
                None => requests_open = false,
            },
        }
    }
}

fn forward_events(kernel: &Kernel, event_tx: UnboundedSender<BridgeEvent>) {
    let mut kernel_events = kernel.events();
    tokio::spawn(async move {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match kernel_events.recv().await {
                Ok(event) => {
                    if event_tx.send(BridgeEvent::Kernel(event)).is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    if event_tx.send(BridgeEvent::Lagged(skipped)).is_err() {
                        break;
                    }
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}

async fn run_once(
    kernel: &mut Kernel,
    prompt: String,
    commands: &mut UnboundedReceiver<BridgeCommand>,
    event_tx: &UnboundedSender<BridgeEvent>,
) {
    // 先装配 agent，才能在 run 期间用独立句柄取消（O(1)，后续 run 不再重复连接）。
    let _ = kernel.tools().await;
    let abort = kernel.abort_handle();
    let _ = event_tx.send(BridgeEvent::Running);

    {
        let run = kernel.run(&prompt);
        tokio::pin!(run);
        let mut commands_open = true;
        loop {
            tokio::select! {
                output = &mut run => {
                    if let Err(error) = output {
                        let _ = event_tx.send(BridgeEvent::RunFailed(error.to_string()));
                    }
                    let _ = event_tx.send(BridgeEvent::Finished);
                    break;
                }
                command = commands.recv(), if commands_open => match command {
                    Some(BridgeCommand::Cancel) => {
                        if let Some(handle) = &abort {
                            handle.abort();
                        }
                    }
                    Some(_) => {
                        let _ = event_tx.send(BridgeEvent::Busy);
                    }
                    None => {
                        commands_open = false;
                        if let Some(handle) = &abort {
                            handle.abort();
                        }
                    }
                },
            }
        }
    }
    send_sessions(kernel, event_tx);
    let _ = event_tx.send(BridgeEvent::Usage(kernel.usage_summary()));
}

fn send_sessions(kernel: &Kernel, event_tx: &UnboundedSender<BridgeEvent>) {
    let _ = event_tx.send(BridgeEvent::Sessions(kernel.list_sessions()));
}
