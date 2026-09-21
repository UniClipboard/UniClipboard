//! Status 命令:通过 daemon 显示应用状态。

use serde::Serialize;
use uc_daemon_client::{DaemonService, HttpWsDaemonService};
use uc_daemon_contract::api::dto::encryption::{
    AdmissionRecoveryActionDto, AdmissionRecoveryCategoryDto, AdmissionRecoveryStageDto,
    ProfileRecoveryResponse, ProfileRecoveryStateDto,
};
use uc_daemon_contract::api::dto::member::DeviceCompatibilityDto;
use uc_daemon_contract::api::dto::member::{DeviceMembershipDto, DeviceTrustUnavailableReasonDto};

use crate::commands::app_session::connect_with_lease;
use crate::exit_codes;
use crate::output;
use crate::ui;

#[derive(Serialize)]
struct StatusOutput {
    setup_completed: bool,
    encryption_ready: bool,
    search_state: String,
    search_reason: Option<String>,
    profile_recovery: ProfileRecoveryResponse,
    device_trust: Option<DeviceTrustStatus>,
}

#[derive(Serialize)]
struct DeviceTrustStatus {
    local_membership: DeviceMembershipDto,
    current_change_id: Option<String>,
    upgrade_required_device_ids: Vec<String>,
    blocked_reason: Option<DeviceTrustUnavailableReasonDto>,
}

