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
//! * **`run_for_address(ip, verbose)`** — in-process path (dev-only).
//!   Uses the pre-P5 facade API directly; called from `dev pairing
//!   issue --addr <ip>`. Stays in-process until P5-3 migrates all
//!   dev commands.

use tokio::select;
use tokio::signal;

#[cfg(feature = "dev-tools")]
use std::net::IpAddr;

// --- daemon path imports (P5-2b) -------------------------------------------
use crate::commands::app_session::connect_or_spawn_oneshot_daemon;
use uc_daemon_client::DaemonClientContext;

// --- in-process path imports (debug builds only) -----------------------------
#[cfg(feature = "dev-tools")]
use uc_application::facade::space_setup::{
    IssuePairingInvitationError, PairingOutcome, TryResumeSessionError,
};

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

    // Silently resume the in-memory session using the KEK the last
    // `init`/`unlock` call cached in secure storage.
    let resume_spinner = ui::spinner("Resuming space session...");
    match cli.app_facade().try_resume_session().await {
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
        Err(TryResumeSessionError::CorruptedKeyMaterial) => {
            ui::spinner_finish_error(
                &resume_spinner,
                "Key material is corrupted — consider resetting this profile.",
            );
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(TryResumeSessionError::KeyringMiss) => {
            ui::spinner_finish_error(
                &resume_spinner,
                "Keychain cannot silently unlock this space. Run a future \
                 `uniclip unlock` (not yet shipped) or re-init.",
            );
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(TryResumeSessionError::Internal(msg)) => {
            ui::spinner_finish_error(&resume_spinner, &format!("Resume failed: {msg}"));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    }

    // Subscribe BEFORE issuing so we never miss an outcome that would
    // race between B1 returning and this task subscribing.
    let mut outcome_rx = match cli.app_facade().subscribe_pairing_completion() {
        Ok(rx) => rx,
        Err(err) => {
            ui::error(&format!("Failed to subscribe pairing completion: {err}"));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    };

    let spinner = ui::spinner("Requesting invitation from rendezvous...");
    let invitation = match cli
        .app_facade()
        .issue_pairing_invitation_for_address(selected_ip)
        .await
    {
        Ok(res) => {
            ui::spinner_finish_success(&spinner, "Invitation issued");
            res
        }
        Err(IssuePairingInvitationError::NetworkNotStarted) => {
            ui::spinner_finish_error(&spinner, "Network not started — run `init` first.");
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(IssuePairingInvitationError::AddressNotAvailable(addr)) => {
            ui::spinner_finish_error(&spinner, &format!("Address is not available: {addr}"));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(other) => {
            ui::spinner_finish_error(&spinner, &format!("{other}"));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    };

    ui::bar();
    ui::verification_code(invitation.code.as_str());
    ui::info("expires_at", &invitation.expires_at.to_rfc3339());
    ui::bar();

    // Machine-readable line on stdout so scripts can capture the code.
    {
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "INVITATION_CODE={}", invitation.code.as_str());
        let _ = out.flush();
    }

    let waiting = ui::spinner("Waiting for joiner to complete handshake (Ctrl+C to cancel)...");

    let exit = select! {
        outcome = outcome_rx.recv() => match outcome {
            Ok(PairingOutcome::Success {
                peer_device_id,
                peer_device_name,
                peer_fingerprint,
            }) => {
                ui::spinner_finish_success(&waiting, "Pairing completed");
                ui::info("peer_device_id", peer_device_id.as_str());
                ui::info("peer_device_name", &peer_device_name);
                ui::info("peer_fingerprint", &peer_fingerprint.to_string());
                exit_codes::EXIT_SUCCESS
            }
            Ok(PairingOutcome::Failure { reason }) => {
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
