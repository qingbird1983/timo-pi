//! 内核桥接：专用 tokio 线程独占 `Kernel`，UI 只收发值类型（设计 01 §4~§5）。
//!
//! UI 线程不做阻塞 IO；run 期间只接受取消，会话切换在运行中会被拒绝。

use timo_kernel::{Kernel, KernelEvent, KernelOptions, Message, SessionInfo};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

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
    /// 内核事件快照。
    Kernel(KernelEvent),
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
pub fn spawn(options: KernelOptions) -> Bridge {
    let (command_tx, command_rx) = unbounded_channel();
    let (event_tx, event_rx) = unbounded_channel();
    let thread_events = event_tx.clone();
    let spawned = std::thread::Builder::new()
        .name("timo-kernel".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run(options, command_rx, thread_events.clone())),
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

async fn run(
    options: KernelOptions,
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

    let mut kernel_events = kernel.events();
    let forward_tx = event_tx.clone();
    tokio::spawn(async move {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match kernel_events.recv().await {
                Ok(event) => {
                    if forward_tx.send(BridgeEvent::Kernel(event)).is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    if forward_tx.send(BridgeEvent::Lagged(skipped)).is_err() {
                        break;
                    }
                }
                Err(RecvError::Closed) => break,
            }
        }
    });

    let sessions = kernel.list_sessions();
    let _ = event_tx.send(BridgeEvent::Ready {
        model: kernel.model_label(),
        sessions,
    });

    while let Some(command) = commands.recv().await {
        match command {
            BridgeCommand::RefreshSessions => send_sessions(&kernel, &event_tx),
            BridgeCommand::NewSession => {
                kernel.new_session();
                let _ = event_tx.send(BridgeEvent::Opened {
                    id: None,
                    history: vec![],
                });
            }
            BridgeCommand::OpenSession(id) => match kernel.resume(&id).await {
                Ok(()) => {
                    let history = kernel.history().unwrap_or_default().to_vec();
                    let _ = event_tx.send(BridgeEvent::Opened {
                        id: Some(id),
                        history,
                    });
                }
                Err(error) => {
                    let _ = event_tx.send(BridgeEvent::OpenFailed(error.to_string()));
                }
            },
            BridgeCommand::DeleteSession(id) => match kernel.delete_session(&id) {
                Ok(()) => {
                    let _ = event_tx.send(BridgeEvent::Deleted(id));
                    send_sessions(&kernel, &event_tx);
                }
                Err(error) => {
                    let _ = event_tx.send(BridgeEvent::DeleteFailed(error.to_string()));
                }
            },
            BridgeCommand::Run(prompt) => {
                run_once(&mut kernel, prompt, &mut commands, &event_tx).await;
            }
            BridgeCommand::Cancel => {}
        }
    }
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
}

fn send_sessions(kernel: &Kernel, event_tx: &UnboundedSender<BridgeEvent>) {
    let _ = event_tx.send(BridgeEvent::Sessions(kernel.list_sessions()));
}
