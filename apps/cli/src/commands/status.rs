//! Status 命令:通过 daemon 显示应用状态。

use serde::Serialize;
use uc_daemon_client::{DaemonService, HttpWsDaemonService};
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
    device_trust: DeviceTrustStatus,
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

    // Assumption: a healthy daemon implies setup_complete=true and
    // encryption unlocked (startup_recovery auto-unlocks). If the daemon
    // lifecycle ever allows starting without setup or with locked
    // encryption, this inference must be replaced with a dedicated
    // endpoint query.
    let setup_completed = true;
    let encryption_ready = true;

    let search = ctx.search_client();
    let (search_state, search_reason) = match search.status().await {
        Ok(status) => (status.state, status.reason),
        Err(err) => {
            ui::error(&format!("Failed to query search status: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let facade = HttpWsDaemonService::new(ctx);
    let device_trust = match facade.device_trust().await {
        Ok(snapshot) => DeviceTrustStatus {
            local_membership: snapshot.local_membership,
            current_change_id: snapshot.current_change.map(|change| change.change_id),
            upgrade_required_device_ids: snapshot
                .devices
                .into_iter()
                .filter(|device| device.compatibility == DeviceCompatibilityDto::UpgradeRequired)
                .map(|device| device.device_id)
                .collect(),
            blocked_reason: snapshot.blocked_reason,
        },
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
        device_trust,
    };

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
        "Device membership",
        membership_label(result.device_trust.local_membership),
    );
    ui::info(
        "Device trust change",
        result
            .device_trust
            .current_change_id
            .as_deref()
            .unwrap_or("none"),
    );
    ui::info(
        "Devices requiring update",
        &result
            .device_trust
            .upgrade_required_device_ids
            .len()
            .to_string(),
    );

    exit_codes::EXIT_SUCCESS
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
    use uc_daemon_contract::api::dto::member::DeviceMembershipDto;

    #[test]
    fn json_includes_device_trust_status() {
        let output = StatusOutput {
            setup_completed: true,
            encryption_ready: true,
            search_state: "ready".to_string(),
            search_reason: None,
            device_trust: DeviceTrustStatus {
                local_membership: DeviceMembershipDto::Active,
                current_change_id: Some("change-1".to_string()),
                upgrade_required_device_ids: vec!["device-b".to_string()],
                blocked_reason: None,
            },
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
}
