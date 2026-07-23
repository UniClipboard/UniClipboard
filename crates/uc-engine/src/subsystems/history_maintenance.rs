use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use uc_application::facade::clipboard_history::{
    CleanupResultView, ClipboardHistoryError, ClipboardHistoryFacade, ReconcileResultView,
    RetentionEnforcementResultView,
};
use uc_core::TaskRegistry;

const HISTORY_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(300);

#[async_trait]
trait HistoryMaintenance: Send + Sync {
    async fn reconcile_missing_files(&self) -> Result<ReconcileResultView, ClipboardHistoryError>;
    async fn cleanup_expired_files(&self) -> Result<CleanupResultView, ClipboardHistoryError>;
    async fn enforce_retention_policy(
        &self,
    ) -> Result<RetentionEnforcementResultView, ClipboardHistoryError>;
}

#[async_trait]
impl HistoryMaintenance for ClipboardHistoryFacade {
    async fn reconcile_missing_files(&self) -> Result<ReconcileResultView, ClipboardHistoryError> {
        ClipboardHistoryFacade::reconcile_missing_files(self).await
    }

    async fn cleanup_expired_files(&self) -> Result<CleanupResultView, ClipboardHistoryError> {
        ClipboardHistoryFacade::cleanup_expired_files(self).await
    }

    async fn enforce_retention_policy(
        &self,
    ) -> Result<RetentionEnforcementResultView, ClipboardHistoryError> {
        ClipboardHistoryFacade::enforce_retention_policy(self).await
    }
}

pub(crate) async fn spawn_history_maintenance_task(
    history: Arc<ClipboardHistoryFacade>,
    tasks: &Arc<TaskRegistry>,
) {
    run_history_maintenance_once(history.as_ref()).await;
    let maintenance: Arc<dyn HistoryMaintenance> = history;
    tasks
        .spawn("history_maintenance", move |cancel| async move {
            run_history_maintenance_loop(maintenance, cancel).await;
        })
        .await;
}

async fn run_history_maintenance_loop(
    maintenance: Arc<dyn HistoryMaintenance>,
    cancel: CancellationToken,
) {
    info!("History maintenance started");
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(HISTORY_MAINTENANCE_INTERVAL) => {}
        }
        run_history_maintenance_once(maintenance.as_ref()).await;
    }
    info!("History maintenance stopped");
}

async fn run_history_maintenance_once(maintenance: &dyn HistoryMaintenance) {
    match maintenance.reconcile_missing_files().await {
        Ok(result) => {
            if result.entries_deleted > 0 || result.errors > 0 {
                info!(
                    entries_scanned = result.entries_scanned,
                    entries_deleted = result.entries_deleted,
                    errors = result.errors,
                    "Reconciled history entries with missing files"
                );
            }
        }
        Err(_) => {
            warn!("History reconciliation failed; skipping remaining maintenance passes");
            return;
        }
    }

    match maintenance.cleanup_expired_files().await {
        Ok(result) => {
            if result.files_removed > 0 || result.errors > 0 {
                info!(
                    files_removed = result.files_removed,
                    entries_deleted = result.entries_deleted,
                    orphans_removed = result.orphans_removed,
                    bytes_reclaimed = result.bytes_reclaimed,
                    errors = result.errors,
                    "History file cache cleanup completed"
                );
            }
        }
        Err(_) => warn!("History file cache cleanup failed"),
    }

    match maintenance.enforce_retention_policy().await {
        Ok(result) => {
            if result.entries_deleted > 0 || result.errors > 0 {
                info!(
                    entries_deleted = result.entries_deleted,
                    errors = result.errors,
                    "History retention policy enforcement completed"
                );
            }
        }
        Err(_) => warn!("History retention policy enforcement failed"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct FakeHistoryMaintenance {
        calls: Mutex<Vec<&'static str>>,
        reconcile_fails: bool,
        cleanup_fails: bool,
        retention_fails: bool,
    }

    impl FakeHistoryMaintenance {
        fn new(reconcile_fails: bool, cleanup_fails: bool, retention_fails: bool) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                reconcile_fails,
                cleanup_fails,
                retention_fails,
            }
        }

        fn calls(&self) -> Vec<&'static str> {
            self.calls.lock().expect("calls lock").clone()
        }

        fn record(&self, call: &'static str) {
            self.calls.lock().expect("calls lock").push(call);
        }
    }

    #[async_trait]
    impl HistoryMaintenance for FakeHistoryMaintenance {
        async fn reconcile_missing_files(
            &self,
        ) -> Result<ReconcileResultView, ClipboardHistoryError> {
            self.record("reconcile");
            if self.reconcile_fails {
                Err(ClipboardHistoryError::Internal("probe".to_string()))
            } else {
                Ok(ReconcileResultView::default())
            }
        }

        async fn cleanup_expired_files(&self) -> Result<CleanupResultView, ClipboardHistoryError> {
            self.record("cleanup");
            if self.cleanup_fails {
                Err(ClipboardHistoryError::Internal("probe".to_string()))
            } else {
                Ok(CleanupResultView::default())
            }
        }

        async fn enforce_retention_policy(
            &self,
        ) -> Result<RetentionEnforcementResultView, ClipboardHistoryError> {
            self.record("retention");
            if self.retention_fails {
                Err(ClipboardHistoryError::Internal("probe".to_string()))
            } else {
                Ok(RetentionEnforcementResultView::default())
            }
        }
    }

    #[tokio::test]
    async fn reconciliation_failure_stops_delete_capable_passes() {
        let maintenance = FakeHistoryMaintenance::new(true, false, false);

        run_history_maintenance_once(&maintenance).await;

        assert_eq!(maintenance.calls(), vec!["reconcile"]);
    }

    #[tokio::test]
    async fn later_pass_failure_does_not_change_the_fixed_order() {
        let maintenance = FakeHistoryMaintenance::new(false, true, true);

        run_history_maintenance_once(&maintenance).await;

        assert_eq!(
            maintenance.calls(),
            vec!["reconcile", "cleanup", "retention"]
        );
    }
}
