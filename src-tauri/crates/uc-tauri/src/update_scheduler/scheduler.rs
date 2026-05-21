//! 后台周期更新检查 scheduler 主循环。
//!
//! 本模块只负责"什么时候 check / 怎么 backoff / 何时让位关停"，
//! 不直接发送系统通知（Phase 4A 再加），也不自动下载（Phase 4B 再加）。
//!
//! 时序：
//! - 启动后先 poll `SetupStatus.has_completed`，setup 未完成时每 30s 重试
//! - 主循环：
//!   1. load settings；`auto_check_update == false` 当作 idle，不 emit
//!      telemetry，按成功 cadence 继续轮询（让用户开关切换无 30min 惩罚）
//!   2. true 时调 `do_check_for_update` 内部入口 + emit
//!      `update_check_performed { source: scheduled, ... }`
//!   3. 成功 6h ± 15min jitter；失败 30min（Q9：固定，不是指数 backoff）
//! - 任一 sleep 内被 cancellation token 打断 → 立即退出

use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uc_core::ports::{SettingsPort, SetupStatusPort};
use uc_observability::analytics::{AnalyticsPort, Event, UpdateCheckOutcome, UpdateCheckSource};

use super::last_notified::LastNotifiedUpdateStore;
use crate::commands::updater::{
    classify_check_failure, detect_install_kind, do_check_for_update, install_kind_for_telemetry,
    PendingUpdate,
};

/// Setup 未完成时的轮询间隔（Q16.1：30s，不订阅事件）。
const SETUP_POLL_INTERVAL: Duration = Duration::from_secs(30);
/// 成功 / idle 后下一轮 check 的基准间隔（Q9：6h）。
pub(crate) const SUCCESS_BASE_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// 成功 / idle 后的 jitter 上限（Q9：±15min，避免所有客户端同步轰炸 release CDN）。
pub(crate) const SUCCESS_JITTER: Duration = Duration::from_secs(15 * 60);
/// 失败重试间隔（Q9：固定 30min，不是指数 backoff）。
pub(crate) const FAILURE_RETRY_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Scheduler 启动所需的全部依赖。
///
/// 持有 strong refs；scheduler task 生命周期由 `CancellationToken` 与
/// `task_registry.shutdown()` 联合管理（见 `run.rs:589` ExitRequested
/// 路径，Phase 3C 接入）。
pub struct SchedulerDeps {
    pub app_handle: AppHandle,
    pub settings_port: Arc<dyn SettingsPort>,
    pub setup_status_port: Arc<dyn SetupStatusPort>,
    pub analytics: Arc<dyn AnalyticsPort>,
    /// 已通知版本去重存储（Phase 4B 用，Phase 3B 仅持有 ref；预留
    /// 是为了避免下个 commit 改 `SchedulerDeps` 形态影响 `run.rs` 装配）。
    pub last_notified: Arc<Mutex<LastNotifiedUpdateStore>>,
}

/// 启动 scheduler 主循环。调用方 `run.rs:480` 内 `tauri::async_runtime::spawn`
/// 它，把 `task_registry.child_token()` 传进来。
pub async fn run(deps: SchedulerDeps, token: CancellationToken) {
    info!(target: "update_scheduler", "starting");
    if !wait_for_setup(&deps.setup_status_port, &token).await {
        info!(target: "update_scheduler", "cancelled before setup completed");
        return;
    }
    info!(target: "update_scheduler", "setup completed; entering main loop");
    main_loop(&deps, token).await;
    info!(target: "update_scheduler", "exited main loop");
}

/// 主循环的迭代结果。决定下一次 sleep 的时长。
///
/// `auto_check_update == false` 的 idle 分支也归 `Success`：
/// 用 6h cadence 周期性 reload settings，用户把开关打开后无 30min 惩罚。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IterationOutcome {
    Success,
    Failure,
}

async fn wait_for_setup(port: &Arc<dyn SetupStatusPort>, token: &CancellationToken) -> bool {
    loop {
        match port.get_status().await {
            Ok(status) if status.has_completed => return true,
            Ok(_) => debug!(target: "update_scheduler", "setup not yet completed"),
            Err(err) => warn!(
                target: "update_scheduler",
                error = %err,
                "failed to read setup status; retrying"
            ),
        }
        tokio::select! {
            _ = token.cancelled() => return false,
            _ = tokio::time::sleep(SETUP_POLL_INTERVAL) => {}
        }
    }
}

