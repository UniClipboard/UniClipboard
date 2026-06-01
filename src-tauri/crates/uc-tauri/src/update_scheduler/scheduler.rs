//! 后台周期更新检查 scheduler 主循环。
//!
//! 本模块负责"什么时候 check / 怎么 backoff / 何时让位关停"，并在检测到
//! 新版本时联动通知发送 + 去重持久化 + 条件 auto-download（Phase 4B）。
//!
//! 时序：
//! - 启动后先 poll `SetupStatus.has_completed`，setup 未完成时每 30s 重试
//! - 主循环：
//!   1. load settings；`auto_check_update == false` 当作 idle，不 emit
//!      telemetry，按成功 cadence 继续轮询（让用户开关切换无 30min 惩罚）
//!   2. true 时调 `do_check_for_update` 内部入口 + emit
//!      `update_check_performed { source: scheduled, ... }`
//!   3. `Available` 分支：去重检测 → `send_update_notification` →
//!      emit `update_notification_shown` → 投递成功才 `record` 持久化；
//!      若 `auto_download_update == true` 且 install_kind 在 in-place
//!      可更新列表（macOS/Windows/AppImage）→ 调 `do_download_update` +
//!      emit `update_action_invoked` Started + terminal 配对
//!   4. 成功 6h ± 15min jitter；失败 30min（Q9：固定，不是指数 backoff）
//! - 任一 sleep 内被 cancellation token 打断 → 立即退出

use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use tauri::{AppHandle, Manager};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uc_core::ports::{SettingsPort, SetupStatusPort};
use uc_core::settings::channel::detect_channel;
use uc_core::settings::model::UpdateChannel;
use uc_observability::analytics::{
    Event, UpdateAction, UpdateActionOutcome, UpdateCheckOutcome, UpdateCheckSource,
};

use super::last_check_at::LastCheckAt;
use super::notify_context::NotifyContext;
use super::window::open_or_focus_updater_window;
use crate::commands::updater::{
    classify_check_failure, detect_install_kind, do_check_for_update, do_download_update,
    install_kind_for_telemetry, DownloadError, InstallKind, PendingUpdate,
};

/// Setup 未完成时的轮询间隔（Q16.1：30s，不订阅事件）。
const SETUP_POLL_INTERVAL: Duration = Duration::from_secs(30);
/// 成功 / idle 后下一轮 check 的基准间隔（Q9：6h）。
pub(crate) const SUCCESS_BASE_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// 成功 / idle 后的 jitter 上限（Q9：±15min，避免所有客户端同步轰炸 release CDN）。
pub(crate) const SUCCESS_JITTER: Duration = Duration::from_secs(15 * 60);
/// 失败重试间隔（Q9：固定 30min，不是指数 backoff）。
pub(crate) const FAILURE_RETRY_INTERVAL: Duration = Duration::from_secs(30 * 60);
/// 被「原生唤醒源」(macOS NSBackgroundActivityScheduler fire / Windows 从睡眠
/// 恢复) 叫醒后，距上次任意 source 的 check 至少这么久，才真正补一次检查。
///
/// 低于它说明刚查过（例如 cadence 刚跑完、或只休眠了几分钟就恢复），跳过以免
/// 重复打 release CDN。取 1h：远低于 6h cadence 减 tolerance，保证正常的 6h
/// 后台活动 fire 一定能通过；又足够高，挡住频繁的 resume 抖动。
pub(crate) const WAKE_MIN_RECHECK_SECS: i64 = 60 * 60;

/// Scheduler 启动所需的全部依赖。
///
/// 持有 strong refs；scheduler task 生命周期由 `CancellationToken` 与
/// `task_registry.shutdown()` 联合管理（见 `run.rs:589` ExitRequested
/// 路径，Phase 3C 接入）。
///
/// 与"通知/弹窗/去重"相关的依赖打包在 [`NotifyContext`] 里，scheduler
/// 主循环和托盘手动检查通过 `Arc<NotifyContext>` 共享同一份。
pub struct SchedulerDeps {
    pub settings_port: Arc<dyn SettingsPort>,
    pub setup_status_port: Arc<dyn SetupStatusPort>,
    pub notify: Arc<NotifyContext>,
}

