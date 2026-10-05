//! `uniclip space invite` — sponsor side of Slice 1 pairing.
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
//!
//! JSON mode emits newline-delimited events because the invitation must be
//! available before the command finishes waiting for the joiner.

use serde::Serialize;
use std::io::Write;
use tokio::select;
use tokio::signal;

#[cfg(feature = "dev-tools")]
use std::net::IpAddr;
#[cfg(feature = "dev-tools")]
use std::time::Duration;

// --- daemon path imports (P5-2b) -------------------------------------------
use crate::commands::app_session::connect_or_spawn_oneshot_daemon;
use uc_daemon_client::DaemonClientContext;

// --- engine path imports (debug builds only) ---------------------------------
#[cfg(feature = "dev-tools")]
use uc_engine::{DevOperation, DevOperationResult, Operation, OperationResult};

#[cfg(feature = "dev-tools")]
use crate::commands::app_session::{build_app_session, refuse_if_daemon_running};
use crate::exit_codes;
use crate::ui;

const EXIT_SIGINT: i32 = 130;

#[derive(Serialize)]
#[serde(
    tag = "event",
    rename_all = "snake_case",
    rename_all_fields = "snake_case"
)]
enum InviteEvent<'a> {
    InvitationIssued {
        code: &'a str,
        expires_at_ms: i64,
    },
    PairingCompleted {
        sponsor_device_id: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        joiner_device_id: Option<&'a str>,
    },
    PairingFailed {
        reason: &'a str,
    },
    Interrupted,
}

#[derive(Clone, Copy)]
enum InviteOutputMode {
    Human,
    JsonLines,
}

impl InviteOutputMode {
    fn from_json(json: bool) -> Self {
        if json {
            Self::JsonLines
        } else {
            Self::Human
        }
    }

    fn header(self) {
        if matches!(self, Self::Human) {
            ui::header("Invite a device");
        }
    }

    fn spinner(self, message: &str) -> indicatif::ProgressBar {
        match self {
            Self::Human => ui::spinner(message),
            Self::JsonLines => indicatif::ProgressBar::hidden(),
        }
    }

    fn invitation_issued(
        self,
        spinner: &indicatif::ProgressBar,
        code: &str,
        expires_at_ms: i64,
    ) -> Result<(), String> {
        match self {
            Self::Human => {
                ui::spinner_finish_success(spinner, "Invitation issued");
                ui::bar();
                ui::verification_code(code);
                if let Some(expires_at) = chrono::DateTime::from_timestamp_millis(expires_at_ms) {
                    ui::info("expires_at", &expires_at.to_rfc3339());
                } else {
                    ui::info("expires_at", &format!("{expires_at_ms}ms"));
                }
                ui::bar();
                emit_invitation_code(code)
            }
            Self::JsonLines => {
                spinner.finish_and_clear();
                emit_json_event(&InviteEvent::InvitationIssued {
                    code,
                    expires_at_ms,
                })
            }
        }
    }

    fn request_failed(self, spinner: &indicatif::ProgressBar, message: &str) {
        match self {
            Self::Human => ui::spinner_finish_error(spinner, message),
            Self::JsonLines => {
                spinner.finish_and_clear();
                ui::error(message);
            }
        }
    }

    fn pairing_completed(
        self,
        spinner: &indicatif::ProgressBar,
        sponsor_device_id: &str,
        joiner_device_id: Option<&str>,
    ) -> Result<(), String> {
        match self {
            Self::Human => {
                ui::spinner_finish_success(spinner, "Pairing completed");
                ui::info("sponsor_device_id", sponsor_device_id);
                if let Some(joiner_device_id) = joiner_device_id {
                    ui::info("joiner_device_id", joiner_device_id);
                }
                Ok(())
            }
            Self::JsonLines => {
                spinner.finish_and_clear();
                emit_json_event(&InviteEvent::PairingCompleted {
                    sponsor_device_id,
                    joiner_device_id,
                })
            }
        }
    }

    fn pairing_failed(self, spinner: &indicatif::ProgressBar, reason: &str) -> Result<(), String> {
        match self {
            Self::Human => {
                ui::spinner_finish_error(spinner, &format!("Pairing failed: {reason}"));
                Ok(())
            }
            Self::JsonLines => {
                spinner.finish_and_clear();
                emit_json_event(&InviteEvent::PairingFailed { reason })
            }
        }
    }

