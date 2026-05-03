//! 桌面 GUI 进程的后台任务调度（GUI-framework agnostic）。
//!
//! 这里只关心"什么时候跑、跑在哪个 tokio runtime"——任务的业务定义留在
//! `uc-application` 的 facade。各 shell（Tauri、未来 native）调用同一组函数
//! 启动这些任务，shell 只负责持有 tokio runtime 与 `TaskRegistry`。
//!
//! 实现纯 tokio——`tauri::async_runtime` 在 desktop target 下就是 tokio，
//! 直接用 `tokio::spawn` 等价且让本模块不依赖任何 GUI 框架。

use std::sync::Arc;

use tokio::time::{self, Duration};
use tracing::{info, warn};

use uc_application::facade::ClipboardHistoryFacade;
use uc_bootstrap::TaskRegistry;
use uc_daemon_client::{DaemonConnectionState, DaemonPairingClient};

const GUI_PAIRING_LEASE_TTL_MS: u64 = 300_000;
const GUI_PAIRING_LEASE_REFRESH_INTERVAL: Duration = Duration::from_secs(120);

/// Start the file cache cleanup task (runs once at startup, fire-and-forget).
///
/// Cleanup goes through `ClipboardHistoryFacade::cleanup_expired_files`,
/// which routes every expired file through the entry-aware delete path
/// (untag iroh-blobs reference + remove cache file + drop sqlite rows
/// in one shot). Pre-Phase-C this called a standalone use case that
/// `tokio::fs::remove_file`-d cache files directly, leaving iroh-blobs
/// metadata pointing at vanished files (the precursor to the Poisoned
/// BaoFileStorage panic at `bao_file.rs:410`).
pub fn start_file_cache_cleanup(
    history_facade: Arc<ClipboardHistoryFacade>,
    task_registry: &Arc<TaskRegistry>,
) {
    let registry = task_registry.clone();
    tokio::spawn(async move {
        registry
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
    });
}

/// Keep the GUI discoverability/participant lease alive for the daemon pairing host.
///
/// This task is owned by the GUI lifecycle rather than any specific frontend page.
/// As long as the desktop app is running, the daemon should treat the GUI as
/// ready to receive inbound pairing requests.
pub fn start_gui_pairing_lease(
    connection_state: DaemonConnectionState,
    task_registry: &Arc<TaskRegistry>,
) {
    let registry = task_registry.clone();
    tokio::spawn(async move {
        registry
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
    });
}

async fn run_gui_pairing_lease_loop<F, Fut>(
    token: tokio_util::sync::CancellationToken,
    lease_ttl_ms: u64,
    renew_interval: Duration,
    mut set_gui_lease: F,
) where
    F: FnMut(bool, Option<u64>) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    if let Err(error) = set_gui_lease(true, Some(lease_ttl_ms)).await {
        warn!(error = %error, "failed to register GUI pairing lease");
    } else {
        info!(lease_ttl_ms, "registered GUI pairing lease");
    }

    let mut ticker = time::interval(renew_interval);
    ticker.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    ticker.tick().await;

    loop {
        tokio::select! {
            _ = token.cancelled() => {
                if let Err(error) = set_gui_lease(false, None).await {
                    warn!(error = %error, "failed to release GUI pairing lease during shutdown");
                } else {
                    info!("released GUI pairing lease");
                }
                return;
            }
            _ = ticker.tick() => {
                if let Err(error) = set_gui_lease(true, Some(lease_ttl_ms)).await {
                    warn!(error = %error, "failed to renew GUI pairing lease");
                }
            }
        }
    }
}
