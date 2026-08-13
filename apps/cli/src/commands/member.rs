//! `uniclip member` — offline-first space member removal.
//!
//! `member remove <PEER-ID>` records an irreversible removal intent for the
//! peer (the same Engine path the GUI uses) and prints the Engine-owned
//! workspace convergence state; `member removal-status` queries the current space-wide
//! state. Routes through a running or freshly-spawned daemon, holding a
//! control-WS lease for the duration (same pattern as `members`).
//!
//! Human output:
//!
//! ```text
//!   Member removal
//!     phase: converging
//!     history events: 2
//!     pending local decisions: 1
//!     diverged peers: 0
//!     peers requiring update: 0
//!     digest: <convergenceDigest 或 "-">
//!     updated: 2026-08-08 12:34:56
//! ```
//!
//! JSON output: the raw `WorkspaceConvergenceDto`.

use chrono::{DateTime, Local, TimeZone};
use uc_daemon_contract::api::dto::member::{WorkspaceConvergenceDto, WorkspaceConvergencePhaseDto};

use crate::commands::app_session::connect_facade_with_lease;
use crate::exit_codes;
use crate::output;
use crate::ui;

pub async fn remove(peer_id: String, json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Member removal");
    }

    let (_lease, facade) = match connect_facade_with_lease(verbose).await {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let removal = match facade.remove_member(peer_id).await {
        Ok(removal) => removal,
        Err(err) => {
            ui::error(&format!("Failed to remove member: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    emit_removal(removal, json)
}

pub async fn removal_status(json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Member removal status");
    }

    let (_lease, facade) = match connect_facade_with_lease(verbose).await {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let convergence = match facade.workspace_convergence().await {
        Ok(convergence) => convergence,
        Err(err) => {
            ui::error(&format!("Failed to query member removal: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    emit_convergence(convergence, json)
}

fn emit_removal(removal: WorkspaceConvergenceDto, json: bool) -> i32 {
    emit_convergence(removal, json)
}

fn emit_convergence(convergence: WorkspaceConvergenceDto, json: bool) -> i32 {
    if json {
        return output::emit_json(&convergence, "workspace_convergence");
    }

    if convergence.phase == WorkspaceConvergencePhaseDto::Complete
        && convergence.pending_removal_decision_device_ids.is_empty()
        && convergence.diverged_peer_device_ids.is_empty()
        && convergence.upgrade_required_peer_device_ids.is_empty()
        && !convergence.removed
    {
        ui::info("·", "No member removal in progress.");
        return exit_codes::EXIT_SUCCESS;
    }

    render_human(&convergence);
    exit_codes::EXIT_SUCCESS
}

fn render_human(convergence: &WorkspaceConvergenceDto) {
    if convergence.removed {
        ui::warn("this device has been removed from the space; re-pair to rejoin");
    }

    let updated = Local
        .timestamp_millis_opt(convergence.updated_at_ms)
        .single()
        .map(|dt: DateTime<Local>| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "-".to_string());
    let digest = convergence.convergence_digest.as_deref().unwrap_or("-");

    ui::bar();
    let phase = serde_json::to_string(&convergence.phase).unwrap_or_else(|_| "?".to_string());
    ui::info("phase", phase.trim_matches('"'));
    ui::info(
        "history events",
        &convergence.history_event_count.to_string(),
    );
    ui::info(
        "pending local decisions",
        &convergence
            .pending_removal_decision_device_ids
            .len()
            .to_string(),
    );
    ui::info(
        "diverged peers",
        &convergence.diverged_peer_device_ids.len().to_string(),
    );
    ui::info(
        "peers requiring update",
        &convergence
            .upgrade_required_peer_device_ids
            .len()
            .to_string(),
    );
    ui::info("digest", digest);
    ui::info("updated", &updated);
    ui::bar();
}
