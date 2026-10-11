//! Engine-owned daemon startup recovery.

use std::sync::Arc;

use tracing::{info_span, Instrument};
use uc_engine::{Engine, Operation, OperationResult, RecoverSessionInput, UpgradeStatusSummary};
use uc_platform::secure_storage::SecureStorageSource;

use super::run_mode::DaemonRunMode;

pub fn spawn_startup_recovery(
    _run_mode: DaemonRunMode,
    engine: Arc<Engine>,
    secure_storage_source: SecureStorageSource,
) {
    let recovery = tokio::spawn(
        async move {
            if let Ok(OperationResult::ProfileRecovery(summary)) =
                engine.execute(Operation::QueryProfileRecovery).await
            {
                if !summary.background_ready {
                    tracing::warn!(
                        source = "uc_engine",
                        state = ?summary.state,
                        background_ready = summary.background_ready,
                        can_submit_passphrase = summary.can_submit_passphrase,
                        restart_required = summary.restart_required,
                        admission_category = ?summary.admission.map(|admission| admission.category),
                        admission_stage = ?summary.admission.map(|admission| admission.stage),
                        recovery_action = ?summary.admission.map(|admission| admission.action),
                        "profile recovery required before background services can start"
                    );
                }
            }
            let recovery = engine
                .execute(Operation::RecoverSession(RecoverSessionInput {
                    allow_secure_storage_unlock: true,
                }))
                .instrument(info_span!("daemon.startup.recover_session"))
                .await;

            match recovery {
                Ok(OperationResult::SessionRecovered {
                    unlocked: true,
                    resumed,
                }) => {
                    // The Engine opened the profile with the stored key, which
                    // proves the selected key store holds this profile's keys.
                    secure_storage_source.confirm_unlocked_by_stored_key();
                    tracing::info!(resumed, "background engine session recovery completed")
                }
                Ok(OperationResult::SessionRecovered {
                    unlocked: false, ..
                }) => tracing::info!("background engine session remains locked"),
                Ok(_) => tracing::warn!("engine returned an unexpected recovery result"),
                Err(error) => tracing::warn!(
                    error_code = error.code(),
                    error_kind = %error.category(),
                    source = "uc_engine",
                    retryable = error.is_retryable(),
                    "background engine session recovery failed"
                ),
            }
        }
        .instrument(info_span!(
            "daemon.startup.recovery",
            trace_id = %format!("startup-{}-{}", std::process::id(), chrono::Utc::now().timestamp_millis())
        )),
    );
    tokio::spawn(async move {
        if let Err(error) = recovery.await {
            tracing::error!(error = %error, error_kind = "startup_recovery_task_failed", "startup recovery task failed");
        }
    }.in_current_span());
}

pub(crate) async fn record_upgrade_status_at_startup(engine: &Engine) {
    let status = match engine.execute(Operation::QueryUpgradeStatus).await {
        Ok(OperationResult::UpgradeStatus(status)) => status,
        Ok(_) => {
            tracing::warn!(target: "upgrade", "engine returned an unexpected upgrade result");
            return;
        }
        Err(error) => {
            tracing::warn!(
                target: "upgrade",
                code = error.code(),
                category = %error.category(),
                "startup upgrade detection failed"
            );
            return;
        }
    };

    let acknowledge = match &status {
        UpgradeStatusSummary::FreshInstall { current } => {
            tracing::info!(target: "upgrade", current, "fresh install detected");
            true
        }
        UpgradeStatusSummary::NoChange { current } => {
            tracing::info!(target: "upgrade", current, "version cursor matches current build");
            false
        }
        UpgradeStatusSummary::Upgraded { from, to } => {
            tracing::info!(
                target: "upgrade",
                has_previous_version = from.is_some(),
                to,
                "upgrade detected"
            );
            from.is_some()
        }
        UpgradeStatusSummary::Downgraded { from, to } => {
            tracing::info!(target: "upgrade", from, to, "downgrade detected");
            false
        }
    };

    if acknowledge {
        match engine.execute(Operation::AcknowledgeUpgrade).await {
            Ok(OperationResult::UpgradeAcknowledged { .. }) => {}
            Ok(_) => tracing::warn!(
                target: "upgrade",
                "engine returned an unexpected upgrade acknowledgement"
            ),
            Err(error) => tracing::warn!(
                target: "upgrade",
                code = error.code(),
                category = %error.category(),
                "upgrade acknowledgement failed"
            ),
        }
    }
}
