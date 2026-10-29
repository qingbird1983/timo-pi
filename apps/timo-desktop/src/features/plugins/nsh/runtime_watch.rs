//! `__ping/__pong` 看门狗状态机（设计 05 §4）。
//!
//! 每 2s 发一次 ping，等 1s；连续两次没有 pong 就判死循环/无响应。
//! 这里只做时间与计数，发 ping 与 stop 由运行期执行。

use std::time::{Duration, Instant};

/// ping 间隔。
pub(crate) const PING_EVERY: Duration = Duration::from_secs(2);
/// 单次 pong 等待。
pub(crate) const PONG_WAIT: Duration = Duration::from_secs(1);
/// 连续无 pong 上限。
pub(crate) const MISS_LIMIT: u8 = 2;

/// 一次 tick 的结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Beat {
    /// 无需动作。
    Idle,
    /// 发一次 ping（序号用于诊断）。
    Ping(u64),
    /// 已发 ping，仍在等待。
    Waiting,
    /// 连续无响应，应停止实例。
    Stop,
}

/// 一个实例的看门狗。
pub(crate) struct Watchdog {
    next_ping: Instant,
    sent_at: Option<Instant>,
    baseline: u64,
    misses: u8,
    seq: u64,
}

impl Watchdog {
    /// 健康门通过后开始计时。
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            next_ping: now + PING_EVERY,
            sent_at: None,
            baseline: 0,
            misses: 0,
            seq: 0,
        }
    }

    /// 推进一次；`pongs` 是宿主收到的累计 pong 数。
    pub(crate) fn tick(&mut self, now: Instant, pongs: u64) -> Beat {
        if let Some(sent) = self.sent_at {
            if pongs > self.baseline {
                self.sent_at = None;
                self.misses = 0;
                self.next_ping = now + PING_EVERY;
                return Beat::Idle;
            }
            if now < sent + PONG_WAIT {
                return Beat::Waiting;
            }
            self.sent_at = None;
            self.misses = self.misses.saturating_add(1);
            if self.misses >= MISS_LIMIT {
                return Beat::Stop;
            }
            self.next_ping = now;
        }
        if now >= self.next_ping {
            self.seq += 1;
            self.sent_at = Some(now);
            self.baseline = pongs;
            self.next_ping = now + PING_EVERY;
            return Beat::Ping(self.seq);
        }
        Beat::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_missed_pongs_stop_the_instance() {
        let start = Instant::now();
        let mut watchdog = Watchdog::new(start);
        assert_eq!(watchdog.tick(start, 0), Beat::Idle);
        assert_eq!(watchdog.tick(start + PING_EVERY, 0), Beat::Ping(1));
        assert_eq!(watchdog.tick(start + PING_EVERY, 0), Beat::Waiting);
        assert_eq!(
            watchdog.tick(start + PING_EVERY + PONG_WAIT, 0),
            Beat::Ping(2)
        );
        assert_eq!(
            watchdog.tick(start + PING_EVERY + PONG_WAIT + PONG_WAIT, 0),
            Beat::Stop
        );
    }

    #[test]
    fn a_pong_resets_the_deadline() {
        let start = Instant::now();
        let mut watchdog = Watchdog::new(start);
        assert_eq!(watchdog.tick(start + PING_EVERY, 0), Beat::Ping(1));
        let answered = start + PING_EVERY + Duration::from_millis(200);
        assert_eq!(watchdog.tick(answered, 1), Beat::Idle);
        assert_eq!(watchdog.tick(answered + PING_EVERY, 1), Beat::Ping(2));
    }
}
