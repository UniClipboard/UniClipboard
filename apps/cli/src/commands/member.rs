//! Offline-first space member removal.

use crate::commands::app_session::connect_facade_with_lease;
use crate::exit_codes;
use crate::{output, ui};

pub async fn remove(peer_id: String, json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Member removal");
    }

    let (_lease, service) = match connect_facade_with_lease(verbose).await {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let device_trust = match service.remove_member(peer_id).await {
        Ok(device_trust) => device_trust,
        Err(error) => {
            ui::error(&format!(
                "Failed to remove member: {}",
                crate::commands::daemon_error_message(&error)
            ));
            return exit_codes::EXIT_ERROR;
        }
    };

    if json {
        output::emit_json(&device_trust, "device trust")
    } else {
        ui::success("Member removal recorded.");
        ui::info("revision", &device_trust.revision.to_string());
        exit_codes::EXIT_SUCCESS
    }
}
