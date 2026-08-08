//! Hidden CLI driver for the Engine-owned shared-device refresh round.
//!
//! Talks to the running daemon through the same member API as the GUI device
//! page (`POST` / `GET /member/shared-device-refresh`). Development and E2E
//! testing only; not part of the public command surface.

use serde::Serialize;
use uc_daemon_client::DaemonRequestError;
use uc_daemon_contract::api::dto::member::{
    SharedDeviceRefreshDeviceStateDto, SharedDeviceRefreshPhaseDto,
};

use crate::commands::app_session::connect_with_lease;
use crate::{exit_codes, output, ui};

/// JSON error view for a query whose refresh round no longer exists.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NotFoundError {
    error: &'static str,
    request_id: String,
}

fn state_label(state: &SharedDeviceRefreshDeviceStateDto) -> &'static str {
    match state {
        SharedDeviceRefreshDeviceStateDto::Discovered => "discovered",
        SharedDeviceRefreshDeviceStateDto::Connecting => "connecting",
        SharedDeviceRefreshDeviceStateDto::Connected => "connected",
        SharedDeviceRefreshDeviceStateDto::AlreadyPresent => "already_present",
        SharedDeviceRefreshDeviceStateDto::WaitingForPeer => "waiting_for_peer",
        SharedDeviceRefreshDeviceStateDto::WaitingForUpdate => "waiting_for_update",
        SharedDeviceRefreshDeviceStateDto::VersionIncompatible => "version_incompatible",
        SharedDeviceRefreshDeviceStateDto::Rejected => "rejected",
    }
}

fn phase_label(phase: &SharedDeviceRefreshPhaseDto) -> &'static str {
    match phase {
        SharedDeviceRefreshPhaseDto::Started => "started",
        SharedDeviceRefreshPhaseDto::Discovering => "discovering",
        SharedDeviceRefreshPhaseDto::Connecting => "connecting",
        SharedDeviceRefreshPhaseDto::RoundCompleted => "round_completed",
    }
}

pub async fn run_start(json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Start shared-device refresh");
    }

    let (_lease, ctx) = match connect_with_lease(verbose).await {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let member = ctx.member_client();

    match member.start_shared_device_refresh().await {
        Ok(started) => {
            if json {
                return output::emit_json(&started, "shared-device-refresh start");
            }
            ui::info("request id", &started.request_id);
            exit_codes::EXIT_SUCCESS
        }
        Err(err) => {
            ui::error(&format!("Failed to start shared-device refresh: {err}"));
            exit_codes::EXIT_ERROR
        }
    }
}

pub async fn run_status(request_id: &str, json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Shared-device refresh status");
    }

    let (_lease, ctx) = match connect_with_lease(verbose).await {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let member = ctx.member_client();

    match member.shared_device_refresh(request_id).await {
        Ok(snapshot) => {
            if json {
                return output::emit_json(&snapshot, "shared-device-refresh status");
            }
            ui::info("request", &snapshot.request_id);
            ui::info("phase", phase_label(&snapshot.phase));
            for device in &snapshot.devices {
                ui::info(
                    "device",
                    &format!("{} ({})", device.display_name, state_label(&device.state)),
                );
            }
            exit_codes::EXIT_SUCCESS
        }
        Err(err) => {
            let not_found = err
                .downcast_ref::<DaemonRequestError>()
                .is_some_and(|error| error.code() == Some("shared_device_refresh_not_found"));
            if not_found {
                if json {
                    let error = NotFoundError {
                        error: "shared_device_refresh_not_found",
                        request_id: request_id.to_string(),
                    };
                    output::emit_json(&error, "shared-device-refresh status error");
                } else {
                    ui::error(&format!(
                        "Shared-device-refresh request {request_id} no longer exists"
                    ));
                }
                return exit_codes::EXIT_ERROR;
            }
            ui::error(&format!("Failed to query shared-device refresh: {err}"));
            exit_codes::EXIT_ERROR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::phase_label;
    use uc_daemon_contract::api::dto::member::SharedDeviceRefreshPhaseDto;

    #[test]
    fn phase_labels_match_the_api_contract() {
        assert_eq!(
            phase_label(&SharedDeviceRefreshPhaseDto::Started),
            "started"
        );
        assert_eq!(
            phase_label(&SharedDeviceRefreshPhaseDto::Discovering),
            "discovering"
        );
        assert_eq!(
            phase_label(&SharedDeviceRefreshPhaseDto::Connecting),
            "connecting"
        );
        assert_eq!(
            phase_label(&SharedDeviceRefreshPhaseDto::RoundCompleted),
            "round_completed"
        );
    }
}
