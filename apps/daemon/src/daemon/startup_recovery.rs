//! Engine-owned daemon startup recovery.

use std::sync::Arc;

use tracing::{info_span, Instrument};
use uc_daemon_local::process_metadata::DaemonSpawnOrigin;
use uc_daemon_local::spawn_contract::unattended_from_env;
use uc_engine::{Engine, Operation, OperationResult, RecoverSessionInput, UpgradeStatusSummary};

use super::run_mode::DaemonRunMode;

fn is_attended(
    run_mode: DaemonRunMode,
    spawn_origin: DaemonSpawnOrigin,
    strict_unattended: bool,
) -> bool {
    matches!(spawn_origin, DaemonSpawnOrigin::Gui)
        && !matches!(run_mode, DaemonRunMode::ServerHeadless)
        && !strict_unattended
}

pub fn spawn_startup_recovery(run_mode: DaemonRunMode, engine: Arc<Engine>) {
    uc_observability::spawn_supervised("daemon.startup_recovery", async move {
        let attended = is_attended(
            run_mode,
            DaemonSpawnOrigin::from_env(),
            unattended_from_env(),
        );
        let allow_secure_storage_unlock = if attended {
            match engine.execute(Operation::QuerySettings).await {
                Ok(OperationResult::Settings(settings)) => settings.security.auto_unlock_enabled,
                Ok(_) | Err(_) => false,
            }
        } else {
            true
        };

        let recovery = engine
            .execute(Operation::RecoverSession(RecoverSessionInput {
                allow_secure_storage_unlock,
            }))
            .instrument(info_span!("daemon.startup.recover_session"))
            .await;

        match recovery {
            Ok(OperationResult::SessionRecovered {
                unlocked: true,
                resumed,
            }) => tracing::info!(resumed, "background engine session recovery completed"),
            Ok(OperationResult::SessionRecovered {
                unlocked: false, ..
            }) => tracing::info!("background engine session remains locked"),
            Ok(_) => tracing::warn!("engine returned an unexpected recovery result"),
            Err(error) => tracing::warn!(
                code = error.code(),
                category = %error.category(),
                "background engine session recovery failed"
            ),
        }
    });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attended_requires_gui_origin_and_non_headless_mode() {
        assert!(is_attended(
            DaemonRunMode::Standalone,
            DaemonSpawnOrigin::Gui,
            false
        ));
        assert!(!is_attended(
            DaemonRunMode::ServerHeadless,
            DaemonSpawnOrigin::Gui,
            false
        ));
        assert!(!is_attended(
            DaemonRunMode::Standalone,
            DaemonSpawnOrigin::Cli,
            false
        ));
        assert!(!is_attended(
            DaemonRunMode::Standalone,
            DaemonSpawnOrigin::Gui,
            true
        ));
    }
}