/// 启动 scheduler 主循环。调用方 `run.rs` 内 `task_registry.spawn` 它，把
/// `task_registry.child_token()` 和一个 `wake_rx` 传进来。
///
/// `wake_rx`：平台原生唤醒源 (`wake_source`) 的接收端。macOS App Nap /
/// Windows Modern Standby 会挂起下面 `main_loop` 里的 `tokio::sleep`，使后台
/// 周期检查迟迟不发车（历史症状：「只有打开主窗口才检测/弹更新窗口」）。原生
/// 唤醒源在那种状态下仍会触发，通过此 channel 把主循环从被挂起的 sleep 里叫醒。
pub async fn run(
    deps: SchedulerDeps,
    wake_rx: tokio::sync::mpsc::Receiver<()>,
    token: CancellationToken,
) {
    info!(target: "update_scheduler", "starting");

    // Phase 4C: install_kind 在进程生命期内不变（用户不会从 dpkg 包切到 rpm
    // 包还跑同一个 binary）。检测一次缓存在 task stack：Linux 路径会跑
    // `dpkg-query` / `rpm -qf` 子进程，从 async 上下文同步调用会短暂阻塞
    // tokio worker，所以走 `spawn_blocking` 把它隔离到 blocking pool。
    // macOS / Windows 路径是常量返回，spawn_blocking 的开销远低于一次跨线程
    // 调度——但为了走同一条代码路径，仍统一通过 spawn_blocking。
    let install_kind = detect_install_kind_async().await;
    info!(
        target: "update_scheduler",
        install_kind = ?install_kind,
        "install kind detected"
    );

    if !wait_for_setup(&deps.setup_status_port, &token).await {
        info!(target: "update_scheduler", "cancelled before setup completed");
        return;
    }
    info!(target: "update_scheduler", "setup completed; entering main loop");
    main_loop(&deps, install_kind, wake_rx, token).await;
    info!(target: "update_scheduler", "exited main loop");
}