async fn main_loop(deps: &SchedulerDeps, token: CancellationToken) {
    loop {
        let outcome = run_one_iteration(deps).await;
        let sleep_dur = next_sleep_after(outcome);
        debug!(
            target: "update_scheduler",
            outcome = ?outcome,
            sleep_secs = sleep_dur.as_secs(),
            "iteration done; scheduling next"
        );
        tokio::select! {
            _ = token.cancelled() => return,
            _ = tokio::time::sleep(sleep_dur) => {}
        }
    }
}

async fn run_one_iteration(deps: &SchedulerDeps) -> IterationOutcome {
    let settings = match deps.settings_port.load().await {
        Ok(s) => s,
        Err(err) => {
            warn!(
                target: "update_scheduler",
                error = %err,
                "failed to load settings; backing off"
            );
            return IterationOutcome::Failure;
        }
    };

    if !settings.general.auto_check_update {
        debug!(target: "update_scheduler", "auto_check_update disabled; idle");
        // Q16.3: 关闭分支不 emit 任何 telemetry，避免污染漏斗分母
        return IterationOutcome::Success;
    }

    let channel = settings.general.update_channel.clone();
    let app = deps.app_handle.clone();
    let pending = app.state::<PendingUpdate>();
    let result = do_check_for_update(&app, channel, pending.inner()).await;

    let install_kind = install_kind_for_telemetry(detect_install_kind());
    let (outcome, failure_kind, iter_outcome) = match &result {
        Ok(Some(_)) => (
            UpdateCheckOutcome::Available,
            None,
            IterationOutcome::Success,
        ),
        Ok(None) => (
            UpdateCheckOutcome::UpToDate,
            None,
            IterationOutcome::Success,
        ),
        Err(err) => (
            UpdateCheckOutcome::Failed,
            Some(classify_check_failure(err)),
            IterationOutcome::Failure,
        ),
    };

    deps.analytics.capture(Event::UpdateCheckPerformed {
        source: UpdateCheckSource::Scheduled,
        outcome,
        failure_kind,
        install_kind,
    });

    iter_outcome
}

/// 计算给定 outcome 后的下一次 sleep 时长（纯函数，方便单测）。
pub(crate) fn next_sleep_after(outcome: IterationOutcome) -> Duration {
    match outcome {
        IterationOutcome::Failure => FAILURE_RETRY_INTERVAL,
        IterationOutcome::Success => jittered_success_interval(),
    }
}