pub async fn run(json: bool, verbose: bool) -> i32 {
    let (_lease, ctx) = match connect_with_lease(verbose).await {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let query = ctx.query_client();
    let profile_recovery = match query.get_profile_recovery().await {
        Ok(status) => status,
        Err(err) => {
            ui::error(&format!("Failed to query profile recovery status: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };
    let encryption_ready = match query.get_encryption_state().await {
        Ok(status) => status.session_ready,
        Err(err) => {
            ui::error(&format!("Failed to query encryption status: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let setup_completed = true;

    if !profile_recovery.background_ready {
        let result = StatusOutput {
            setup_completed,
            encryption_ready,
            search_state: "unavailable".to_string(),
            search_reason: Some("profile_recovery_required".to_string()),
            profile_recovery,
            device_trust: None,
        };
        return emit_status(result, json);
    }

    let search = ctx.search_client();
    let (search_state, search_reason) = match search.status().await {
        Ok(status) => (status.state, status.reason),
        Err(err) => {
            ui::error(&format!("Failed to query search status: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let facade = HttpWsDaemonService::new(ctx);
    let device_trust = match facade.query_device_group_choices().await {
        Ok(choices) => Some(DeviceTrustStatus {
            local_membership: choices.device_trust.local_membership,
            current_change_id: choices
                .device_trust
                .current_change
                .map(|change| change.change_id),
            upgrade_required_device_ids: choices
                .device_trust
                .devices
                .into_iter()
                .filter(|device| device.compatibility == DeviceCompatibilityDto::UpgradeRequired)
                .map(|device| device.device_id)
                .collect(),
            blocked_reason: choices.device_trust.blocked_reason,
        }),
        Err(err) => {
            ui::error(&format!("Failed to query device trust status: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let result = StatusOutput {
        setup_completed,
        encryption_ready,
        search_state,
        search_reason,
        profile_recovery,
        device_trust,
    };

    emit_status(result, json)
}

fn emit_status(result: StatusOutput, json: bool) -> i32 {
    if json {
        return output::emit_json(&result, "status response");
    }

    ui::info(
        "Setup completed",
        if result.setup_completed { "yes" } else { "no" },
    );
    ui::info(
        "Encryption ready",
        if result.encryption_ready { "yes" } else { "no" },
    );
    ui::info("Search state", &result.search_state);
    ui::info(
        "Search reason",
        result.search_reason.as_deref().unwrap_or("none"),
    );
    ui::info(
        "Profile recovery",
        recovery_state_label(&result.profile_recovery.state),
    );
    if let Some(admission) = &result.profile_recovery.admission {
        ui::info(
            "Recovery category",
            admission_category_label(&admission.category),
        );
        ui::info("Recovery stage", admission_stage_label(&admission.stage));
        ui::info(
            "Recommended action",
            admission_action_label(&admission.action),
        );
        ui::warn("Existing data has not been deleted.");
        ui::info("Diagnostics", "run `uniclip debug export-logs`");
    }
    if let Some(device_trust) = &result.device_trust {
        ui::info(
            "Device membership",
            membership_label(device_trust.local_membership.clone()),
        );
        ui::info(
            "Device trust change",
            device_trust.current_change_id.as_deref().unwrap_or("none"),
        );
        ui::info(
            "Devices requiring update",
            &device_trust.upgrade_required_device_ids.len().to_string(),
        );
    }

    exit_codes::EXIT_SUCCESS
}

fn recovery_state_label(state: &ProfileRecoveryStateDto) -> &'static str {
    match state {
        ProfileRecoveryStateDto::NotRequired => "not required",
        ProfileRecoveryStateDto::AwaitingPassphrase => "awaiting passphrase",
        ProfileRecoveryStateDto::Recovering => "recovering",
        ProfileRecoveryStateDto::Recovered => "recovered",
        ProfileRecoveryStateDto::PartiallyRecoverable => "partially recoverable",
        ProfileRecoveryStateDto::Failed => "failed",
        ProfileRecoveryStateDto::AdmissionRecoveryRequired => "admission recovery required",
    }
}

fn admission_category_label(category: &AdmissionRecoveryCategoryDto) -> &'static str {
    match category {
        AdmissionRecoveryCategoryDto::CredentialMissing => "credential missing",
        AdmissionRecoveryCategoryDto::AuthenticationMismatch => "authentication mismatch",
        AdmissionRecoveryCategoryDto::CurrentMetadataInvalid => "current metadata invalid",
        AdmissionRecoveryCategoryDto::LegacyFallbackInvalid => "legacy fallback invalid",
        AdmissionRecoveryCategoryDto::LegacyMigrationFailed => "legacy migration failed",
        AdmissionRecoveryCategoryDto::RecordRelationIncomplete => "record relation incomplete",
        AdmissionRecoveryCategoryDto::DerivedSummaryInvalid => "derived summary invalid",
        AdmissionRecoveryCategoryDto::GenerationMismatch => "generation mismatch",
        AdmissionRecoveryCategoryDto::OtherStorageError => "storage error",
    }
}

fn admission_stage_label(stage: &AdmissionRecoveryStageDto) -> &'static str {
    match stage {
        AdmissionRecoveryStageDto::Credential => "credential",
        AdmissionRecoveryStageDto::RepositoryMetadata => "repository metadata",
        AdmissionRecoveryStageDto::LegacyRepository => "legacy repository",
        AdmissionRecoveryStageDto::RepositoryRecord => "repository record",
        AdmissionRecoveryStageDto::RecoverySummary => "recovery summary",
        AdmissionRecoveryStageDto::Storage => "storage",
    }
}

fn admission_action_label(action: &AdmissionRecoveryActionDto) -> &'static str {
    match action {
        AdmissionRecoveryActionDto::RestoreCredential => "restore credential from a trusted copy",
        AdmissionRecoveryActionDto::ChooseBackup => "choose a known-good backup",
        AdmissionRecoveryActionDto::RebuildDerivedState => "rebuild derived state",
        AdmissionRecoveryActionDto::ExportDiagnostics => "export diagnostics",
    }
}

fn membership_label(state: DeviceMembershipDto) -> &'static str {
    match state {
        DeviceMembershipDto::Active => "active",
        DeviceMembershipDto::Removed => "removed",
        DeviceMembershipDto::Unavailable => "unavailable",
        DeviceMembershipDto::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::{DeviceTrustStatus, StatusOutput};
    use serde_json::json;
    use uc_daemon_contract::api::dto::encryption::{
        AdmissionRecoveryActionDto, AdmissionRecoveryCategoryDto, AdmissionRecoveryDto,
        AdmissionRecoveryStageDto, ProfileRecoveryResponse, ProfileRecoveryStateDto,
    };
    use uc_daemon_contract::api::dto::member::DeviceMembershipDto;

    #[test]
    fn json_includes_device_trust_status() {
        let output = StatusOutput {
            setup_completed: true,
            encryption_ready: true,
            search_state: "ready".to_string(),
            search_reason: None,
            profile_recovery: ProfileRecoveryResponse {
                state: ProfileRecoveryStateDto::NotRequired,
                can_submit_passphrase: false,
                restart_required: false,
                background_ready: true,
                cleanup_pending: false,
                losses: Vec::new(),
                admission: None,
            },
            device_trust: Some(DeviceTrustStatus {
                local_membership: DeviceMembershipDto::Active,
                current_change_id: Some("change-1".to_string()),
                upgrade_required_device_ids: vec!["device-b".to_string()],
                blocked_reason: None,
            }),
        };

        let value = serde_json::to_value(output).expect("serialize status output");
        assert_eq!(value["device_trust"]["local_membership"], json!("active"));
        assert_eq!(
            value["device_trust"]["current_change_id"],
            json!("change-1")
        );
        assert_eq!(
            value["device_trust"]["upgrade_required_device_ids"],
            json!(["device-b"])
        );
    }

    #[test]
    fn json_includes_classified_admission_recovery() {
        let output = StatusOutput {
            setup_completed: true,
            encryption_ready: false,
            search_state: "unavailable".to_string(),
            search_reason: Some("profile_recovery_required".to_string()),
            profile_recovery: ProfileRecoveryResponse {
                state: ProfileRecoveryStateDto::AdmissionRecoveryRequired,
                can_submit_passphrase: false,
                restart_required: false,
                background_ready: false,
                cleanup_pending: false,
                losses: Vec::new(),
                admission: Some(AdmissionRecoveryDto {
                    category: AdmissionRecoveryCategoryDto::LegacyFallbackInvalid,
                    stage: AdmissionRecoveryStageDto::LegacyRepository,
                    action: AdmissionRecoveryActionDto::ChooseBackup,
                }),
            },
            device_trust: None,
        };

        let value = serde_json::to_value(output).expect("serialize status output");
        assert_eq!(
            value["profile_recovery"]["state"],
            json!("admission_recovery_required")
        );
        assert_eq!(
            value["profile_recovery"]["admission"],
            json!({
                "category": "legacy_fallback_invalid",
                "stage": "legacy_repository",
                "action": "choose_backup"
            })
        );
        assert!(value["device_trust"].is_null());
    }
}
