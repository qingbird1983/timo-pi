//! 内核桥接：专用 tokio 线程独占 `Kernel`，UI 只收发值类型（设计 01 §4~§5）。
//!
//! UI 线程不做阻塞 IO；run 期间只接受取消，会话切换在运行中会被拒绝。
//! 工具审阅经 [`crate::bridge::ReviewBroker`] 转成事件，等待 UI 的 oneshot 决定。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use timo_config::KEYCHAIN_SERVICE;
use timo_kernel::trust::{ProjectTrustPrompt, TrustRequest};
use timo_kernel::{
    Kernel, KernelEvent, KernelOptions, Message, ReviewDecision, ReviewKind, SessionInfo,
    SlashCommand, ToolReview, ToolReviewRequest, TrustChoice, UsageSummary,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;

use crate::settings::ReviewPrefs;

#[path = "bridge_providers.rs"]
mod providers;
use providers::{models_path, provider_infos, save_provider_key};

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

/// 装配完成后的项目信任结论（只读，供状态行与诊断）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustReport {
    /// 是否加载项目资源。
    pub trusted: bool,
    /// 决定来源的短名（`TrustSource::label`）。
    pub source: String,
    /// 不信任时被跳过的资源数。
    pub skipped: usize,
    /// 存储读写问题（若有）。
    pub notice: Option<String>,
}

/// 内核发给 UI 的事件。
#[derive(Debug)]
pub enum BridgeEvent {
    /// 内核装配完成。
    Ready {
        /// 当前 provider/model。
        model: String,
        /// 会话列表。
        sessions: Vec<SessionInfo>,
    },
    /// 会话列表刷新。
    Sessions(Vec<SessionInfo>),
    /// 会话文件已删除。
    Deleted(String),
    /// 会话已切换（`id = None` 表示草稿）。
    Opened {
        /// 会话 id。
        id: Option<String>,
        /// 投影历史。
        history: Vec<Message>,
    },
    /// 一次 run 开始。
    Running,
    /// 一次 run 结束。
    Finished,
    /// 命令清单（内建 + 技能 + 扩展）。
    Commands(Vec<SlashCommand>),
    /// 累计用量（会话级）。
    Usage(UsageSummary),
    /// 内核事件快照。
    Kernel(KernelEvent),
    /// 工具执行前审阅请求；UI 决定后经 `respond` 回传。
    Review {
        /// 待审副作用。
        request: ToolReviewRequest,
        /// 决定通道；UI 关闭窗口即 drop → 拒绝。
        respond: oneshot::Sender<ReviewDecision>,
    },
    /// 项目信任询问；UI 决定后经 `respond` 回传（`None` = 界面关闭/取消）。
    Trust {
        /// 待决的项目资源与上下文。
        request: TrustRequest,
        /// 决定通道；窗口关闭即 drop → 按"不信任（本次）"。
        respond: oneshot::Sender<Option<TrustChoice>>,
    },
    /// 项目信任结论（装配完成、`Ready` 之前发出；设计 12 §3.5）。
    TrustReport(TrustReport),
    /// 供应商列表。
    Providers(Vec<ProviderInfo>),
    /// 某供应商密钥已保存。
    ProviderSaved(String),
    /// 某供应商密钥已移除。
    ProviderRemoved(String),
    /// 钥匙串/配置操作失败。
    ProviderError(String),
    /// 打开会话失败。
    OpenFailed(String),
    /// 删除会话失败。
    DeleteFailed(String),
    /// run 失败（循环级错误）。
    RunFailed(String),
    /// 运行中收到会话切换命令。
    Busy,
    /// 事件通道落后。
    Lagged(u64),
    /// 启动失败（凭证/配置），内核不可用。
    Fatal(String),
}

/// 桥接两端：命令发送端 + 事件接收端。
pub struct Bridge {
    /// 发送命令。
    pub commands: UnboundedSender<BridgeCommand>,
    /// 接收事件。
    pub events: UnboundedReceiver<BridgeEvent>,
}

