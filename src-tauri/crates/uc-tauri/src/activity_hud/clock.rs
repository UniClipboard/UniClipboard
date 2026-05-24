//! 单调时钟抽象。
//!
//! 状态机内部所有时间戳都用 `u64` 毫秒表达,生产代码以
//! [`SystemClock`] 锚定 `Instant::now()`,单测以 [`ManualClock`] 控制
//! 推进。这样避开 `std::time::Instant` 无 public 构造器的限制,让
//! `state.rs` 里"速度滑窗 / 终态保留 / auto-hide 防抖"这些和时间强
//! 耦合的分支可以被纯单元测试覆盖。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// 返回自任意单调起点的毫秒数。实现必须保证单调不减。
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// 锚定 `Instant::now()` 的真实时钟。起点由 `new()` 决定,起点本身的
/// 绝对值不暴露,调用方只比较相对差值。
pub struct SystemClock {
    start: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        // duration_since 从单调时钟看不会回退;`as u64` 在 ~5 亿年后才溢出,
        // 进程生命周期内不会到。
        Instant::now().duration_since(self.start).as_millis() as u64
    }
}

/// 单元测试用的可手动推进时钟。线程安全:多个测试线程并发推进时
/// 仍是单调累加。
pub struct ManualClock {
    current_ms: AtomicU64,
}

impl ManualClock {
    pub fn new() -> Self {
        Self {
            current_ms: AtomicU64::new(0),
        }
    }

    /// 推进 `delta_ms` 毫秒。多次调用累加。
    pub fn advance(&self, delta_ms: u64) {
        self.current_ms.fetch_add(delta_ms, Ordering::Relaxed);
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.current_ms.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn system_clock_is_monotonic() {
        let clock = SystemClock::new();
        let t0 = clock.now_ms();
        let t1 = clock.now_ms();
        assert!(t1 >= t0);
    }

    #[test]
    fn manual_clock_starts_at_zero_and_advances() {
        let clock = ManualClock::new();
        assert_eq!(clock.now_ms(), 0);
        clock.advance(100);
        assert_eq!(clock.now_ms(), 100);
        clock.advance(50);
        assert_eq!(clock.now_ms(), 150);
    }

    #[test]
    fn manual_clock_is_send_sync() {
        // 编译期断言,trait 调用方一律持 Arc<dyn Clock>。
        fn assert_send_sync<T: Send + Sync>(_: &T) {}
        let clock: Arc<dyn Clock> = Arc::new(ManualClock::new());
        assert_send_sync(&clock);
    }
}
