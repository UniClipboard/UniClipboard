//! # Dependency Injection / 依赖注入模块
//!
//! ## Responsibilities / 职责
//!
//! - ✅ Create infra implementations (db, fs, secure storage) / 创建 infra 层具体实现
//! - ✅ Create platform implementations (clipboard, network) / 创建 platform 层具体实现
//! - ✅ Inject all dependencies into App / 将所有依赖注入到 App
//!
//! ## Prohibited / 禁止事项
//!
//! ❌ **No business logic / 禁止包含任何业务逻辑**
//! - Do not decide "what to do if encryption uninitialized"
//! - 不判断"如果加密未初始化就怎样"
//! - Do not handle "what to do if device not registered"
//! - 不处理"如果设备未注册就怎样"
//!
//! ❌ **No configuration validation / 禁止做配置验证**
//! - Config already loaded in config.rs
//! - 配置已在 config.rs 加载
//! - Validation should be in use case or upper layer
//! - 验证应在 use case 或上层
//!
//! ❌ **No direct concrete implementation usage / 禁止直接使用具体实现**
//! - Must inject through Port traits
//! - 必须通过 Port trait 注入
//! - Do not call implementation methods directly after App construction
//! - 不在 App 构造后直接调用实现方法
//!
//! ## Architecture Principle / 架构原则
//!
//! > **This is the only place allowed to depend on uc-infra + uc-platform + uc-app simultaneously.**
//! > **这是唯一允许同时依赖 uc-infra、uc-platform 和 uc-app 的地方。**
//! > But this privilege is only for "assembly", not for "decision making".
//! > 但这种特权仅用于"组装"，不用于"决策"。

use std::sync::Arc;
use tauri::async_runtime;
use tokio::time::{self, Duration};
use tracing::{info, warn};

use uc_application::facade::ClipboardHistoryFacade;
use uc_bootstrap::TaskRegistry;
use uc_daemon_client::{DaemonConnectionState, DaemonPairingClient};

// Re-export assembly types from uc-bootstrap.
pub use uc_bootstrap::assembly::{
    get_storage_paths, resolve_pairing_device_name, wire_dependencies, WiredDependencies,
    WiringError, WiringResult,
};

// Re-export BackgroundRuntimeDeps from uc-bootstrap (definition moved in Phase 40).
pub use uc_bootstrap::BackgroundRuntimeDeps;

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
pub fn start_background_tasks(
    history_facade: Arc<ClipboardHistoryFacade>,
    task_registry: &Arc<TaskRegistry>,
) {
    let registry = task_registry.clone();
    async_runtime::spawn(async move {
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
pub fn start_gui_pairing_lease_task(
    connection_state: DaemonConnectionState,
    task_registry: &Arc<TaskRegistry>,
) {
    let registry = task_registry.clone();
    async_runtime::spawn(async move {
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