/// 启动专用内核线程；装配失败也会发 [`BridgeEvent::Fatal`]。
pub fn spawn(options: KernelOptions, review: ReviewPrefs) -> Bridge {
    let (command_tx, command_rx) = unbounded_channel();
    let (event_tx, event_rx) = unbounded_channel();
    let prefs = Arc::new(Mutex::new(review));
    let data_dir = options.data_dir.clone();

    let mut options = options;
    options.tool_review = Some(Arc::new(ReviewBroker {
        events: event_tx.clone(),
        prefs: prefs.clone(),
    }));
    options.trust_prompt = Some(Arc::new(TrustBroker {
        events: event_tx.clone(),
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

/// UI 侧信任询问器：把请求交给界面并等待 `oneshot` 选择。
struct TrustBroker {
    events: UnboundedSender<BridgeEvent>,
}

impl ProjectTrustPrompt for TrustBroker {
    fn ask<'a>(
        &'a self,
        request: TrustRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<TrustChoice>> + Send + 'a>> {
        let (respond, answer) = oneshot::channel();
        let sent = self.events.send(BridgeEvent::Trust { request, respond });
        Box::pin(async move {
            if sent.is_err() {
                return None;
            }
            // 通道断开 = 窗口已关闭；`None` 由内核按"不信任（本次）"处理。
            answer.await.unwrap_or(None)
        })
    }
}

async fn run(
    options: KernelOptions,
    data_dir: Option<PathBuf>,
    prefs: Arc<Mutex<ReviewPrefs>>,
    mut commands: UnboundedReceiver<BridgeCommand>,
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

    let trust = kernel.project_trust();
    let _ = event_tx.send(BridgeEvent::TrustReport(TrustReport {
        trusted: trust.trusted,
        source: trust.source.label().to_string(),
        skipped: trust.skipped.len(),
        notice: trust.notice.clone(),
    }));

    let sessions = kernel.list_sessions();
    let _ = event_tx.send(BridgeEvent::Ready {
        model: kernel.model_label(),
        sessions,
    });
    // 只建提示与技能（不触发 MCP 连接），让面板在首轮前就有命令可列。
    kernel.reload_resources();
    let _ = event_tx.send(BridgeEvent::Commands(kernel.slash_commands()));
    let _ = event_tx.send(BridgeEvent::Usage(kernel.usage_summary()));

    while let Some(command) = commands.recv().await {
        match command {
            BridgeCommand::RefreshSessions => send_sessions(&kernel, &event_tx),
            BridgeCommand::NewSession => {
                kernel.new_session();
                let _ = event_tx.send(BridgeEvent::Opened {
                    id: None,
                    history: vec![],
                });
                let _ = event_tx.send(BridgeEvent::Usage(kernel.usage_summary()));
            }
            BridgeCommand::OpenSession(id) => match kernel.resume(&id).await {
                Ok(()) => {
                    let history = kernel.history().unwrap_or_default().to_vec();
                    let _ = event_tx.send(BridgeEvent::Opened {
                        id: Some(id),
                        history,
                    });
                    let _ = event_tx.send(BridgeEvent::Usage(kernel.usage_summary()));
                }
                Err(error) => {
                    let _ = event_tx.send(BridgeEvent::OpenFailed(error.to_string()));
                }
            },
            BridgeCommand::DeleteSession(id) => match kernel.delete_session(&id) {
                Ok(()) => {
                    let _ = event_tx.send(BridgeEvent::Deleted(id));
                    send_sessions(&kernel, &event_tx);
                    let _ = event_tx.send(BridgeEvent::Usage(kernel.usage_summary()));
                }
                Err(error) => {
                    let _ = event_tx.send(BridgeEvent::DeleteFailed(error.to_string()));
                }
            },
            BridgeCommand::Run(prompt) => {
                run_once(&mut kernel, prompt, &mut commands, &event_tx).await;
            }
            BridgeCommand::Cancel => {}
            BridgeCommand::SetReviewPrefs { files, commands } => {
                if let Ok(mut prefs) = prefs.lock() {
                    prefs.files = files;
                    prefs.commands = commands;
                }
            }
            BridgeCommand::ListProviders => {
                let model = kernel.model_label();
                let list = provider_infos(data_dir.as_deref(), &model);
                let _ = event_tx.send(BridgeEvent::Providers(list));
            }
            BridgeCommand::SaveProviderKey { provider, secret } => {
                let path = models_path(data_dir.as_deref());
                match save_provider_key(path, &provider, &secret).await {
                    Ok(()) => {
                        let _ = event_tx.send(BridgeEvent::ProviderSaved(provider));
                        let model = kernel.model_label();
                        let list = provider_infos(data_dir.as_deref(), &model);
                        let _ = event_tx.send(BridgeEvent::Providers(list));
                    }
                    Err(error) => {
                        let _ = event_tx.send(BridgeEvent::ProviderError(error));
                    }
                }
            }
            BridgeCommand::RemoveProviderKey { provider } => {
                let provider_name = provider.clone();
                let removed = tokio::task::spawn_blocking(move || {
                    timo_config::keychain_delete(KEYCHAIN_SERVICE, &provider_name)
                })
                .await;
                match removed {
                    Ok(Ok(())) => {
                        let _ = event_tx.send(BridgeEvent::ProviderRemoved(provider));
                        let model = kernel.model_label();
                        let list = provider_infos(data_dir.as_deref(), &model);
                        let _ = event_tx.send(BridgeEvent::Providers(list));
                    }
                    Ok(Err(error)) => {
                        let _ = event_tx.send(BridgeEvent::ProviderError(error.to_string()));
                    }
                    Err(error) => {
                        let _ = event_tx.send(BridgeEvent::ProviderError(error.to_string()));
                    }
                }
            }
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

#[cfg(test)]
#[path = "bridge_tests.rs"]
mod tests;
