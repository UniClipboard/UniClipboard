//! `uniclip members` — list paired devices + presence (ADR-008 P5-2a).
//!
//! Routes through a running or freshly-spawned daemon (ADR-008 P5-2a).
//! Holds a control-WS lease to keep a transient Oneshot daemon alive for
//! the duration of the query sequence, then lets the daemon self-terminate
//! via its idle timer.
//!
//! Human output:
//!
//! ```text
//!   laptop (online) [local]
//!   phone (offline)
//!   workstation (unknown)
//! ```
//!
//! JSON output: array of `{device_id, device_name, is_local, state}`.

use serde::Serialize;
use uc_core::ports::ReachabilityState;
use uc_daemon_client::DaemonClientContext;

use crate::commands::app_session::connect_or_spawn_oneshot_daemon;
use crate::exit_codes;
use crate::ui;

pub async fn run(json: bool, verbose: bool) -> i32 {
    ui::header("Members");

    let service = match connect_or_spawn_oneshot_daemon(verbose).await {
        Ok(s) => s,
        Err(code) => return code,
    };

    let _lease = match service.hold_control_lease().await {
        Ok(guard) => guard,
        Err(err) => {
            ui::error(&format!("Failed to hold daemon session lease: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let ctx = match DaemonClientContext::from_env() {
        Ok(ctx) => ctx,
        Err(err) => {
            ui::error(&format!("Failed to connect to daemon: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };
    let query = ctx.query_client();

    // Probe presence so state is fresh before listing.
    let probe_spinner = ui::spinner("Probing paired peers...");
    match query.refresh_presence().await {
        Ok(report) => {
            ui::spinner_finish_success(
                &probe_spinner,
                &format!(
                    "Probed {} peer(s): {} online, {} offline, {} error(s)",
                    report.total, report.online, report.offline, report.errors
                ),
            );
        }
        Err(err) => {
            ui::spinner_finish_error(
                &probe_spinner,
                &format!("Probe round failed: {err} (showing last-known state)"),
            );
        }
    }

    // Fetch remote members from the daemon.
    let remote_members = match query.get_paired_devices().await {
        Ok(members) => members,
        Err(err) => {
            ui::error(&format!("Failed to list paired devices: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    // Fetch local device info (non-fatal if unavailable).
    let local_device = match query.get_local_device_info().await {
        Ok(info) => Some(info),
        Err(err) => {
            tracing::debug!(error = %err, "could not fetch local device info");
            None
        }
    };

    // Build combined entries: local device first, then remote members.
    let mut entries: Vec<MemberDto> = Vec::with_capacity(1 + remote_members.len());

    if let Some(local) = local_device {
        entries.push(MemberDto {
            device_id: local.peer_id,
            device_name: local.device_name,
            is_local: true,
            state: format_state(ReachabilityState::Online),
        });
    }

    for member in &remote_members {
        let state = match member.channel.as_str() {
            "direct" | "relay" => ReachabilityState::Online,
            "offline" => ReachabilityState::Offline,
            _ => ReachabilityState::Unknown,
        };
        entries.push(MemberDto {
            device_id: member.peer_id.clone(),
            device_name: member.device_name.clone(),
            is_local: false,
            state: format_state(state),
        });
    }

    if json {
        match serde_json::to_string_pretty(&entries) {
            Ok(json_str) => println!("{json_str}"),
            Err(err) => {
                ui::error(&format!("Failed to serialize roster: {err}"));
                return exit_codes::EXIT_ERROR;
            }
        }
    } else {
        render_human(&entries);
    }

    exit_codes::EXIT_SUCCESS
}

fn render_human(entries: &[MemberDto]) {
    ui::bar();
    if entries.is_empty() {
        ui::info("members", "(none)");
    } else {
        for entry in entries {
            let local_tag = if entry.is_local { " [local]" } else { "" };
            let line = format!("{} ({}){}", entry.device_name, entry.state, local_tag);
            ui::info("·", &line);
        }
    }
    ui::bar();
}

fn format_state(state: ReachabilityState) -> &'static str {
    match state {
        ReachabilityState::Online => "online",
        ReachabilityState::Offline => "offline",
        ReachabilityState::Unknown => "unknown",
    }
}

#[derive(Serialize)]
struct MemberDto {
    device_id: String,
    device_name: String,
    is_local: bool,
    state: &'static str,
}
