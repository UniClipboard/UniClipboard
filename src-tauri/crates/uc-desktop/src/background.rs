//! 桌面 GUI 进程的后台任务调度（GUI-framework agnostic）。
//!
//! 这里只关心任务的"注册与生命周期协调"——任务的业务定义留在
//! `uc-application` 的 facade。每个 starter 都是 `async fn`：它本身不
//! `spawn` 任何东西，只是 `await` `TaskRegistry::spawn(...)` 把任务注册
//! 进 registry。**进入 async 上下文的方式由 shell 决定**——Tauri shell 用
//! `tauri::async_runtime::spawn`（Tauri 持有的全局 tokio runtime），未来
//! native shell 用自己的 tokio handle。这样本模块完全不需要触发
//! `tokio::spawn`，从而不依赖"调用线程必须已经处于 tokio runtime 上下文"
//! 这个隐式假设——这正是 Tauri 的 `setup` 闭包不满足的假设。
//!
//! 这种"async fn + caller 决定 spawn"的形态与
//! [`uc_bootstrap::spawn_blob_processing_tasks`] 一致。

use std::sync::Arc;

use tokio::time::{self, Duration};
use tracing::{info, warn};

use uc_application::facade::ClipboardHistoryFacade;
use uc_bootstrap::TaskRegistry;
use uc_daemon_client::{DaemonConnectionState, DaemonPairingClient};

const GUI_PAIRING_LEASE_TTL_MS: u64 = 300_000;
const GUI_PAIRING_LEASE_REFRESH_INTERVAL: Duration = Duration::from_secs(120);
/// 单次 lease RPC 的兜底超时——daemon 卡死时 shutdown 路径不能被它拖住，
/// 否则 `task_registry.token().cancel()` 等不到 lease task 退出，整个 GUI
/// 退出会被无限阻塞。注册/续租阶段超时只是 warn，真正关键是 cancel 分支。
const GUI_PAIRING_LEASE_RPC_TIMEOUT: Duration = Duration::from_secs(5);

