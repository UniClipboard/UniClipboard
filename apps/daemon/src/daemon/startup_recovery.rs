//! daemon 启动恢复任务。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::Notify;
use tracing::{info_span, Instrument};
use uc_application::facade::{AppFacade, UpgradeStatus};
use uc_application::receive_reconciliation::{
    EnsureReceiveReadyPort, ReceiveReadinessError, ReceiveReadinessStatus,
};
use uc_core::ports::SettingsPort;
use uc_daemon_local::process_metadata::DaemonSpawnOrigin;
use uc_daemon_local::spawn_contract::unattended_from_env;
use uc_engine::internal::session_recovery::execute_recover_session;
use uc_engine::{OperationResult, RecoverSessionInput};

use super::run_mode::DaemonRunMode;

/// daemon 版本号。迁入 uc-daemon 后取本 crate 的 `CARGO_PKG_VERSION`——与原
/// uc-desktop `DAEMON_VERSION` 同为 workspace 版本，运行值不变（ADR-008 P1）。
const DAEMON_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct DaemonReceiveReadinessCoordinator {
    recovery: Arc<dyn EnsureReceiveReadyPort>,
    clipboard_capture_gate: Arc<AtomicBool>,
    deferred_ready_notify: Arc<Notify>,
}

impl DaemonReceiveReadinessCoordinator {
    pub fn new(
        recovery: Arc<dyn EnsureReceiveReadyPort>,
        clipboard_capture_gate: Arc<AtomicBool>,
        deferred_ready_notify: Arc<Notify>,
    ) -> Self {
        Self {
            recovery,
            clipboard_capture_gate,
            deferred_ready_notify,
        }
    }
}

#[async_trait::async_trait]
impl EnsureReceiveReadyPort for DaemonReceiveReadinessCoordinator {
    async fn ensure_receive_ready(&self) -> Result<(), ReceiveReadinessError> {
        self.recovery.ensure_receive_ready().await?;
        self.clipboard_capture_gate.store(true, Ordering::SeqCst);
        self.deferred_ready_notify.notify_one();
        Ok(())
    }

    fn close_receive_gate(&self) {
        self.recovery.close_receive_gate();
        self.clipboard_capture_gate.store(false, Ordering::SeqCst);
    }

    fn receive_readiness_status(&self) -> ReceiveReadinessStatus {
        self.recovery.receive_readiness_status()
    }
}

/// ADR-008 D9: whether this daemon is *attended* — i.e. respects the user's
/// `auto_unlock_enabled` setting because a GUI will connect and drive the
/// unlock. Attended iff the daemon was GUI-spawned, is not a headless node, and
/// was not launched strict-unattended. Every other launch force-unlocks.
fn is_attended(
    run_mode: DaemonRunMode,
    spawn_origin: DaemonSpawnOrigin,
    strict_unattended: bool,
) -> bool {
    matches!(spawn_origin, DaemonSpawnOrigin::Gui)
        && !matches!(run_mode, DaemonRunMode::ServerHeadless)
        && !strict_unattended
}

/// 启动恢复任务所需输入。
pub struct StartupRecoveryInput {
    pub run_mode: DaemonRunMode,
    pub(crate) app_facade: Arc<AppFacade>,
    pub settings: Arc<dyn SettingsPort>,
    pub receive_readiness: Arc<dyn EnsureReceiveReadyPort>,
}

/// 在后台恢复加密会话、空间会话和 presence。
///
/// 恢复动作可能触发系统钥匙串，启动阶段不能同步等待它完成；daemon 先把
/// HTTP 监听拉起来，再让这个后台任务慢慢恢复。
pub fn spawn_startup_recovery(input: StartupRecoveryInput) {
    // One-shot best-effort recovery with no natural lifecycle owner to hold a
    // handle. Use `spawn_supervised` so a panic mid-recovery surfaces as a log
    // line instead of vanishing silently (see `uc-infra/AGENTS.md §13.3`).
    uc_observability::spawn_supervised("daemon.startup_recovery", async move {
        // ADR-008 D9 (P4-2): only an *attended* daemon respects the user's
        // `auto_unlock_enabled` setting. Attended = this daemon was spawned by
        // a GUI (which will connect and drive unlock + `POST /lifecycle/ready`),
        // is not a headless node, and was not launched strict-unattended. For
        // an attended daemon with `auto_unlock_enabled = false` we stay locked
        // and let the GUI unlock — the deferred services are released by the
        // GUI's lifecycle/ready once the session becomes ready.
        //
        // Every other launch — interactive `uniclip start`, strict-unattended
        // autostart, headless, or a manually-run `uniclipd` — has no GUI
        // fallback, so it force-unlocks via keyring (the historical behavior):
        // without an unlock the clipboard/sync workers stay stuck in the
        // deferred queue and the daemon looks alive while doing nothing.
        let attended = is_attended(
            input.run_mode,
            DaemonSpawnOrigin::from_env(),
            unattended_from_env(),
        );

        let auto_unlock_enabled = if attended {
            let settings = input.settings.load().await.unwrap_or_default();
            settings.security.auto_unlock_enabled
        } else {
            true
        };

        let recovery = execute_recover_session(
            &input.app_facade,
            input.receive_readiness.as_ref(),
            RecoverSessionInput {
                allow_secure_storage_unlock: auto_unlock_enabled,
            },
        )
        .instrument(info_span!("daemon.startup.recover_session"))
        .await;

        match recovery {
            Ok(OperationResult::SessionRecovered {
                unlocked: true,
                resumed,
            }) => {
                tracing::info!(
                    resumed,
                    "background unlock: engine session recovery completed"
                );
            }
            Ok(OperationResult::SessionRecovered {
                unlocked: false, ..
            }) => {
                tracing::info!("background unlock: engine session remains locked");
            }
            Ok(_) => {
                tracing::warn!("background unlock: engine returned an unexpected recovery result");
            }
            Err(error) => {
                tracing::warn!(error = %error, "background unlock: engine session recovery failed");
            }
        }
    });
}