/// 在 blocking pool 上跑 `detect_install_kind` 一次。Panic 视为"未知打包形态"
/// 兜底——scheduler 不能因为一次 install kind 探测异常就整体崩溃；后续
/// `should_auto_download(Unknown) == false` 自然 short-circuit 自动下载。
async fn detect_install_kind_async() -> InstallKind {
    match tokio::task::spawn_blocking(detect_install_kind).await {
        Ok(kind) => kind,
        Err(err) => {
            warn!(
                target: "update_scheduler",
                error = %err,
                "detect_install_kind task panicked; defaulting to Unknown"
            );
            InstallKind::Unknown
        }
    }
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

async fn main_loop(
    deps: &SchedulerDeps,
    install_kind: InstallKind,
    mut wake_rx: tokio::sync::mpsc::Receiver<()>,
    token: CancellationToken,
) {
    // 启动即查一次（保持原行为：进入等待前先跑一轮）。
    let mut outcome = run_one_iteration(deps, install_kind).await;
    loop {
        let sleep_dur = next_sleep_after(outcome);
        debug!(
            target: "update_scheduler",
            outcome = ?outcome,
            sleep_secs = sleep_dur.as_secs(),
            "iteration done; scheduling next"
        );
        // Create a single sleep deadline to preserve the cadence even when a wake is skipped.
        let deadline = Instant::now() + sleep_dur;
        let sleep_future = tokio::time::sleep_until(deadline);
        tokio::pin!(sleep_future);

        tokio::select! {
            _ = token.cancelled() => return,
            _ = &mut sleep_future => {
                // 整段 cadence 睡满 → 正常补检查。
                outcome = run_one_iteration(deps, install_kind).await;
            }
            recv = wake_rx.recv() => match recv {
                Some(()) => {
                    // 被原生唤醒源叫醒（App Nap 退出 / 从睡眠恢复 / 后台活动 fire）。
                    // 当 tokio sleep 在后台被挂起时，这是唯一能让 check 发车的路径。
                    // 用墙钟 LastCheckAt 做 guard：刚查过就跳过，避免重复打 CDN。
                    let since = deps
                        .notify
                        .app_handle
                        .state::<LastCheckAt>()
                        .seconds_since();
                    if since >= WAKE_MIN_RECHECK_SECS {
                        info!(
                            target: "update_scheduler",
                            seconds_since = since,
                            "native wake source fired; running update check"
                        );
                        outcome = run_one_iteration(deps, install_kind).await;
                    } else {
                        debug!(
                            target: "update_scheduler",
                            seconds_since = since,
                            "native wake source fired but checked recently; skipping"
                        );
                        // Preserve the original cadence by continuing to wait on the same sleep deadline.
                        continue;
                    }
                }
                None => {
                    // 所有 sender 已 drop。理论上不会发生：run.rs 把一份 sender
                    // 作为 keepalive 移进本 task。真出现就退化为纯 cadence——
                    // 并停止 select wake_rx，否则 recv() 会立即返回 None 形成
                    // busy loop。
                    warn!(
                        target: "update_scheduler",
                        "wake channel closed; falling back to cadence-only loop"
                    );
                    cadence_only_loop(deps, install_kind, outcome, token).await;
                    return;
                }
            }
        }
    }
}

/// 唤醒 channel 关闭后的退化路径：丢掉 `wake_rx`，只靠 tokio cadence 驱动。
/// 与改造前的 `main_loop` 行为一致——后台可能被 App Nap 拖慢，但前台仍可用。
async fn cadence_only_loop(
    deps: &SchedulerDeps,
    install_kind: InstallKind,
    mut outcome: IterationOutcome,
    token: CancellationToken,
) {
    loop {
        let sleep_dur = next_sleep_after(outcome);
        tokio::select! {
            _ = token.cancelled() => return,
            _ = tokio::time::sleep(sleep_dur) => {}
        }
        outcome = run_one_iteration(deps, install_kind).await;
    }
}

async fn run_one_iteration(
    deps: &SchedulerDeps,
    install_kind_raw: InstallKind,
) -> IterationOutcome {
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

    let app_version = deps.notify.app_handle.package_info().version.to_string();
    let resolved_channel = resolve_channel(settings.general.update_channel.clone(), &app_version);
    let app = deps.notify.app_handle.clone();
    let pending = app.state::<PendingUpdate>();
    let result = do_check_for_update(&app, Some(resolved_channel.clone()), pending.inner()).await;
    // Phase 5B: 任何 source 的 check 完成（成功或失败）都标记时间戳，让
    // `show_main_window` 顺手检查阈值能正确感知最近一次活动。"Downloading
    // 状态拒绝"会经 `Err` 路径触达这里 —— 不算真 HTTP 尝试，但也意味着
    // updater 子系统活跃，30min 内不再额外触发 window_show check 是合理的。
    app.state::<LastCheckAt>().record_now();

    // install_kind 在 `run()` 入口一次性探测后沿调用链传入，避免每轮迭代再
    // 调一次同步函数（Linux 路径有 OnceLock 命中但仍是一次原子读 + cfg!()
    // 分支，能省则省）。
    let install_kind = install_kind_for_telemetry(install_kind_raw);

    // Available 分支：通知去重 + 条件 auto-download。在 emit
    // `update_check_performed` 之前先处理副作用，这样 PostHog 上时序
    // 是 (notification_shown?, action_invoked Started?, check_performed,
    // action_invoked Terminal?)——与 manual 路径相符。
    if let Ok(Some(metadata)) = &result {
        let window_opened = deps
            .notify
            .notify_if_new_version(&resolved_channel, &metadata.version, install_kind)
            .await;
        if settings.general.auto_download_update && should_auto_download(install_kind_raw) {
            let downloaded_to_ready = auto_download(deps, &app, pending.inner()).await;
            // 兜底：去重让窗口没弹（之前的进程通知过这版本），但本进程
            // 又新下到了 Ready。用户从未在本次会话里见过更新提示——
            // 此时再开一次窗口，UI 会显示 "已下载，立即安装"。
            if downloaded_to_ready && !window_opened {
                if let Err(err) = open_or_focus_updater_window(&deps.notify.app_handle, false) {
                    warn!(
                        target: "update_scheduler",
                        error = %err,
                        "failed to open updater window after auto-download (ready fallback)"
                    );
                }
            }
        }
    }

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

    deps.notify.analytics.capture(Event::UpdateCheckPerformed {
        source: UpdateCheckSource::Scheduled,
        outcome,
        failure_kind,
        install_kind,
    });

    iter_outcome
}

/// Resolve the effective channel for this iteration.
///
/// 用户在 settings 显式设了 channel → 直接用；否则按 `app_version` 走
/// `uc-core::settings::channel::detect_channel` 兜底（与 `do_check_for_update`
/// 的内部默认逻辑保持一致——一个语义只能有一份实现）。
pub(crate) fn resolve_channel(
    settings_channel: Option<UpdateChannel>,
    app_version: &str,
) -> UpdateChannel {
    settings_channel.unwrap_or_else(|| detect_channel(app_version))
}

/// 给定 install kind，决定 scheduler 是否应该自动 in-place 下载新版本。
///
/// 仅 macOS / Windows / AppImage 走 tauri-plugin-updater 的 in-place 流程；
/// Deb / Rpm 由系统包管理器接管（PackageManagerUpdateDialog 引导用户），
/// scheduler 不应触发 in-place 下载——下载下来的包也装不进去。
/// `Unknown` 走防御性 false（找不到打包形态时宁可不动）。
pub(crate) fn should_auto_download(install_kind: InstallKind) -> bool {
    matches!(
        install_kind,
        InstallKind::Macos | InstallKind::Windows | InstallKind::AppImage
    )
}

/// 触发 in-place 自动下载，emit `update_action_invoked` Started + terminal 配对。
///
/// 与 `commands/updater.rs::download_update` Tauri command body 完全同
/// 模式：precondition 拒绝时不 emit Started + 不 emit terminal（funnel
/// 分母干净，OQ1 决议）。下载失败不重试——Q9 backoff 让下一轮 30min
/// 后再走一次完整 check。
///
/// 返回 `true` 表示这次确实下载到了 Ready 状态，给上层（scheduler iteration）
/// 用来判断是否需要兜底再开一次更新窗口。
async fn auto_download(deps: &SchedulerDeps, app: &AppHandle, pending: &PendingUpdate) -> bool {
    let result = do_download_update(app, pending).await;

    let did_start = !matches!(result, Err(DownloadError::Precondition(_)));
    if did_start {
        deps.notify.analytics.capture(Event::UpdateActionInvoked {
            action: UpdateAction::DownloadBg,
            outcome: UpdateActionOutcome::Started,
            error_kind: None,
        });
    }

    let terminal = match &result {
        Ok(()) => Some(UpdateActionOutcome::Succeeded),
        Err(DownloadError::Cancelled(_)) => Some(UpdateActionOutcome::Cancelled),
        Err(DownloadError::Failed(_)) => Some(UpdateActionOutcome::Failed),
        Err(DownloadError::Precondition(_)) => None,
    };
    if let Some(outcome) = terminal {
        deps.notify.analytics.capture(Event::UpdateActionInvoked {
            action: UpdateAction::DownloadBg,
            outcome,
            error_kind: result
                .as_ref()
                .err()
                .and_then(|e| e.error_kind())
                .map(|s| s.to_string()),
        });
    }

    matches!(result, Ok(()))
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

    // ---- resolve_channel ----------------------------------------------------

    #[test]
    fn resolve_channel_uses_settings_when_present() {
        // 用户显式选了 channel → 直接用，不看 app_version
        assert_eq!(
            resolve_channel(Some(UpdateChannel::Alpha), "0.12.0"),
            UpdateChannel::Alpha
        );
        assert_eq!(
            resolve_channel(Some(UpdateChannel::Beta), "0.12.0-alpha.1"),
            UpdateChannel::Beta
        );
    }

    #[test]
    fn resolve_channel_falls_back_to_detect_channel_when_none() {
        // settings 未设 → 走 uc-core detect_channel（按 app_version 推断）
        // 0.12.0 应该是 Stable
        assert_eq!(resolve_channel(None, "0.12.0"), UpdateChannel::Stable);
    }

    #[test]
    fn resolve_channel_prerelease_detection_via_app_version() {
        // app_version 含 `-alpha.` 走 uc-core::detect_channel 推断为 alpha
        let resolved = resolve_channel(None, "0.13.0-alpha.1");
        assert_eq!(
            resolved,
            UpdateChannel::Alpha,
            "expected detect_channel to map prerelease to Alpha"
        );
    }

    // ---- should_auto_download ----------------------------------------------

    #[test]
    fn should_auto_download_allows_inplace_targets() {
        for kind in [
            InstallKind::Macos,
            InstallKind::Windows,
            InstallKind::AppImage,
        ] {
            assert!(
                should_auto_download(kind),
                "expected auto-download for {kind:?}"
            );
        }
    }

    #[test]
    fn should_auto_download_blocks_system_packages_and_unknown() {
        // WindowsPortable is deliberately NOT in the in-place list: the NSIS
        // updater would install into Program Files instead of refreshing the
        // portable folder, so portable users update by downloading a new zip.
        for kind in [
            InstallKind::Deb,
            InstallKind::Rpm,
            InstallKind::WindowsPortable,
            InstallKind::Unknown,
        ] {
            assert!(
                !should_auto_download(kind),
                "expected NO auto-download for {kind:?} (handled by package manager / defensive)"
            );
        }
    }

    // ---- detect_install_kind_async -----------------------------------------

    #[tokio::test]
    async fn detect_install_kind_async_matches_sync_detection() {
        // Phase 4C: 异步版本仅是 `spawn_blocking(detect_install_kind)` 包装。
        // 结果应与同步路径完全一致——这道防线锁住未来若有人改 fallback 行为
        // （比如默认值偏到 macOS）必须先改本测试。
        let async_result = detect_install_kind_async().await;
        let sync_result = detect_install_kind();
        assert_eq!(async_result, sync_result);
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
