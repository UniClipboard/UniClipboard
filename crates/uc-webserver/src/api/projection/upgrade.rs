//! Upgrade boundary projection: `UpgradeStatus` onto the wire DTO.
//!
//! Context-dependent (needs the running build version for the
//! `FreshInstall` / `NoChange` variants), so this is an owned mapper function
//! rather than an `IntoApiDto` impl.

use uc_application::facade::UpgradeStatus;
use uc_daemon_contract::api::dto::upgrade::UpgradeStatusDto;

pub(crate) fn upgrade_status_to_dto(
    status: UpgradeStatus,
    current_version: &str,
) -> UpgradeStatusDto {
    match status {
        UpgradeStatus::FreshInstall => UpgradeStatusDto::FreshInstall {
            current: current_version.to_string(),
        },
        UpgradeStatus::NoChange => UpgradeStatusDto::NoChange {
            current: current_version.to_string(),
        },
        UpgradeStatus::Upgraded { from, to } => UpgradeStatusDto::Upgraded {
            from: from.map(|v| v.to_string()),
            to: to.to_string(),
        },
        UpgradeStatus::Downgraded { from, to } => UpgradeStatusDto::Downgraded {
            from: from.to_string(),
            to: to.to_string(),
        },
    }
}