/// Register the file cache cleanup task with `TaskRegistry`.
///
/// Cleanup goes through `ClipboardHistoryFacade::cleanup_expired_files`,
/// which routes every expired file through the entry-aware delete path
/// (untag iroh-blobs reference + remove cache file + drop sqlite rows
/// in one shot). Pre-Phase-C this called a standalone use case that
/// `tokio::fs::remove_file`-d cache files directly, leaving iroh-blobs
/// metadata pointing at vanished files (the precursor to the Poisoned
/// BaoFileStorage panic at `bao_file.rs:410`).
///
/// Caller must drive this future inside a tokio runtime context (e.g.
/// `tauri::async_runtime::spawn(async move { start_file_cache_cleanup(...).await })`).
pub async fn start_file_cache_cleanup(
    history_facade: Arc<ClipboardHistoryFacade>,
    task_registry: &Arc<TaskRegistry>,
) {
    task_registry
        .spawn("file_cache_cleanup", |_token| async move {
            match history_facade.cleanup_expired_files().await {
                Ok(result) => {
                    if result.files_removed > 0 {
                        info!(
                            files_removed = result.files_removed,
                            entries_deleted = result.entries_deleted,
                            orphans_removed = result.orphans_removed,
                            bytes_reclaimed = result.bytes_reclaimed,
                            errors = result.errors,
                            "Startup file cache cleanup completed"
                        );
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Startup file cache cleanup failed (non-fatal)");
                }
            }
        })
        .await;
    info!("All background tasks registered with TaskRegistry");
}

/// Register the GUI pairing lease keepalive loop with `TaskRegistry`.
///
/// As long as the desktop app is running, the daemon should treat the GUI as
/// ready to receive inbound pairing requests. The loop is owned by the GUI
/// lifecycle rather than any specific frontend page.
///
/// Caller must drive this future inside a tokio runtime context (see
/// [`start_file_cache_cleanup`] for the same caller-driven pattern).
pub async fn start_gui_pairing_lease(
    connection_state: DaemonConnectionState,
    task_registry: &Arc<TaskRegistry>,
) {
    task_registry
        .spawn("gui_pairing_lease", move |token| async move {
            let client = DaemonPairingClient::new(connection_state);
            run_gui_pairing_lease_loop(
                token,
                GUI_PAIRING_LEASE_TTL_MS,
                GUI_PAIRING_LEASE_REFRESH_INTERVAL,
                move |enabled, ttl_ms| {
                    let client = client.clone();
                    async move { client.register_gui_participant(enabled, ttl_ms).await }
                },
            )
            .await;
        })
        .await;
}

/// Visible to the test module; production callers use [`start_gui_pairing_lease`].
pub(crate) async fn run_gui_pairing_lease_loop<F, Fut>(
    token: tokio_util::sync::CancellationToken,
    lease_ttl_ms: u64,
    renew_interval: Duration,
    mut set_gui_lease: F,
) where
    F: FnMut(bool, Option<u64>) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    match time::timeout(
        GUI_PAIRING_LEASE_RPC_TIMEOUT,
        set_gui_lease(true, Some(lease_ttl_ms)),
    )
    .await
    {
        Ok(Ok(())) => info!(lease_ttl_ms, "registered GUI pairing lease"),
        Ok(Err(error)) => warn!(error = %error, "failed to register GUI pairing lease"),
        Err(_) => warn!(
            timeout_ms = GUI_PAIRING_LEASE_RPC_TIMEOUT.as_millis() as u64,
            "GUI pairing lease registration RPC timed out"
        ),
    }

    let mut ticker = time::interval(renew_interval);
    ticker.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    ticker.tick().await;

    loop {
        tokio::select! {
            _ = token.cancelled() => {
                match time::timeout(
                    GUI_PAIRING_LEASE_RPC_TIMEOUT,
                    set_gui_lease(false, None),
                )
                .await
                {
                    Ok(Ok(())) => info!("released GUI pairing lease"),
                    Ok(Err(error)) => warn!(
                        error = %error,
                        "failed to release GUI pairing lease during shutdown"
                    ),
                    Err(_) => warn!(
                        timeout_ms = GUI_PAIRING_LEASE_RPC_TIMEOUT.as_millis() as u64,
                        "GUI pairing lease release RPC timed out during shutdown"
                    ),
                }
                return;
            }
            _ = ticker.tick() => {
                match time::timeout(
                    GUI_PAIRING_LEASE_RPC_TIMEOUT,
                    set_gui_lease(true, Some(lease_ttl_ms)),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => warn!(error = %error, "failed to renew GUI pairing lease"),
                    Err(_) => warn!(
                        timeout_ms = GUI_PAIRING_LEASE_RPC_TIMEOUT.as_millis() as u64,
                        "GUI pairing lease renew RPC timed out"
                    ),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    /// Each call to the lease setter is recorded as a tuple of (enabled, ttl_ms).
    /// We assert against this log to verify the register / renew / release sequence.
    type LeaseLog = Arc<Mutex<Vec<(bool, Option<u64>)>>>;

    fn recording_setter(
        log: LeaseLog,
        result: Result<(), String>,
    ) -> impl FnMut(
        bool,
        Option<u64>,
    ) -> std::pin::Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        move |enabled, ttl| {
            let log = log.clone();
            let result = result.clone();
            Box::pin(async move {
                log.lock().unwrap().push((enabled, ttl));
                result.map_err(|s| anyhow::anyhow!(s)).map(|()| ())
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn registers_lease_then_releases_on_cancel_without_renewing() {
        // Cancel before the renew interval fires — we should see exactly
        // one register call (enabled=true, ttl=Some) and one release call
        // (enabled=false, ttl=None), with no renews in between.
        let log: LeaseLog = Arc::new(Mutex::new(Vec::new()));
        let token = CancellationToken::new();
        let token_for_loop = token.clone();
        let setter = recording_setter(log.clone(), Ok(()));

        let loop_handle = tokio::spawn(async move {
            run_gui_pairing_lease_loop(token_for_loop, 300_000, Duration::from_secs(120), setter)
                .await;
        });

        // Yield so the initial register call runs before we cancel.
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(1)).await;
        token.cancel();
        loop_handle.await.expect("loop must complete after cancel");

        let calls = log.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec![(true, Some(300_000)), (false, None)],
            "expected register-then-release sequence, no renews"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn renews_lease_periodically_until_cancelled() {
        // Advance time past several renew intervals and verify each tick
        // produces a renew call. The first call is the initial register.
        let log: LeaseLog = Arc::new(Mutex::new(Vec::new()));
        let token = CancellationToken::new();
        let token_for_loop = token.clone();
        let renew = Duration::from_secs(120);
        let setter = recording_setter(log.clone(), Ok(()));

        let loop_handle = tokio::spawn(async move {
            run_gui_pairing_lease_loop(token_for_loop, 300_000, renew, setter).await;
        });

        // Initial register — yield once to let the loop start and register.
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(1)).await;
        tokio::task::yield_now().await;

        // Advance past 3 renew intervals.
        for _ in 0..3 {
            tokio::time::advance(renew).await;
            tokio::task::yield_now().await;
        }

        token.cancel();
        loop_handle.await.expect("loop completes after cancel");

        let calls = log.lock().unwrap().clone();
        // Expected: [register, renew, renew, renew, release]. The initial
        // ticker.tick() consumes the immediately-fired tick so renews start
        // after the FIRST `renew_interval`, not at t=0.
        assert!(
            calls.len() >= 5,
            "expected at least register + 3 renews + release, got {} calls: {:?}",
            calls.len(),
            calls
        );
        assert_eq!(
            calls.first(),
            Some(&(true, Some(300_000))),
            "first call must be initial register"
        );
        assert_eq!(
            calls.last(),
            Some(&(false, None)),
            "last call must be release"
        );
        // All non-final calls must be enabled=true with the configured TTL.
        for (i, call) in calls.iter().enumerate().skip(1).take(calls.len() - 2) {
            assert_eq!(
                call,
                &(true, Some(300_000)),
                "renew call #{i} must keep the same lease shape: {call:?}"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn release_call_runs_even_if_setter_returns_error() {
        // If renew fails we just log a warning — but on cancellation we
        // still issue the release call. Errors must not poison the loop.
        let log: LeaseLog = Arc::new(Mutex::new(Vec::new()));
        let token = CancellationToken::new();
        let token_for_loop = token.clone();
        // All setter calls fail — we still expect a release attempt at cancel.
        let setter = recording_setter(log.clone(), Err("transport down".into()));

        let loop_handle = tokio::spawn(async move {
            run_gui_pairing_lease_loop(token_for_loop, 60_000, Duration::from_secs(30), setter)
                .await;
        });

        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(1)).await;
        token.cancel();
        loop_handle
            .await
            .expect("loop completes despite setter errors");

        let calls = log.lock().unwrap().clone();
        assert!(
            calls.contains(&(true, Some(60_000))),
            "register must be attempted even though it errors: {calls:?}"
        );
        assert!(
            calls.contains(&(false, None)),
            "release must be attempted on cancel even after setter errors: {calls:?}"
        );
    }
}