/// 6h base + 均匀采样自 [-15min, +15min] 的 offset。返回 saturating
/// 在 [0, base + jitter] 区间内的 Duration（base 远大于 jitter，下界
/// 实际不会触发）。
fn jittered_success_interval() -> Duration {
    let jitter_secs = SUCCESS_JITTER.as_secs() as i64;
    let offset_secs: i64 = rand::rng().random_range(-jitter_secs..=jitter_secs);
    let base_secs = SUCCESS_BASE_INTERVAL.as_secs() as i64;
    let total = (base_secs + offset_secs).max(0) as u64;
    Duration::from_secs(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::RwLock;
    use uc_core::setup::SetupStatus;

    /// In-memory `SetupStatusPort` for scheduler unit tests. Flips to
    /// completed after `flip_after_n_reads` `get_status()` calls.
    struct FakeSetupStatus {
        status: RwLock<SetupStatus>,
        reads: AtomicUsize,
        flip_after_n_reads: usize,
    }

    impl FakeSetupStatus {
        fn always_completed() -> Arc<Self> {
            Arc::new(Self {
                status: RwLock::new(SetupStatus {
                    has_completed: true,
                    ..SetupStatus::default()
                }),
                reads: AtomicUsize::new(0),
                flip_after_n_reads: 0,
            })
        }

        fn never_completed() -> Arc<Self> {
            Arc::new(Self {
                status: RwLock::new(SetupStatus::default()),
                reads: AtomicUsize::new(0),
                flip_after_n_reads: usize::MAX,
            })
        }
    }

    #[async_trait]
    impl SetupStatusPort for FakeSetupStatus {
        async fn get_status(&self) -> anyhow::Result<SetupStatus> {
            let n = self.reads.fetch_add(1, Ordering::SeqCst);
            if n + 1 >= self.flip_after_n_reads {
                self.status.write().await.has_completed = true;
            }
            Ok(self.status.read().await.clone())
        }

        async fn set_status(&self, status: &SetupStatus) -> anyhow::Result<()> {
            *self.status.write().await = status.clone();
            Ok(())
        }
    }

    // ---- Pure backoff math --------------------------------------------------

    #[test]
    fn next_sleep_after_failure_is_fixed_30min() {
        assert_eq!(
            next_sleep_after(IterationOutcome::Failure),
            FAILURE_RETRY_INTERVAL
        );
        assert_eq!(FAILURE_RETRY_INTERVAL, Duration::from_secs(30 * 60));
    }

    #[test]
    fn next_sleep_after_success_stays_within_jitter_window() {
        let min = SUCCESS_BASE_INTERVAL.saturating_sub(SUCCESS_JITTER);
        let max = SUCCESS_BASE_INTERVAL.saturating_add(SUCCESS_JITTER);
        for _ in 0..2_000 {
            let d = next_sleep_after(IterationOutcome::Success);
            assert!(
                d >= min && d <= max,
                "expected {:?} ∈ [{:?}, {:?}]",
                d,
                min,
                max
            );
        }
    }

    #[test]
    fn next_sleep_after_success_actually_jitters() {
        // 抽 200 个样本，至少出现 2 个不同值（极大概率成立；接近 0
        // 概率失败的均匀采样实现也是 bug）
        let mut samples = std::collections::HashSet::new();
        for _ in 0..200 {
            samples.insert(next_sleep_after(IterationOutcome::Success).as_secs());
        }
        assert!(
            samples.len() > 1,
            "jitter produced a single value across 200 samples: {:?}",
            samples
        );
    }

    #[test]
    fn intervals_match_plan_constants() {
        // 锁住 task_plan 里写的 6h / 15min / 30min 约定，防止后人误调
        assert_eq!(SUCCESS_BASE_INTERVAL, Duration::from_secs(6 * 60 * 60));
        assert_eq!(SUCCESS_JITTER, Duration::from_secs(15 * 60));
        assert_eq!(FAILURE_RETRY_INTERVAL, Duration::from_secs(30 * 60));
        assert_eq!(SETUP_POLL_INTERVAL, Duration::from_secs(30));
    }

    // ---- wait_for_setup -----------------------------------------------------

    #[tokio::test]
    async fn wait_for_setup_returns_true_when_already_completed() {
        let port: Arc<dyn SetupStatusPort> = FakeSetupStatus::always_completed();
        let token = CancellationToken::new();
        assert!(wait_for_setup(&port, &token).await);
    }

    #[tokio::test]
    async fn wait_for_setup_returns_false_when_cancelled_before_completion() {
        let port: Arc<dyn SetupStatusPort> = FakeSetupStatus::never_completed();
        let token = CancellationToken::new();
        let waiter_token = token.clone();
        let waiter = tokio::spawn(async move {
            let port: Arc<dyn SetupStatusPort> = FakeSetupStatus::never_completed();
            wait_for_setup(&port, &waiter_token).await
        });
        // 让 waiter 至少调一次 get_status 并进入 sleep
        tokio::task::yield_now().await;
        token.cancel();
        assert!(!waiter.await.unwrap());
        // silence unused-variable lint on `port`
        let _ = port;
    }

    #[tokio::test(start_paused = true)]
    async fn wait_for_setup_picks_up_eventual_completion() {
        let port = Arc::new(FakeSetupStatus {
            status: RwLock::new(SetupStatus::default()),
            reads: AtomicUsize::new(0),
            flip_after_n_reads: 3, // 第 3 次 get_status 才置位
        });
        let port_dyn: Arc<dyn SetupStatusPort> = port.clone();
        let token = CancellationToken::new();
        let waiter = tokio::spawn(async move { wait_for_setup(&port_dyn, &token).await });

        // 推进时钟 3 × poll interval；start_paused 让 sleep 立即满足
        for _ in 0..3 {
            tokio::time::advance(SETUP_POLL_INTERVAL).await;
        }
        let completed = waiter.await.unwrap();
        assert!(completed);
        assert!(port.reads.load(Ordering::SeqCst) >= 3);
    }
}
