//! `LastCheckAt` —— 距离上次任意 source 的 `check_for_update` 完成的时间戳。
//!
//! ## 为什么需要
//!
//! Phase 5B 的 "window_show 顺手检查" 需要知道距离上次 check 多久。Q10 / Q10.1
//! 锁死阈值 30min，距离 = 现在 - LastCheckAt。任何 source（manual / scheduled /
//! startup / window_show）的 check 完成后都更新 LastCheckAt，让 dashboard 上
//! "上一次检查" 是真实的最近一次。
//!
//! ## 为什么 `AtomicI64`
//!
//! - 锁-free 读：window_show 在 UI 热路径，不能 `.lock()`
//! - epoch seconds 直接可读：日志 / debug 不需要 `Instant::elapsed` 转换
//! - 不需要跨进程持久化：UC 重启即默认 "刚检查过"，由 scheduler 在数十秒后
//!   接管。Q18.1 已锁死 scheduler 不让单点 panic 死掉，所以"重启后无 check
//!   兜底"不是有效场景
//! - 时钟跳变风险可忽略：检查是 fire-and-forget HTTP，最多一次额外请求
//!
//! ## 初始化时机
//!
//! 在 `run.rs` 装配时立即设为 `now`。这样首次 30min 内 `show_main_window`
//! 不会与 scheduler 首次 check 双发；scheduler 在 `wait_for_setup` 后做完
//! 首次 check 会再次更新到 now（无副作用）。

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 上次 `check_for_update` 完成的 epoch 时间戳（秒）。Tauri-managed state。
///
/// 任何 source（manual / scheduled / startup / window_show）的 check 完成时
/// 调用 [`LastCheckAt::record_now`]；`show_main_window` 读 [`LastCheckAt::seconds_since`]
/// 决定是否触发顺手检查。
#[derive(Debug)]
pub struct LastCheckAt(AtomicI64);

impl LastCheckAt {
    /// 用当前 epoch 秒初始化。在 `run.rs` 装配时调用一次。
    ///
    /// 选择 "init = now" 而非 0：避免 `silent_start=false` 下首次 `show_main_window`
    /// 与 scheduler 首次 check 双发同一个 `update_check_performed` 事件（间隔
    /// 仅几十秒，PostHog 漏斗分母会被噪音放大）。
    pub fn initialized_now() -> Self {
        Self(AtomicI64::new(current_epoch_secs()))
    }

    /// 写入当前 epoch 秒。可在任意线程并发调用（最后一次写胜出，无锁）。
    pub fn record_now(&self) {
        self.0.store(current_epoch_secs(), Ordering::Relaxed);
    }

    /// 显式写入指定 epoch 秒。仅供单元测试构造已知状态使用。
    #[cfg(test)]
    pub fn record_epoch_secs(&self, secs: i64) {
        self.0.store(secs, Ordering::Relaxed);
    }

    /// 当前 epoch 与上次记录的差（秒），系统时钟回拨 / 未来时间戳异常时
    /// 钳到 0 而非返回负数 —— 保证 caller 的"距离上次活动很短"判断不会
    /// 被时钟跳变骗过去（30min 阈值会因为负值意外通过）。
    pub fn seconds_since(&self) -> i64 {
        current_epoch_secs()
            .saturating_sub(self.0.load(Ordering::Relaxed))
            .max(0)
    }
}

impl Default for LastCheckAt {
    fn default() -> Self {
        Self::initialized_now()
    }
}

/// 当前 epoch 秒。`SystemTime` 早于 epoch 时返回 0（保守，让阈值判断走 "刚
/// 检查过" 路径而不是"很久没检查"）。
fn current_epoch_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn initialized_now_yields_recent_epoch() {
        let last = LastCheckAt::initialized_now();
        let elapsed = last.seconds_since();
        assert!(
            (0..=2).contains(&elapsed),
            "fresh LastCheckAt seconds_since should be ~0, got {}",
            elapsed
        );
    }

    #[test]
    fn record_now_advances_the_stored_epoch() {
        let last = LastCheckAt::initialized_now();
        last.record_epoch_secs(1_000_000_000); // 2001-09-09
        let before = last.seconds_since();
        assert!(
            before > 700_000_000,
            "seconds_since old fixed epoch should be large, got {}",
            before
        );

        last.record_now();
        let after = last.seconds_since();
        assert!(
            (0..=2).contains(&after),
            "after record_now, seconds_since should be ~0, got {}",
            after
        );
    }

    #[test]
    fn seconds_since_advances_with_wall_clock() {
        let last = LastCheckAt::initialized_now();
        let t0 = last.seconds_since();
        thread::sleep(Duration::from_millis(1100));
        let t1 = last.seconds_since();
        assert!(
            t1 > t0,
            "seconds_since should grow with wall clock; t0={} t1={}",
            t0,
            t1
        );
    }

    #[test]
    fn seconds_since_clamps_to_zero_on_clock_skew() {
        let last = LastCheckAt::initialized_now();
        // Pretend the last check is FAR in the future (clock jumped back).
        last.record_epoch_secs(i64::MAX / 2);
        // saturating_sub on a smaller current minus larger stored returns 0,
        // not a negative — keeps the "just checked" semantics.
        assert_eq!(last.seconds_since(), 0);
    }

    #[test]
    fn default_matches_initialized_now() {
        let last = LastCheckAt::default();
        assert!((0..=2).contains(&last.seconds_since()));
    }
}