pub(crate) async fn record_upgrade_status_at_startup(app_facade: &AppFacade) {
    let status = match app_facade.upgrade.detect_on_startup(DAEMON_VERSION).await {
        Ok(status) => status,
        Err(error) => {
            tracing::warn!(
                target: "upgrade",
                error = %error,
                "detect_on_startup failed; skipping upgrade decision this boot"
            );
            return;
        }
    };

    match &status {
        UpgradeStatus::FreshInstall => {
            tracing::info!(
                target: "upgrade",
                current = DAEMON_VERSION,
                "fresh install detected"
            );
            // Seal fresh installs immediately. Otherwise the next boot can
            // misclassify a newly completed setup as an unknown upgrade.
            if let Err(error) = app_facade.upgrade.acknowledge(DAEMON_VERSION).await {
                tracing::warn!(
                    target: "upgrade",
                    error = %error,
                    current = DAEMON_VERSION,
                    "fresh install detected but failed to seal version cursor; \
                     re-pair notice may surface on this device until next boot"
                );
            }
        }
        UpgradeStatus::NoChange => {
            tracing::info!(
                target: "upgrade",
                current = DAEMON_VERSION,
                "version cursor matches current build"
            );
        }
        UpgradeStatus::Upgraded { from, to } => {
            tracing::info!(
                target: "upgrade",
                from = from.as_ref().map(|v| v.to_string()).as_deref().unwrap_or("<unknown>"),
                to = %to,
                "upgrade detected"
            );
            // Known upgrades have no user-facing acknowledgement path, so the
            // daemon advances their cursor. Unknown upgrades remain pending
            // until the GUI has shown and dismissed the re-pair notice.
            if from.is_some() {
                if let Err(error) = app_facade.upgrade.acknowledge(DAEMON_VERSION).await {
                    tracing::warn!(
                        target: "upgrade",
                        error = %error,
                        to = %to,
                        "known upgrade detected but failed to seal version cursor; \
                         upgrade will be re-detected next boot"
                    );
                }
            }
        }
        UpgradeStatus::Downgraded { from, to } => {
            tracing::info!(
                target: "upgrade",
                from = %from,
                to = %to,
                "downgrade detected — rolled back to an older build"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct Recovery {
        fail: AtomicBool,
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl EnsureReceiveReadyPort for Recovery {
        async fn ensure_receive_ready(&self) -> Result<(), ReceiveReadinessError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail.load(Ordering::SeqCst) {
                Err(ReceiveReadinessError::Recovery(
                    "encrypted artifact metadata is unavailable".to_string(),
                ))
            } else {
                Ok(())
            }
        }

        fn close_receive_gate(&self) {}

        fn receive_readiness_status(&self) -> ReceiveReadinessStatus {
            ReceiveReadinessStatus {
                ready: !self.fail.load(Ordering::SeqCst),
                degraded_reason: self
                    .fail
                    .load(Ordering::SeqCst)
                    .then(|| "encrypted artifact metadata is unavailable".to_string()),
            }
        }
    }

    #[tokio::test]
    async fn receive_recovery_failure_keeps_runtime_gates_closed_until_retry_succeeds() {
        let recovery = Arc::new(Recovery {
            fail: AtomicBool::new(true),
            calls: AtomicUsize::new(0),
        });
        let clipboard_gate = Arc::new(AtomicBool::new(false));
        let notify = Arc::new(Notify::new());
        let coordinator = DaemonReceiveReadinessCoordinator::new(
            recovery.clone(),
            clipboard_gate.clone(),
            notify.clone(),
        );

        coordinator
            .ensure_receive_ready()
            .await
            .expect_err("failed recovery must be reported");
        assert!(!clipboard_gate.load(Ordering::SeqCst));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), notify.notified())
                .await
                .is_err(),
            "failed recovery must not release deferred services"
        );

        recovery.fail.store(false, Ordering::SeqCst);
        coordinator
            .ensure_receive_ready()
            .await
            .expect("retry should recover");

        assert!(clipboard_gate.load(Ordering::SeqCst));
        tokio::time::timeout(std::time::Duration::from_secs(1), notify.notified())
            .await
            .expect("successful recovery must release deferred services");
        assert_eq!(recovery.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn only_gui_spawned_standalone_is_attended() {
        // The one attended case: GUI-spawned, not headless, not strict.
        assert!(is_attended(
            DaemonRunMode::Standalone,
            DaemonSpawnOrigin::Gui,
            false
        ));
    }

    #[test]
    fn cli_and_manual_launches_force_unlock() {
        for origin in [DaemonSpawnOrigin::Cli, DaemonSpawnOrigin::Unknown] {
            assert!(
                !is_attended(DaemonRunMode::Standalone, origin, false),
                "{origin:?} has no GUI fallback — must force-unlock"
            );
        }
    }

    #[test]
    fn headless_and_strict_unattended_force_unlock_even_if_gui_spawned() {
        assert!(
            !is_attended(DaemonRunMode::ServerHeadless, DaemonSpawnOrigin::Gui, false),
            "headless has no display/GUI surface — force-unlock"
        );
        assert!(
            !is_attended(DaemonRunMode::Standalone, DaemonSpawnOrigin::Gui, true),
            "strict-unattended overrides the GUI origin — force-unlock"
        );
    }
}
