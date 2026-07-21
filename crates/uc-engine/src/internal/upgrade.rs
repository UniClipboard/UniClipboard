use uc_application::facade::{AppFacade, UpgradeStatus};

use crate::{EngineError, EngineErrorCategory, OperationResult, UpgradeStatusSummary};

pub const QUERY_UPGRADE_STATUS_FAILED_CODE: u32 = 1401;
pub const ACKNOWLEDGE_UPGRADE_FAILED_CODE: u32 = 1402;

pub(crate) async fn execute_query_upgrade_status(
    facade: &AppFacade,
    app_version: &str,
) -> Result<OperationResult, EngineError> {
    let status = facade
        .upgrade
        .detect_on_startup(app_version)
        .await
        .map_err(|_| internal_error(QUERY_UPGRADE_STATUS_FAILED_CODE))?;
    Ok(OperationResult::UpgradeStatus(map_status(
        status,
        app_version,
    )))
}

pub(crate) async fn execute_acknowledge_upgrade(
    facade: &AppFacade,
    app_version: &str,
) -> Result<OperationResult, EngineError> {
    facade
        .upgrade
        .acknowledge(app_version)
        .await
        .map_err(|_| internal_error(ACKNOWLEDGE_UPGRADE_FAILED_CODE))?;
    Ok(OperationResult::UpgradeAcknowledged {
        version: app_version.to_string(),
    })
}

fn map_status(status: UpgradeStatus, app_version: &str) -> UpgradeStatusSummary {
    match status {
        UpgradeStatus::FreshInstall => UpgradeStatusSummary::FreshInstall {
            current: app_version.to_string(),
        },
        UpgradeStatus::NoChange => UpgradeStatusSummary::NoChange {
            current: app_version.to_string(),
        },
        UpgradeStatus::Upgraded { from, to } => UpgradeStatusSummary::Upgraded {
            from: from.map(|version| version.to_string()),
            to: to.to_string(),
        },
        UpgradeStatus::Downgraded { from, to } => UpgradeStatusSummary::Downgraded {
            from: from.to_string(),
            to: to.to_string(),
        },
    }
}

fn internal_error(code: u32) -> EngineError {
    EngineError::new(code, EngineErrorCategory::Internal, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping_uses_the_engine_version_for_terminal_states() {
        assert_eq!(
            map_status(UpgradeStatus::FreshInstall, "1.2.3"),
            UpgradeStatusSummary::FreshInstall {
                current: "1.2.3".into(),
            }
        );
        assert_eq!(
            map_status(UpgradeStatus::NoChange, "1.2.3"),
            UpgradeStatusSummary::NoChange {
                current: "1.2.3".into(),
            }
        );
    }
}
