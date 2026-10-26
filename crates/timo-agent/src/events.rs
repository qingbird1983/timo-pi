//! 事件总线（architecture/02 §3 的落地；接缝 #2：内核状态外泄唯一通道）。

use std::sync::Arc;

use timo_ai::{AssistantStreamEvent, Message, Usage};
use tokio::sync::broadcast;

/// 内核事件全集（payload 均为不可变快照；订阅者不得改内核态）。
#[derive(Debug, Clone)]
pub enum KernelEvent {
    /// 一次 run 开始（恒为 run 首事件）。
    AgentStarted,
    /// 一次 run 结束（恒为 run 末事件）。
    AgentFinished {
        /// 本次 run 产生的全部新消息。
        messages: Arc<Vec<Message>>,
    },
    /// 一轮（一次模型请求 + 工具批）开始。
    TurnStarted,
    /// 一轮结束。
    TurnFinished {
        /// 本轮 assistant 终态消息。
        message: Arc<Message>,
        /// 本轮工具结果（可能为空）。
        tool_results: Arc<Vec<Message>>,
    },
    /// 消息持久化（对齐 pi message_end；会话挂接点）。
    MessageAdded {
        /// 消息快照。
        message: Arc<Message>,
    },
    /// 流式粒度事件（Start/Delta/BlockEnd…，对齐 pi message_update）。
    StreamEvent {
        /// 协议流事件快照。
        event: Arc<AssistantStreamEvent>,
    },
    /// 工具调用请求（Phase 1 仅通告；veto 接口 Phase 2）。
    ToolCallRequested {
        /// 工具调用内容快照（含工具名、参数与解析错误）。
        call: Arc<timo_ai::Content>,
    },
    /// 工具执行开始。
    ToolCallStarted {
        /// 调用 id。
        tool_call_id: String,
        /// 工具名。
        tool_name: String,
        /// 参数快照（参数 JSON 非法时保留解析器给出的原值）。
        args: Arc<serde_json::Value>,
    },
    /// 工具部分结果（on_update 通道；settle 后不再产出）。
    ToolCallUpdate {
        /// 调用 id。
        tool_call_id: String,
        /// 工具名。
        tool_name: String,
        /// 部分结果快照。
        partial: Arc<timo_tools::ToolOutput>,
    },
    /// 工具执行结束。
    ToolCallFinished {
        /// 调用 id。
        tool_call_id: String,
        /// 工具名。
        tool_name: String,
        /// 结果消息快照。
        result: Arc<Message>,
        /// 时长（毫秒；未执行为 None）。
        duration_ms: Option<u64>,
    },
    /// 供应商请求元数据。
    ProviderRequest {
        /// 供应商 id。
        provider: String,
        /// 模型 id。
        model: String,
    },
    /// 供应商响应元数据。
    ProviderResponse {
        /// 供应商 id。
        provider: String,
        /// 模型 id。
        model: String,
        /// 用量。
        usage: Usage,
    },
    /// 错误通告（循环级；协议内错误经 Error 终态消息表达）。
    Error {
        /// 上下文描述。
        context: String,
    },
}

/// 广播事件总线（容量 1024；滞后订阅者收到 Lagged 不致命）。
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<KernelEvent>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    /// 新总线。
    #[must_use]
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { tx }
    }

    /// 订阅（接缝 #2：新增订阅者无需改循环）。
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<KernelEvent> {
        self.tx.subscribe()
    }

    /// 发布（无订阅者时静默）。
    pub fn emit(&self, event: KernelEvent) {
        let _ = self.tx.send(event);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscribers_receive_snapshots() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        bus.emit(KernelEvent::Error {
            context: "x".into(),
        });
        match rx.recv().await {
            Ok(KernelEvent::Error { context }) => assert_eq!(context, "x"),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
