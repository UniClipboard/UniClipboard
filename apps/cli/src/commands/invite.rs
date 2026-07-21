//! `uniclip invite` — sponsor side of Slice 1 pairing.
//!
//! ## Execution paths
//!
//! * **`run(verbose)`** — daemon path (ADR-008 P5-2b).
//!   Connects to (or spawns) the daemon, subscribes to
//!   `setup.pairingCompleted` over WS, calls
//!   `POST /v2/setup/issue-invitation`, and blocks until
//!   an outcome arrives or Ctrl+C.
//!
//! * **`run_for_address(ip, verbose)`** — unified engine path (dev-only).
//!   Called from `dev pairing issue --addr <ip>`.

use tokio::select;
use tokio::signal;

#[cfg(feature = "dev-tools")]
use std::net::IpAddr;

// --- daemon path imports (P5-2b) -------------------------------------------
use crate::commands::app_session::connect_or_spawn_oneshot_daemon;
use uc_daemon_client::DaemonClientContext;

// --- engine path imports (debug builds only) ---------------------------------
#[cfg(feature = "dev-tools")]
use uc_engine::{DevOperation, DevOperationResult, DevPairingOutcome};

#[cfg(feature = "dev-tools")]
use crate::commands::app_session::{build_app_session, refuse_if_daemon_running};
use crate::exit_codes;
use crate::ui;

const EXIT_SIGINT: i32 = 130;

// ---------------------------------------------------------------------------
// Public entry: daemon path (ADR-008 P5-2b)
// ---------------------------------------------------------------------------

