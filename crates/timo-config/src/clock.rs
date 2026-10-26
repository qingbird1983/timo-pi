//! 可注入时钟（standards/05 §3：时间一律注入，保证可复现）。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 时钟抽象：生产实现走系统时间，测试注入固定值。
pub trait Clock: Send + Sync {
    /// 当前 unix 毫秒。
    fn now_ms(&self) -> i64;
}

/// 系统时钟（生产默认）。
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}

/// 固定时钟（测试用）：从给定毫秒起步，`advance_ms` 手动推进。
#[derive(Debug)]
pub struct FixedClock {
    ms: AtomicU64,
}

impl FixedClock {
    /// 从指定 unix 毫秒创建。
    #[must_use]
    pub fn new(ms: i64) -> Self {
        Self {
            ms: AtomicU64::new(ms.max(0) as u64),
        }
    }

    /// 手动推进（仅测试）。
    pub fn advance_ms(&self, delta: i64) {
        self.ms.fetch_add(delta.max(0) as u64, Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.ms.load(Ordering::SeqCst) as i64
    }
}
