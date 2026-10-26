//! 流事件与流句柄（设计 01 §3；接缝 #6 富枚举）。

use std::time::Instant;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::errors::AiError;
use crate::types::{AssistantMessage, StopReason};

/// 协议适配器产出的流事件（每次事件携带当前最新 partial 快照，见设计 00 §5 D3）。
#[derive(Debug, Clone)]
pub enum AssistantStreamEvent {
    /// 流开始（恒为首事件）。
    Start {
        /// 当前 partial 快照。
        partial: AssistantMessage,
    },
    /// 新内容块开始（text/thinking/toolcall 由 partial 判别）。
    BlockStart {
        /// partial.content 中的下标。
        index: usize,
        /// 当前 partial 快照。
        partial: AssistantMessage,
    },
    /// 增量（文本/思考/工具参数片段）。
    Delta {
        /// partial.content 中的下标。
        index: usize,
        /// 增量原文。
        delta: String,
        /// 当前 partial 快照。
        partial: AssistantMessage,
    },
    /// 内容块结束（工具调用此时已有完整参数）。
    BlockEnd {
        /// partial.content 中的下标。
        index: usize,
        /// 当前 partial 快照。
        partial: AssistantMessage,
    },
    /// 正常终态（reason ∈ Stop/Length/ToolUse/Deferred）。
    Done {
        /// 终止原因。
        reason: StopReason,
        /// 终态消息。
        message: AssistantMessage,
    },
    /// 异常终态（stop_reason ∈ Error/Aborted + error_message）。
    Error {
        /// 终态消息（含错误信息）。
        message: AssistantMessage,
    },
}

/// 流选项（每次请求一份；api_key 由调用方按认证优先级解析后注入）。
#[derive(Debug, Clone)]
pub struct StreamOptions {
    /// 已解析的凭证（None = 走模型目录默认，通常在 Resolver 已定）。
    pub api_key: Option<String>,
    /// 取消令牌（接缝 #7）。
    pub cancel: CancellationToken,
    /// 覆盖 max_tokens。
    pub max_tokens: Option<u64>,
    /// 覆盖 temperature（仅 thinking 关闭时生效，Anthropic 规则）。
    pub temperature: Option<f64>,
    /// 传输层重试次数（默认 0；仅未开始的初始请求）。
    pub max_retries: u32,
    /// 服务端要求退避的上限（毫秒；默认 60_000，0 = 不设限）。
    pub max_retry_delay_ms: u64,
    /// 覆盖 baseUrl。
    pub base_url: Option<String>,
    /// 额外请求头（追加；同名列覆盖默认）。
    pub headers: Vec<(String, String)>,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self {
            api_key: None,
            cancel: CancellationToken::new(),
            max_tokens: None,
            temperature: None,
            max_retries: 0,
            max_retry_delay_ms: 60_000,
            base_url: None,
            headers: vec![],
        }
    }
}

/// assistant 消息流：事件通道 + 终态提取。
pub struct AssistantMessageStream {
    rx: mpsc::Receiver<AssistantStreamEvent>,
    started: Option<Instant>,
}

impl AssistantMessageStream {
    /// 由事件接收端构造（适配器内部使用）。
    #[must_use]
    pub fn new(rx: mpsc::Receiver<AssistantStreamEvent>) -> Self {
        Self {
            rx,
            started: Some(Instant::now()),
        }
    }

    /// 取下一个事件；通道关闭（None）视为异常流提前结束。
    pub async fn next_event(&mut self) -> Option<AssistantStreamEvent> {
        self.rx.recv().await
    }

    /// 消费至终态并返回最终消息（对齐 pi event-stream `.result()`）。
    ///
    /// 流未发出终事件即关闭时，合成 `Error` 终态消息（不 panic）。
    pub async fn result(mut self) -> AssistantMessage {
        let mut final_msg: Option<AssistantMessage> = None;
        while let Some(ev) = self.next_event().await {
            match ev {
                AssistantStreamEvent::Done { message, .. }
                | AssistantStreamEvent::Error { message } => final_msg = Some(message),
                _ => {}
            }
        }
        match final_msg {
            Some(mut m) => {
                m.duration_ms = Some(self.elapsed_ms());
                m
            }
            None => AssistantMessage {
                stop_reason: StopReason::Error,
                error_message: Some("stream ended without terminal event".to_string()),
                duration_ms: Some(self.elapsed_ms()),
                ..AssistantMessage::pending("unknown", "unknown", "unknown", 0)
            },
        }
    }

    fn elapsed_ms(&self) -> u64 {
        self.started
            .map(|s| s.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }
}

/// 适配器内部：把 `AiError` 编码为流终事件所需的终态消息。
#[must_use]
pub fn error_message(base: &AssistantMessage, err: &AiError, cancelled: bool) -> AssistantMessage {
    AssistantMessage {
        stop_reason: if cancelled {
            StopReason::Aborted
        } else {
            StopReason::Error
        },
        error_message: Some(err.to_string()),
        ..base.clone()
    }
}