pub async fn run(verbose: bool) -> i32 {
    ui::header("Invite a device");

    let service = match connect_or_spawn_oneshot_daemon(verbose).await {
        Ok(s) => s,
        Err(code) => return code,
    };

    // Subscribe BEFORE issuing so we never miss an outcome that races
    // between POST returning and the WS delivering the event.
    let mut rx = match service.subscribe_setup_pairing_completion().await {
        Ok(rx) => rx,
        Err(err) => {
            ui::error(&format!("Failed to subscribe pairing completion: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let ctx = match DaemonClientContext::from_env() {
        Ok(ctx) => ctx,
        Err(err) => {
            ui::error(&format!("Failed to build daemon client context: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let spinner = ui::spinner("Requesting invitation from rendezvous...");
    let invitation = match ctx.setup_v2_client().issue_invitation().await {
        Ok(inv) => {
            ui::spinner_finish_success(&spinner, "Invitation issued");
            inv
        }
        Err(err) => {
            ui::spinner_finish_error(&spinner, &crate::commands::daemon_error_message(&err));
            return exit_codes::EXIT_ERROR;
        }
    };

    ui::bar();
    ui::verification_code(&invitation.code);

    // Convert epoch-ms to a human-readable UTC timestamp via chrono.
    if let Some(dt) = chrono::DateTime::from_timestamp_millis(invitation.expires_at_ms) {
        ui::info("expires_at", &dt.to_rfc3339());
    } else {
        ui::info("expires_at", &format!("{}ms", invitation.expires_at_ms));
    }

    ui::bar();

    // Machine-readable line on stdout so scripts (e.g., the single-
    // machine e2e test) can capture the code without parsing ANSI from
    // the styled stderr output. Humans see the styled version above.
    // Explicit flush because Rust stdout is fully-buffered when piped.
    {
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "INVITATION_CODE={}", invitation.code);
        let _ = out.flush();
    }

    let waiting = ui::spinner("Waiting for joiner to complete handshake (Ctrl+C to cancel)...");

    select! {
        outcome = rx.recv() => match outcome {
            Some(event) if event.success => {
                ui::spinner_finish_success(&waiting, "Pairing completed");
                ui::info("sponsor_device_id", &event.sponsor_device_id);
                if let Some(ref joiner_id) = event.joiner_device_id {
                    ui::info("joiner_device_id", joiner_id);
                }
                exit_codes::EXIT_SUCCESS
            }
            Some(event) => {
                let reason = event.reason.as_deref().unwrap_or("unknown");
                ui::spinner_finish_error(
                    &waiting,
                    &format!("Pairing failed: {reason}"),
                );
                exit_codes::EXIT_ERROR
            }
            None => {
                ui::spinner_finish_error(&waiting, "Outcome stream ended unexpectedly");
                exit_codes::EXIT_ERROR
            }
        },
        _ = signal::ctrl_c() => {
            ui::spinner_finish_error(&waiting, "Interrupted by user");
            EXIT_SIGINT
        }
    }
}

// ---------------------------------------------------------------------------
// Dev-only entry: in-process path (debug builds only)
// ---------------------------------------------------------------------------

#[cfg(feature = "dev-tools")]
pub(crate) async fn run_for_address(selected_ip: IpAddr, verbose: bool) -> i32 {
    run_for_address_inner(selected_ip, verbose).await
}

#[cfg(feature = "dev-tools")]
async fn run_for_address_inner(selected_ip: IpAddr, verbose: bool) -> i32 {
    ui::header(&format!("Invite a device via {selected_ip}"));

    if let Err(code) = refuse_if_daemon_running().await {
        return code;
    }

    let cli = match build_app_session(verbose).await {
        Ok(bundle) => bundle,
        Err(code) => return code,
    };

    let resume_spinner = ui::spinner("Resuming space session...");
    match cli.recover_session().await {
        Ok(true) => {
            ui::spinner_finish_success(&resume_spinner, "Session resumed");
        }
        Ok(false) => {
            ui::spinner_finish_error(
                &resume_spinner,
                "No space on this profile — run `init` first.",
            );
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(error) => {
            ui::spinner_finish_error(&resume_spinner, &format!("Resume failed: {error}"));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    }

    // Subscribe BEFORE issuing so we never miss an outcome that would
    // race between B1 returning and this task subscribing.
    let mut outcome_rx = match cli.engine().dev_pairing_outcomes().await {
        Ok(rx) => rx,
        Err(err) => {
            ui::error(&format!("Failed to subscribe pairing completion: {err}"));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    };

    let spinner = ui::spinner("Requesting invitation from rendezvous...");
    let invitation = match cli
        .engine()
        .execute_dev(DevOperation::IssueInvitationForAddress {
            address: selected_ip,
        })
        .await
    {
        Ok(DevOperationResult::InvitationIssued(invitation)) => {
            ui::spinner_finish_success(&spinner, "Invitation issued");
            invitation
        }
        Ok(_) => {
            ui::spinner_finish_error(&spinner, "Unexpected engine response");
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(error) => {
            ui::spinner_finish_error(&spinner, &format!("{error}"));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    };

    ui::bar();
    ui::verification_code(&invitation.code);
    if let Some(expires_at) = chrono::DateTime::from_timestamp_millis(invitation.expires_at_ms) {
        ui::info("expires_at", &expires_at.to_rfc3339());
    }
    ui::bar();

    // Machine-readable line on stdout so scripts can capture the code.
    {
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "INVITATION_CODE={}", invitation.code);
        let _ = out.flush();
    }

    let waiting = ui::spinner("Waiting for joiner to complete handshake (Ctrl+C to cancel)...");

    let exit = select! {
        outcome = outcome_rx.recv() => match outcome {
            Ok(DevPairingOutcome::Success {
                peer_device_id,
                peer_device_name,
                peer_fingerprint,
            }) => {
                ui::spinner_finish_success(&waiting, "Pairing completed");
                ui::info("peer_device_id", &peer_device_id);
                ui::info("peer_device_name", &peer_device_name);
                ui::info("peer_fingerprint", &peer_fingerprint);
                exit_codes::EXIT_SUCCESS
            }
            Ok(DevPairingOutcome::Failure { reason }) => {
                ui::spinner_finish_error(
                    &waiting,
                    &format!("Pairing failed: {reason}"),
                );
                exit_codes::EXIT_ERROR
            }
            // broadcast Lagged/Closed: facade torn down or subscriber
            // starved. Neither state is recoverable at this point.
            Err(err) => {
                ui::spinner_finish_error(
                    &waiting,
                    &format!("Outcome stream ended: {err}"),
                );
                exit_codes::EXIT_ERROR
            }
        },
        _ = signal::ctrl_c() => {
            ui::spinner_finish_error(&waiting, "Interrupted by user");
            EXIT_SIGINT
        }
    };

    cli.shutdown().await;
    exit
}