    fn interrupted(self, spinner: &indicatif::ProgressBar) -> Result<(), String> {
        match self {
            Self::Human => {
                ui::spinner_finish_error(spinner, "Interrupted by user");
                Ok(())
            }
            Self::JsonLines => {
                spinner.finish_and_clear();
                emit_json_event(&InviteEvent::Interrupted)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry: daemon path (ADR-008 P5-2b)
// ---------------------------------------------------------------------------

pub async fn run(json: bool, verbose: bool) -> i32 {
    let output = InviteOutputMode::from_json(json);
    output.header();

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

    let spinner = output.spinner("Requesting invitation from rendezvous...");
    let invitation = match ctx.setup_v2_client().issue_invitation().await {
        Ok(invitation) => invitation,
        Err(err) => {
            let message = invitation_request_error_message(&err);
            output.request_failed(&spinner, &message);
            return exit_codes::EXIT_ERROR;
        }
    };

    if let Err(message) =
        output.invitation_issued(&spinner, &invitation.code, invitation.expires_at_ms)
    {
        ui::error(&message);
        return exit_codes::EXIT_ERROR;
    }

    let waiting = output.spinner("Waiting for joiner to complete handshake (Ctrl+C to cancel)...");

    select! {
        outcome = rx.recv() => match outcome {
            Some(event) if event.success => {
                match output.pairing_completed(
                    &waiting,
                    &event.sponsor_device_id,
                    event.joiner_device_id.as_deref(),
                ) {
                    Ok(()) => exit_codes::EXIT_SUCCESS,
                    Err(message) => {
                        ui::error(&message);
                        exit_codes::EXIT_ERROR
                    }
                }
            }
            Some(event) => {
                let reason = event.reason.as_deref().unwrap_or("unknown");
                if let Err(message) = output.pairing_failed(&waiting, reason) {
                    ui::error(&message);
                }
                exit_codes::EXIT_ERROR
            }
            None => {
                let reason = "outcome stream ended unexpectedly";
                if let Err(message) = output.pairing_failed(&waiting, reason) {
                    ui::error(&message);
                }
                exit_codes::EXIT_ERROR
            }
        },
        _ = signal::ctrl_c() => {
            if let Err(message) = output.interrupted(&waiting) {
                ui::error(&message);
            }
            EXIT_SIGINT
        }
    }
}

fn invitation_request_error_message(err: &anyhow::Error) -> String {
    let code = err
        .downcast_ref::<uc_daemon_client::DaemonRequestError>()
        .and_then(uc_daemon_client::DaemonRequestError::code);
    match code {
        Some("invitation_no_publishable_address") => {
            "No usable network connection is available for pairing. Connect this device to a network, then try again."
                .to_string()
        }
        Some("invitation_local_publication_failed") => {
            "This device could not make the invitation available on the local network. Check its network connection, then try again."
                .to_string()
        }
        Some("invitation_directory_transport_failed") => {
            "The invitation service could not be reached, and local pairing is unavailable. Check the network, then try again."
                .to_string()
        }
        Some("invitation_directory_rejected") => {
            "The invitation service declined this request. Do not keep retrying; export diagnostics and contact support."
                .to_string()
        }
        Some("invitation_directory_invalid_response") => {
            "The invitation service returned an invalid response. Try again once; if it continues, export diagnostics and contact support."
                .to_string()
        }
        _ => crate::commands::daemon_error_message(err),
    }
}

fn emit_invitation_code(code: &str) -> Result<(), String> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "INVITATION_CODE={code}")
        .and_then(|()| out.flush())
        .map_err(|error| format!("Failed to write invitation code: {error}"))
}

fn emit_json_event(event: &InviteEvent<'_>) -> Result<(), String> {
    let mut out = std::io::stdout().lock();
    serde_json::to_writer(&mut out, event)
        .map_err(|error| format!("Failed to serialize invitation event: {error}"))?;
    writeln!(out)
        .and_then(|()| out.flush())
        .map_err(|error| format!("Failed to write invitation event: {error}"))
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
                "No space on this profile — run `space init` first.",
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

    let baseline_revision = match membership_diagnostics_revision(&cli).await {
        Ok(revision) => revision,
        Err(error) => {
            ui::error(&format!("Failed to read workspace convergence: {error}"));
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
    if let Err(message) = emit_invitation_code(&invitation.code) {
        ui::error(&message);
        cli.shutdown().await;
        return exit_codes::EXIT_ERROR;
    }

    let waiting = ui::spinner("Waiting for joiner to complete handshake (Ctrl+C to cancel)...");

    let wait_for_pairing = async {
        let expires_in = invitation
            .expires_at_ms
            .saturating_sub(chrono::Utc::now().timestamp_millis())
            .max(0);
        let deadline = tokio::time::Instant::now() + Duration::from_millis(expires_in as u64);

        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err("Invitation expired before pairing completed".to_string());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
            match membership_diagnostics_revision(&cli).await {
                Ok(revision) if revision > baseline_revision => return Ok(()),
                Ok(_) => {}
                Err(error) => return Err(format!("Failed to read workspace convergence: {error}")),
            }
        }
    };

    let exit = select! {
        outcome = wait_for_pairing => match outcome {
            Ok(()) => {
                ui::spinner_finish_success(&waiting, "Pairing completed");
                exit_codes::EXIT_SUCCESS
            }
            Err(reason) => {
                ui::spinner_finish_error(
                    &waiting,
                    &format!("Pairing failed: {reason}"),
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

#[cfg(feature = "dev-tools")]
async fn membership_diagnostics_revision(
    cli: &crate::commands::app_session::CliAppSession,
) -> Result<u64, String> {
    match cli
        .engine()
        .execute(Operation::QueryMembershipDiagnostics)
        .await
        .map_err(|error| error.to_string())?
    {
        OperationResult::MembershipDiagnostics(summary) => Ok(summary.revision),
        result => Err(format!("unexpected engine response: {result:?}")),
    }
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;
    use uc_daemon_client::DaemonRequestError;

    use super::{invitation_request_error_message, InviteEvent};

    #[test]
    fn invitation_request_failures_show_distinct_recovery_actions() {
        for (code, expected) in [
            (
                "invitation_no_publishable_address",
                "No usable network connection is available for pairing. Connect this device to a network, then try again.",
            ),
            (
                "invitation_local_publication_failed",
                "This device could not make the invitation available on the local network. Check its network connection, then try again.",
            ),
            (
                "invitation_directory_transport_failed",
                "The invitation service could not be reached, and local pairing is unavailable. Check the network, then try again.",
            ),
            (
                "invitation_directory_rejected",
                "The invitation service declined this request. Do not keep retrying; export diagnostics and contact support.",
            ),
            (
                "invitation_directory_invalid_response",
                "The invitation service returned an invalid response. Try again once; if it continues, export diagnostics and contact support.",
            ),
        ] {
            let error = anyhow::Error::new(DaemonRequestError::Status {
                path: "/v2/setup/issue-invitation".to_string(),
                status: if code == "invitation_directory_rejected" {
                    StatusCode::CONFLICT
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                },
                code: Some(code.to_string()),
                message: "safe daemon message".to_string(),
            });

            assert_eq!(invitation_request_error_message(&error), expected);
        }
    }

    #[test]
    fn json_events_are_independently_parseable() {
        let issued = serde_json::to_value(InviteEvent::InvitationIssued {
            code: "1234-5678",
            expires_at_ms: 42,
        })
        .expect("issued event should serialize");
        let completed = serde_json::to_value(InviteEvent::PairingCompleted {
            sponsor_device_id: "sponsor-1",
            joiner_device_id: Some("joiner-1"),
        })
        .expect("completed event should serialize");

        assert_eq!(
            issued,
            serde_json::json!({
                "event": "invitation_issued",
                "code": "1234-5678",
                "expires_at_ms": 42
            })
        );
        assert_eq!(
            completed,
            serde_json::json!({
                "event": "pairing_completed",
                "sponsor_device_id": "sponsor-1",
                "joiner_device_id": "joiner-1"
            })
        );
    }
}
