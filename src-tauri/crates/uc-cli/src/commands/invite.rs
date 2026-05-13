//! `uniclip invite` — sponsor side of Slice 1 pairing.
//!
//! Silently resumes the local session (using the KEK cached in
//! keychain / file secure storage by a prior `init` or `unlock`),
//! issues a fresh pairing invitation (B1), prints the code, and blocks
//! until either:
//! * [`PairingOutcome::Success`] fires (admit + trust + Confirm landed
//!   on this device), exiting 0.
//! * [`PairingOutcome::Failure`] fires (proof mismatch, admit/trust
//!   error, Confirm send failure, invitation expired on-the-wire),
//!   exiting 1.
//! * The user sends Ctrl+C, exiting 130 per the conventional SIGINT
//!   shell code — assembly teardown still runs.

use tokio::select;
use tokio::signal;

use std::net::IpAddr;

use uc_application::facade::space_setup::{
    IssuePairingInvitationError, PairingOutcome, TryResumeSessionError,
};

use crate::commands::app_session::{build_app_session, refuse_if_daemon_running};
use crate::exit_codes;
use crate::ui;

const EXIT_SIGINT: i32 = 130;

pub async fn run(verbose: bool) -> i32 {
    run_inner(None, verbose).await
}

pub(crate) async fn run_for_address(selected_ip: IpAddr, verbose: bool) -> i32 {
    run_inner(Some(selected_ip), verbose).await
}

async fn run_inner(selected_ip: Option<IpAddr>, verbose: bool) -> i32 {
    match selected_ip {
        Some(ip) => ui::header(&format!("Invite a device via {ip}")),
        None => ui::header("Invite a device"),
    }

    if let Err(code) = refuse_if_daemon_running().await {
        return code;
    }

    let cli = match build_app_session(verbose).await {
        Ok(bundle) => bundle,
        Err(code) => return code,
    };

    // Silently resume the in-memory session using the KEK the last
    // `init`/`unlock` call cached in secure storage. Without this the
    // HMAC proof verifier later fails with "space session is locked"
    // when the joiner's ChallengeResponse arrives — manifesting to the
    // joiner as a spurious `PassphraseMismatch`.
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
    let issue_result = match selected_ip {
        Some(ip) => {
            cli.app_facade()
                .issue_pairing_invitation_for_address(ip)
                .await
        }
        None => cli.app_facade().issue_pairing_invitation().await,
    };
    let invitation = match issue_result {
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

    // Machine-readable line on stdout so scripts (e.g., the single-
    // machine e2e test) can capture the code without parsing ANSI from
    // the styled stderr output. Humans see the styled version above.
    // Explicit flush because Rust stdout is fully-buffered when piped.
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
                ui::spinner_finish_error(&waiting, &format!("Pairing failed: {reason}"));
                exit_codes::EXIT_ERROR
            }
            // broadcast Lagged/Closed: facade torn down or subscriber
            // starved. Neither state is recoverable at this point.
            Err(err) => {
                ui::spinner_finish_error(&waiting, &format!("Outcome stream ended: {err}"));
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
