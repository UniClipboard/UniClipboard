//! Upgrade boundary projection onto the wire DTO.

use uc_daemon_contract::api::dto::upgrade::UpgradeStatusDto;
use uc_engine::UpgradeStatusSummary;

pub(crate) fn upgrade_status_to_dto(status: UpgradeStatusSummary) -> UpgradeStatusDto {
    match status {
        UpgradeStatusSummary::FreshInstall { current } => {
            UpgradeStatusDto::FreshInstall { current }
        }
        UpgradeStatusSummary::NoChange { current } => UpgradeStatusDto::NoChange { current },
        UpgradeStatusSummary::Upgraded { from, to } => {
            UpgradeStatusDto::Upgraded { from, to }
        }
        UpgradeStatusSummary::Downgraded { from, to } => {
            UpgradeStatusDto::Downgraded { from, to }
        }
    }
}
