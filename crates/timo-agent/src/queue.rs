//! steering / follow-up 消息队列（pi PendingMessageQueue 同构；默认 one-at-a-time）。

use std::collections::VecDeque;

use timo_ai::Message;

/// 队列模式：`all` 全取 / `one-at-a-time` 取一（pi 同名语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueMode {
    /// drain 取全部。
    All,
    /// drain 取一条。
    OneAtATime,
}

/// 待注入消息队列。
#[derive(Debug)]
pub struct PendingMessageQueue {
    inner: std::sync::Mutex<VecDeque<Message>>,
    mode: QueueMode,
}

impl PendingMessageQueue {
    /// 空队列（默认 one-at-a-time）。
    #[must_use]
    pub fn new() -> Self {
        Self::with_mode(QueueMode::OneAtATime)
    }

    /// 指定模式。
    #[must_use]
    pub fn with_mode(mode: QueueMode) -> Self {
        Self {
            inner: std::sync::Mutex::new(VecDeque::new()),
            mode,
        }
    }

    /// 入队（steer/follow_up 入口）。
    pub fn push(&self, message: Message) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(message);
    }

    /// peek 语义（模式决定返回量）；drain 移除 peek 返回的部分。
    pub fn drain(&self) -> Vec<Message> {
        let mut q = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match self.mode {
            QueueMode::All => q.drain(..).collect(),
            QueueMode::OneAtATime => q.pop_front().into_iter().collect(),
        }
    }

    /// 是否有排队消息。
    #[must_use]
    pub fn has_queued(&self) -> bool {
        !self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    }

    /// 清空。
    pub fn clear(&self) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

impl Default for PendingMessageQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use timo_ai::user_text;

    #[test]
    fn one_at_a_time_drain() {
        let q = PendingMessageQueue::new();
        q.push(user_text("a", 1));
        q.push(user_text("b", 2));
        assert!(q.has_queued());
        let got = q.drain();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].timestamp(), 1);
        assert!(q.has_queued());
        q.clear();
        assert!(!q.has_queued());
    }

    #[test]
    fn all_mode_drain() {
        let q = PendingMessageQueue::with_mode(QueueMode::All);
        q.push(user_text("a", 1));
        q.push(user_text("b", 2));
        assert_eq!(q.drain().len(), 2);
        assert!(!q.has_queued());
    }
}
