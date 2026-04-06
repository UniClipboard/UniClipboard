//! Interactive setup commands over daemon-owned setup state.

// Submodules for phase-driven flow
#[path = "setup/host_flow.rs"]
mod host_flow;
#[path = "setup/join_flow.rs"]
mod join_flow;

pub use host_flow::{derive_host_phase, HostCliPhase, HostCliSession};
pub use join_flow::{derive_join_phase, JoinCliPhase, JoinCliSession};

use std::fmt;
use std::io::{self, IsTerminal};
use std::time::Duration;

use console::style;
use serde::Serialize;
use serde_json::Value;
use uc_app::usecases::CoreUseCases;
use uc_core::security::model::Passphrase;
use uc_core::security::state::EncryptionState;
use uc_daemon::api::dto::setup::SetupStateResponseDto;
use uc_daemon::api::types::{PeerSnapshotDto, SetupStateResponse};
// Re-export for integration tests (same crate)
pub(crate) use uc_daemon_client::setup::{format_peer_id_suffix, parse_setup_state, SetupVariant};
use uc_daemon_client::{DaemonClientContext, DaemonPairingClient, DaemonSetupClient};

use uc_cli_macros::autostop;

use crate::exit_codes;
// `ensure_local_daemon_running` is referenced in source but rewritten to
// `ensure_local_daemon_running_capture` by the `#[autostop]` proc macro before
// name resolution; both imports are kept so the raw source remains readable.
#[allow(unused_imports)]
use crate::local_daemon::{
    ensure_local_daemon_running, ensure_local_daemon_running_capture, LocalDaemonError,
};
use crate::output;
use crate::ui;

const POLL_INTERVAL: Duration = Duration::from_millis(400);
const HOST_LEASE_REFRESH_INTERVAL: Duration = Duration::from_secs(20);

// ── Interactive guide (no subcommand) ───────────────────────────────

pub async fn run_interactive(json: bool, verbose: bool) -> i32 {
    if json {
        eprintln!("Error: `--json` is only supported with `setup status`");
        return exit_codes::EXIT_ERROR;
    }
    if !stdin_is_terminal() {
        eprintln!("Error: interactive setup requires a terminal");
        return exit_codes::EXIT_ERROR;
    }

    ui::header("Welcome to UniClipboard");

    let items = vec![
        "Create new Space (I'm the first device)".to_string(),
        "Join existing Space (connect to another device)".to_string(),
    ];

    let choice = match ui::select("What would you like to do?", &items) {
        Ok(choice) => choice,
        Err(e) => {
            ui::error(&format!("Setup cancelled: {e}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    ui::bar();

    match choice {
        0 => run_new_space().await,
        1 => run_connect(json, verbose).await,
        _ => unreachable!(),
    }
}

// ── New Space flow (create encrypted space only, no pairing) ────────

/// Returns `Ok(())` if encryption state allows new-space initialization,
/// or `Err(exit_code)` if the operation should be rejected.
///
/// Uses a whitelist approach: only `Uninitialized` is allowed.
/// All other states (Initializing, Initialized, Error, etc.) are rejected.
pub fn new_space_encryption_guard(state: EncryptionState) -> Result<(), i32> {
    match state {
        EncryptionState::Uninitialized => Ok(()),
        _ => Err(exit_codes::EXIT_ERROR),
    }
}

async fn run_new_space() -> i32 {
    // 1. Build CLI runtime directly (no daemon needed for encryption init)
    let runtime = match uc_bootstrap::build_cli_runtime(Some(uc_observability::LogProfile::Cli)) {
        Ok(r) => r,
        Err(e) => {
            ui::error(&format!("Failed to initialize: {e}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    // 2. Check encryption state — reject if already initialized
    let state = match runtime.encryption_state().await {
        Ok(s) => s,
        Err(e) => {
            ui::error(&format!("Failed to check encryption state: {e}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    if let Err(code) = new_space_encryption_guard(state) {
        ui::error("Space already initialized.");
        ui::info(
            "Hint",
            "run `uniclipboard setup` and select 'Create new Space' to initialize your space first",
        );
        return code;
    }

    // 3. Prompt for passphrase
    let passphrase_str = match prompt_new_space_passphrase() {
        Ok(p) => p,
        Err(e) => {
            ui::error(&e);
            return exit_codes::EXIT_ERROR;
        }
    };

    // 4. Initialize encryption locally (no daemon involved)
    let spinner = ui::spinner("Creating encrypted space...");
    let uc = CoreUseCases::new(&runtime);
    match uc
        .initialize_encryption()
        .execute(Passphrase(passphrase_str))
        .await
    {
        Ok(()) => {
            ui::spinner_finish_success(&spinner, "Encrypted space created");
        }
        Err(e) => {
            ui::spinner_finish_error(&spinner, &format!("Failed to create space: {e}"));
            return exit_codes::EXIT_ERROR;
        }
    }

    // 5. Persist setup completion so daemon/GUI recognise setup is done
    if let Err(e) = uc.mark_setup_complete().execute().await {
        ui::error(&format!("Failed to persist setup status: {e}"));
        return exit_codes::EXIT_ERROR;
    }

    // 6. Success
    ui::bar();
    ui::end("Setup complete! Your space is ready.");
    exit_codes::EXIT_SUCCESS
}

// ── Host flow ───────────────────────────────────────────────────────

#[autostop]
pub async fn run_pair(json: bool, _verbose: bool) -> i32 {
    if json {
        eprintln!("Error: `--json` is only supported with `setup status`");
        return exit_codes::EXIT_ERROR;
    }
    if !stdin_is_terminal() {
        eprintln!("Error: `setup pair` requires an interactive terminal");
        return exit_codes::EXIT_ERROR;
    }

    let daemon_session = match ensure_local_daemon_running().await {
        Ok(s) => s,
        Err(error) => return print_local_daemon_error(error),
    };

    let ctx = match DaemonClientContext::from_env() {
        Ok(ctx) => ctx,
        Err(error) => {
            ui::error(&format!("Failed to connect to daemon: {error}"));
            return exit_codes::EXIT_DAEMON_UNREACHABLE;
        }
    };
    let setup_client = ctx.setup_client();
    let pairing_client = ctx.pairing_client();

    let initial_state = match clear_transient_setup_state(&setup_client).await {
        Ok(state) => state,
        Err(error) => return print_anyhow_error(error),
    };

    ui::step("Device identity");
    print_identity_banner(&initial_state);

    // Guard: space must already be initialized before entering pairing mode.
    if !initial_state.has_completed {
        ui::error("Space is not initialized.");
        ui::info(
            "Hint",
            "run `uniclipboard setup` and select 'Create new Space' to initialize your space first",
        );
        return exit_codes::EXIT_ERROR;
    }

    // ── Phase-driven session ─────────────────────────────────
    let mut session = HostCliSession::default();
    let mut submitted_decision_session: Option<String> = None;
    let mut submitted_verification_session: Option<String> = None;

    // State signature for debug logging (D-05).
    let mut last_state_signature: Option<String> = None;

    loop {
        // POLL
        let dto: SetupStateResponseDto = match setup_client.get_setup_state().await {
            Ok(state) => state.data,
            Err(error) => {
                finish_spinner(&mut session.spinner);
                return print_anyhow_error(error);
            }
        };

        // PARSE
        let parsed = parse_setup_state(&dto);

        // DERIVE PHASE
        let next_phase = derive_host_phase(&parsed, &session.phase);

        // DEBUG LOG: print when state signature changes (D-05).
        let signature = format!("{:?}", parsed);
        if last_state_signature.as_ref() != Some(&signature) {
            tracing::debug!(host_phase = ?next_phase, hint = ?parsed.hint, variant = ?parsed.variant, session_id = ?parsed.session_id, "host pairing state changed");
            last_state_signature = Some(signature);
        }

        // PHASE CHANGED: UI update only (D-17).
        if next_phase != session.phase {
            on_host_phase_changed(&session.phase, &next_phase, &mut session.spinner);
            session.phase = next_phase;
        }

        // EXECUTE ACTION (match on current phase).
        let action_result: Result<(), i32> = match &session.phase {
            HostCliPhase::WaitingJoinRequest => {
                // No action needed; backend sends events. Enable pairing presence once.
                if !session.pairing_presence_enabled {
                    if pairing_client
                        .register_gui_participant(true, None)
                        .await
                        .is_err()
                    {
                        finish_spinner(&mut session.spinner);
                        return exit_codes::EXIT_ERROR;
                    }
                    session.pairing_presence_enabled = true;
                    session.last_lease_refresh = std::time::Instant::now();
                } else if session.last_lease_refresh.elapsed() >= HOST_LEASE_REFRESH_INTERVAL {
                    if pairing_client
                        .register_gui_participant(true, None)
                        .await
                        .is_err()
                    {
                        finish_spinner(&mut session.spinner);
                        return exit_codes::EXIT_ERROR;
                    }
                    session.last_lease_refresh = std::time::Instant::now();
                }
                if session.spinner.is_none() {
                    session.spinner =
                        Some(ui::spinner("Host ready. Waiting for a join request..."));
                }
                Ok(())
            }

            HostCliPhase::NeedDecision { session_id } => {
                // Only prompt if we haven't submitted a decision for this session.
                if submitted_decision_session.as_deref() == Some(session_id) {
                    Ok(()) // Already submitted; continue polling via sleep.
                } else {
                    finish_spinner(&mut session.spinner);
                    let peer_label = parsed
                        .selected_peer_label
                        .clone()
                        .unwrap_or_else(|| "unknown peer".to_string());
                    ui::step(&format!(
                        "Join request from {}",
                        console::style(peer_label).bold()
                    ));
                    if let Some(code) = &parsed.short_code {
                        ui::verification_code(code);
                    }
                    match ui::confirm("Accept this peer?", true) {
                        Ok(true) => {
                            if setup_client.confirm_peer_trust().await.is_err() {
                                return exit_codes::EXIT_ERROR;
                            }
                            submitted_decision_session = Some(session_id.clone());
                            Ok(())
                        }
                        Ok(false) => {
                            if setup_client.cancel_setup().await.is_err()
                                || disable_host_pairing_presence(
                                    &pairing_client,
                                    &mut session.pairing_presence_enabled,
                                )
                                .await
                                .is_err()
                            {
                                return exit_codes::EXIT_ERROR;
                            }
                            ui::warn("Host pairing canceled.");
                            return exit_codes::EXIT_ERROR; // Canceled -> treat as error for loop exit.
                        }
                        Err(e) => {
                            ui::error(&e);
                            return exit_codes::EXIT_ERROR;
                        }
                    }
                }
            }

            HostCliPhase::NeedVerification { session_id } => {
                // Only prompt if we haven't submitted verification for this session.
                if submitted_verification_session.as_deref() == Some(session_id) {
                    Ok(()) // Already submitted; continue polling via sleep.
                } else {
                    finish_spinner(&mut session.spinner);
                    let peer_label = parsed
                        .selected_peer_label
                        .clone()
                        .unwrap_or_else(|| "selected peer".to_string());
                    ui::step(&format!(
                        "Confirm peer trust for {}",
                        console::style(peer_label).bold()
                    ));
                    if let Some(code) = &parsed.short_code {
                        ui::verification_code(code);
                    }
                    match ui::confirm("Do the verification codes match?", true) {
                        Ok(true) => {
                            if setup_client.confirm_peer_trust().await.is_err() {
                                return exit_codes::EXIT_ERROR;
                            }
                            submitted_verification_session = Some(session_id.clone());
                            Ok(())
                        }
                        Ok(false) => {
                            if setup_client.cancel_setup().await.is_err() {
                                return exit_codes::EXIT_ERROR;
                            }
                            ui::warn("Host pairing canceled.");
                            return exit_codes::EXIT_ERROR; // Canceled -> treat as error for loop exit.
                        }
                        Err(e) => {
                            ui::error(&e);
                            return exit_codes::EXIT_ERROR;
                        }
                    }
                }
            }

            HostCliPhase::WaitingBackendCompletion => {
                // Nothing to do; just wait for the backend to complete.
                if session.spinner.is_none() {
                    session.spinner = Some(ui::spinner("Processing..."));
                }
                Ok(())
            }

            HostCliPhase::Completed => {
                finish_spinner(&mut session.spinner);
                let _ = disable_host_pairing_presence(
                    &pairing_client,
                    &mut session.pairing_presence_enabled,
                )
                .await;
                ui::success("Setup host flow completed!");
                if daemon_session.spawned {
                    ui::info(
                        "Next",
                        "daemon will stop shortly. Run `uniclipboard start` to begin clipboard sync.",
                    );
                }
                return exit_codes::EXIT_SUCCESS;
            }

            HostCliPhase::Canceled => {
                finish_spinner(&mut session.spinner);
                let _ = disable_host_pairing_presence(
                    &pairing_client,
                    &mut session.pairing_presence_enabled,
                )
                .await;
                ui::end("Host setup returned to idle.");
                return exit_codes::EXIT_SUCCESS;
            }
        };

        // D-18: action failure returns EXIT_ERROR immediately (no retry).
        if action_result.is_err() {
            return exit_codes::EXIT_ERROR;
        }

        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Per D-17: on_phase_changed handles only UI state (spinner, logging).
/// No business logic here.
fn on_host_phase_changed(
    old: &HostCliPhase,
    new: &HostCliPhase,
    spinner: &mut Option<indicatif::ProgressBar>,
) {
    if old.is_terminal() {
        return; // Don't log transitions from terminal states.
    }
    tracing::debug!(from = ?old, to = ?new, "host phase transition");
    // Clear spinner on any phase change so the new phase sets its own.
    finish_spinner(spinner);
}

// ── Join flow ───────────────────────────────────────────────────────

#[autostop]
pub async fn run_connect(json: bool, _verbose: bool) -> i32 {
    if json {
        eprintln!("Error: `--json` is only supported with `setup status`");
        return exit_codes::EXIT_ERROR;
    }
    if !stdin_is_terminal() {
        eprintln!("Error: `setup join` requires an interactive terminal");
        return exit_codes::EXIT_ERROR;
    }

    let daemon_session = match ensure_local_daemon_running().await {
        Ok(s) => s,
        Err(error) => return print_local_daemon_error(error),
    };

    let ctx = match DaemonClientContext::from_env() {
        Ok(ctx) => ctx,
        Err(error) => {
            ui::error(&format!("Failed to connect to daemon: {error}"));
            return exit_codes::EXIT_DAEMON_UNREACHABLE;
        }
    };
    let setup_client = ctx.setup_client();
    let query_client = ctx.query_client();

    let initial_state = match clear_transient_setup_state(&setup_client).await {
        Ok(state) => state,
        Err(error) => return print_anyhow_error(error),
    };

    ui::step("Device identity");
    print_identity_banner(&initial_state);

    if initial_state.has_completed {
        ui::error("This device is already connected to a Space.");
        ui::info(
            "Hint",
            "Use `uniclipboard setup pair` on this device to connect another device.",
        );
        return exit_codes::EXIT_ERROR;
    }

    if let Err(error) = setup_client.start_join_space().await {
        return print_anyhow_error(error);
    }

    // ── Phase-driven session ─────────────────────────────────
    let mut session = JoinCliSession::default();

    // State signature for debug logging (D-05).
    let mut last_state_signature: Option<String> = None;

    loop {
        // POLL
        let dto: SetupStateResponseDto = match setup_client.get_setup_state().await {
            Ok(state) => state.data,
            Err(error) => {
                finish_spinner(&mut session.spinner);
                return print_anyhow_error(error);
            }
        };

        // PARSE
        let parsed = parse_setup_state(&dto);

        // DERIVE PHASE
        let next_phase = derive_join_phase(&parsed, &session.phase);

        // DEBUG LOG: print when state signature changes (D-05).
        let signature = format!("{:?}", parsed);
        if last_state_signature.as_ref() != Some(&signature) {
            tracing::debug!(join_phase = ?next_phase, hint = ?parsed.hint, variant = ?parsed.variant, session_id = ?parsed.session_id, "join pairing state changed");
            last_state_signature = Some(signature);
        }

        // PHASE CHANGED: UI update only (D-17).
        if next_phase != session.phase {
            on_join_phase_changed(&session.phase, &next_phase, &mut session.spinner);
            session.phase = next_phase;
        }

        // EXECUTE ACTION.
        let action_result: Result<(), i32> = match &session.phase {
            JoinCliPhase::SelectingPeer => {
                // If the joiner has already submitted a peer request, don't re-prompt.
                // We're waiting for the backend to transition us out of SelectingPeer
                // (to WaitingHostResponse / NeedPeerConfirmation / NeedPassphrase).
                if session.submitted_peer_request {
                    if session.spinner.is_none() {
                        session.spinner = Some(ui::spinner("Waiting for host response..."));
                    }
                    tokio::time::sleep(POLL_INTERVAL).await;
                    continue;
                }
                // Show spinner while discovering.
                if session.spinner.is_none() {
                    session.spinner = Some(ui::spinner("Discovering peers on the network..."));
                }
                let peers = match query_client.get_peers().await {
                    Ok(peers) => filter_joinable_peers(peers),
                    Err(error) => {
                        finish_spinner(&mut session.spinner);
                        return print_anyhow_error(error);
                    }
                };
                if peers.is_empty() {
                    // Keep polling; spinner already shown.
                    Ok(())
                } else {
                    finish_spinner(&mut session.spinner);
                    match prompt_for_peer_selection(&peers) {
                        Ok(Some(peer_id)) => {
                            session.submitted_peer_request = true;
                            session.spinner = Some(ui::spinner("Connecting to peer..."));
                            if setup_client.select_device(peer_id).await.is_err() {
                                finish_spinner(&mut session.spinner);
                                return exit_codes::EXIT_ERROR;
                            }
                            Ok(())
                        }
                        Ok(None) => {
                            // User canceled.
                            if setup_client.cancel_setup().await.is_err() {
                                return exit_codes::EXIT_ERROR;
                            }
                            ui::warn("Join setup canceled.");
                            return exit_codes::EXIT_ERROR; // Canceled -> treat as error for loop exit.
                        }
                        Err(e) => {
                            ui::error(&e);
                            return exit_codes::EXIT_ERROR;
                        }
                    }
                }
            }

            JoinCliPhase::WaitingPeerDiscovery => {
                // Currently unused — SelectingPeer shows spinner while discovering.
                // This phase exists for future extensibility.
                Ok(())
            }

            JoinCliPhase::WaitingHostResponse => {
                if session.spinner.is_none() {
                    session.spinner = Some(ui::spinner("Waiting for host response..."));
                }
                Ok(())
            }

            JoinCliPhase::NeedPeerConfirmation { session_id: _ } => {
                // Idempotent: skip if already confirmed.
                finish_spinner(&mut session.spinner);
                let peer_label = parsed
                    .selected_peer_label
                    .clone()
                    .unwrap_or_else(|| "selected peer".to_string());
                ui::step(&format!(
                    "Confirm peer trust for {}",
                    console::style(peer_label).bold()
                ));
                if let Some(code) = &parsed.short_code {
                    ui::verification_code(code);
                }
                match ui::confirm("Do the verification codes match?", true) {
                    Ok(true) => {
                        if setup_client.confirm_peer_trust().await.is_err() {
                            return exit_codes::EXIT_ERROR;
                        }
                        Ok(())
                    }
                    Ok(false) => {
                        if setup_client.cancel_setup().await.is_err() {
                            return exit_codes::EXIT_ERROR;
                        }
                        ui::warn("Join setup canceled.");
                        return exit_codes::EXIT_ERROR; // Canceled -> treat as error for loop exit.
                    }
                    Err(e) => {
                        ui::error(&e);
                        return exit_codes::EXIT_ERROR;
                    }
                }
            }

            JoinCliPhase::NeedPassphrase => {
                // Show passphrase error warning if applicable.
                if parsed.error_code.as_deref() == Some("PassphraseInvalidOrMismatch") {
                    ui::warn("Passphrase rejected; retrying current join session");
                }
                finish_spinner(&mut session.spinner);
                let passphrase: String = match ui::password("Space passphrase") {
                    Ok(p) if p.trim().is_empty() => {
                        ui::error("Passphrase cannot be empty");
                        return exit_codes::EXIT_ERROR;
                    }
                    Ok(p) => p,
                    Err(e) => {
                        ui::error(&e);
                        return exit_codes::EXIT_ERROR;
                    }
                };
                session.spinner = Some(ui::spinner("Verifying passphrase..."));
                if setup_client.verify_passphrase(passphrase).await.is_err() {
                    finish_spinner(&mut session.spinner);
                    return exit_codes::EXIT_ERROR;
                }
                Ok(())
            }

            JoinCliPhase::WaitingBackendCompletion => {
                if session.spinner.is_none() {
                    session.spinner = Some(ui::spinner("Processing..."));
                }
                Ok(())
            }

            JoinCliPhase::Completed => {
                finish_spinner(&mut session.spinner);
                ui::success("Setup join flow completed!");
                if daemon_session.spawned {
                    ui::info(
                        "Next",
                        "daemon will stop shortly. Run `uniclipboard start` to begin clipboard sync.",
                    );
                }
                return exit_codes::EXIT_SUCCESS;
            }

            JoinCliPhase::Canceled => {
                finish_spinner(&mut session.spinner);
                ui::end("Join setup canceled.");
                return exit_codes::EXIT_SUCCESS;
            }
        };

        // D-18: action failure returns EXIT_ERROR immediately (no retry).
        if action_result.is_err() {
            return exit_codes::EXIT_ERROR;
        }

        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Per D-17: on_phase_changed handles only UI state (spinner, logging).
fn on_join_phase_changed(
    old: &JoinCliPhase,
    new: &JoinCliPhase,
    spinner: &mut Option<indicatif::ProgressBar>,
) {
    if old.is_terminal() {
        return;
    }
    tracing::debug!(from = ?old, to = ?new, "join phase transition");
    finish_spinner(spinner);
}

// ── Status & Reset (non-interactive) ────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupStatusOutput {
    state: Value,
    session_id: Option<String>,
    next_step_hint: String,
    profile: String,
    clipboard_mode: String,
    device_name: String,
    peer_id: String,
}

impl fmt::Display for SetupStatusOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let variant = SetupVariant::from_state_value(&self.state);
        match variant {
            SetupVariant::Idle => writeln!(f, "state: Idle")?,
            SetupVariant::JoinSpaceConfirmPeer => writeln!(f, "state: JoinSpaceConfirmPeer")?,
            SetupVariant::JoinSpaceInputPassphrase => {
                writeln!(f, "state: JoinSpaceInputPassphrase")?
            }
            SetupVariant::Completed => writeln!(f, "state: Completed")?,
            SetupVariant::Unknown(s) => writeln!(f, "state: {}", s)?,
        }
        writeln!(
            f,
            "sessionId: {}",
            self.session_id.as_deref().unwrap_or("-")
        )?;
        writeln!(f, "nextStepHint: {}", self.next_step_hint)?;
        writeln!(f, "profile: {}", self.profile)?;
        writeln!(f, "clipboardMode: {}", self.clipboard_mode)?;
        writeln!(f, "deviceName: {}", self.device_name)?;
        write!(f, "peerId: {}", self.peer_id)
    }
}

pub async fn run_status(json: bool, _verbose: bool) -> i32 {
    let ctx = match DaemonClientContext::from_env() {
        Ok(ctx) => ctx,
        Err(error) => {
            ui::error(&format!("Failed to connect to daemon: {error}"));
            return exit_codes::EXIT_DAEMON_UNREACHABLE;
        }
    };
    let setup_client = ctx.setup_client();

    let state_dto = match setup_client.get_setup_state().await {
        Ok(state) => state.data,
        Err(error) => return print_anyhow_error(error),
    };

    let output_value = SetupStatusOutput::from(state_dto);
    if let Err(error) = output::print_result(&output_value, json) {
        eprintln!("Error: {error}");
        return exit_codes::EXIT_ERROR;
    }

    exit_codes::EXIT_SUCCESS
}

#[autostop]
pub async fn run_reset(json: bool, _verbose: bool) -> i32 {
    if json {
        eprintln!("Error: `--json` is not supported with `setup reset`");
        return exit_codes::EXIT_ERROR;
    }

    let session = match ensure_local_daemon_running().await {
        Ok(session) => session,
        Err(error) => return print_local_daemon_error(error),
    };

    let ctx = match DaemonClientContext::from_env() {
        Ok(ctx) => ctx,
        Err(error) => {
            ui::error(&format!("Failed to connect to daemon: {error}"));
            return exit_codes::EXIT_DAEMON_UNREACHABLE;
        }
    };

    let response = match ctx.setup_client().reset_setup().await {
        Ok(response) => response,
        Err(error) => return print_anyhow_error(error),
    };

    // `session.spawned == true` → the autostop guard (installed by #[autostop])
    // will SIGTERM the daemon on function return. Reflect that in the output
    // rather than trusting the backend's hard-coded `daemon_kept_running` field.
    ui::success(&render_reset_output(&response.profile, !session.spawned));
    if session.spawned {
        ui::info(
            "Next",
            "daemon will stop shortly. Run `uniclipboard start` to begin clipboard sync.",
        );
    }

    exit_codes::EXIT_SUCCESS
}

// ── From impl ───────────────────────────────────────────────────────

impl From<SetupStateResponse> for SetupStatusOutput {
    fn from(value: SetupStateResponse) -> Self {
        Self {
            state: value.state,
            session_id: value.session_id,
            next_step_hint: value.next_step_hint,
            profile: value.profile,
            clipboard_mode: value.clipboard_mode,
            device_name: value.device_name,
            peer_id: value.peer_id,
        }
    }
}

impl From<SetupStateResponseDto> for SetupStatusOutput {
    fn from(value: SetupStateResponseDto) -> Self {
        Self {
            state: value.state,
            session_id: value.session_id,
            next_step_hint: value.next_step_hint,
            profile: value.profile,
            clipboard_mode: value.clipboard_mode,
            device_name: value.device_name,
            peer_id: value.peer_id,
        }
    }
}

// ── Prompt helpers ──────────────────────────────────────────────────

fn stdin_is_terminal() -> bool {
    io::stdin().is_terminal()
}

fn print_identity_banner(state: &SetupStateResponseDto) {
    ui::identity_banner(
        &state.profile,
        &state.clipboard_mode,
        &state.device_name,
        &state.peer_id,
    );
}

fn prompt_new_space_passphrase() -> Result<String, String> {
    ui::bar();
    ui::password_with_confirm("New space passphrase", "Confirm passphrase")
}

fn prompt_for_peer_selection(peers: &[PeerSnapshotDto]) -> Result<Option<String>, String> {
    let items: Vec<String> = peers
        .iter()
        .map(|peer| {
            let name = peer.device_name.as_deref().unwrap_or("unknown device");
            format!("{name} ({})", format_peer_id_suffix(&peer.peer_id))
        })
        .collect();

    let mut all_items = items;
    all_items.push(style("Cancel").dim().to_string());

    ui::step("Select a peer to join");

    let chosen = ui::select("Discovered peers", &all_items)?;

    if chosen == all_items.len() - 1 {
        return Ok(None);
    }

    Ok(Some(peers[chosen].peer_id.clone()))
}

// ── Spinner management ──────────────────────────────────────────────

fn finish_spinner(spinner: &mut Option<indicatif::ProgressBar>) {
    if let Some(pb) = spinner.take() {
        pb.finish_and_clear();
    }
}

/// Wraps [`DaemonSetupClient::clear_transient_state`], extracting the inner data.
async fn clear_transient_setup_state(
    setup_client: &DaemonSetupClient,
) -> Result<SetupStateResponseDto, anyhow::Error> {
    Ok(setup_client.clear_transient_state().await?.data)
}

// ── Render helpers ──────────────────────────────────────────────────

pub(crate) fn render_reset_output(profile: &str, daemon_kept_running: bool) -> String {
    let mut lines = vec![format!("Reset complete for profile {profile}")];
    if daemon_kept_running {
        lines.push("Daemon kept running".to_string());
    } else {
        lines.push("Daemon stopped".to_string());
    }
    lines.join("\n")
}

fn filter_joinable_peers(peers: Vec<PeerSnapshotDto>) -> Vec<PeerSnapshotDto> {
    let mut peers: Vec<_> = peers.into_iter().filter(|peer| !peer.is_paired).collect();
    peers.sort_by(|left, right| {
        left.device_name
            .as_deref()
            .unwrap_or(left.peer_id.as_str())
            .cmp(
                right
                    .device_name
                    .as_deref()
                    .unwrap_or(right.peer_id.as_str()),
            )
    });
    peers
}

async fn disable_host_pairing_presence(
    client: &DaemonPairingClient,
    host_pairing_presence_enabled: &mut bool,
) -> Result<(), anyhow::Error> {
    if !*host_pairing_presence_enabled {
        return Ok(());
    }

    client.register_gui_participant(false, None).await?;
    *host_pairing_presence_enabled = false;
    Ok(())
}

fn print_local_daemon_error(error: LocalDaemonError) -> i32 {
    ui::error(&format!("{error}"));
    exit_codes::EXIT_DAEMON_UNREACHABLE
}

fn print_anyhow_error(error: anyhow::Error) -> i32 {
    ui::error(&format!("{error}"));
    exit_codes::EXIT_ERROR
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identity_banner_contains_fixed_fields() {
        let state = SetupStateResponse {
            state: Value::String("Welcome".to_string()),
            session_id: Some("session-1".to_string()),
            next_step_hint: "idle".to_string(),
            profile: "peerA".to_string(),
            clipboard_mode: "full".to_string(),
            device_name: "Peer A".to_string(),
            peer_id: "peer-a".to_string(),
            selected_peer_id: None,
            selected_peer_name: None,
            has_completed: false,
        };

        // Just verify the output doesn't panic.
        let output = SetupStatusOutput::from(state);
        let rendered = format!("{output}");
        assert!(rendered.contains("peerA"));
        assert!(rendered.contains("full"));
        assert!(rendered.contains("Peer A"));
        assert!(rendered.contains("peer-a"));
    }

    #[test]
    fn setup_status_output_serializes_camel_case_keys() {
        let output = SetupStatusOutput {
            state: json!({"Completed": null}),
            session_id: Some("session-1".to_string()),
            next_step_hint: "completed".to_string(),
            profile: "peerA".to_string(),
            clipboard_mode: "full".to_string(),
            device_name: "Peer A".to_string(),
            peer_id: "peer-a".to_string(),
        };

        let value = serde_json::to_value(output).expect("status output should serialize");
        assert_eq!(value["sessionId"], "session-1");
        assert_eq!(value["nextStepHint"], "completed");
        assert_eq!(value["clipboardMode"], "full");
        assert_eq!(value["deviceName"], "Peer A");
        assert_eq!(value["peerId"], "peer-a");
        assert!(value.get("session_id").is_none());
    }

    #[test]
    fn detects_setup_variant_and_error_code() {
        let state = json!({
            "JoinSpaceInputPassphrase": {
                "error": "PassphraseInvalidOrMismatch"
            }
        });

        let variant = SetupVariant::from_state_value(&state);
        assert!(matches!(variant, SetupVariant::JoinSpaceInputPassphrase));
        // Inline error extraction (same logic as parse_setup_state)
        let error_code: Option<&str> = state
            .get("JoinSpaceInputPassphrase")
            .and_then(|p| p.get("error"))
            .and_then(|e| e.as_str());
        assert_eq!(error_code, Some("PassphraseInvalidOrMismatch"));
    }

    #[test]
    fn filters_out_already_paired_peers_before_selection() {
        let peers = vec![
            PeerSnapshotDto {
                peer_id: "peer-b".to_string(),
                device_name: Some("Peer B".to_string()),
                addresses: vec![],
                is_paired: true,
                connected: true,
                pairing_state: "Paired".to_string(),
            },
            PeerSnapshotDto {
                peer_id: "peer-a".to_string(),
                device_name: Some("Peer A".to_string()),
                addresses: vec![],
                is_paired: false,
                connected: true,
                pairing_state: "Discovered".to_string(),
            },
        ];

        let filtered = filter_joinable_peers(peers);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].peer_id, "peer-a");
    }

    #[test]
    fn format_selected_peer_label_uses_peer_suffix_when_available() {
        let dto = SetupStateResponseDto {
            state: json!("Completed"),
            session_id: Some("session-1".to_string()),
            next_step_hint: "host-confirm-peer".to_string(),
            profile: "peerA".to_string(),
            clipboard_mode: "full".to_string(),
            device_name: "Peer A".to_string(),
            peer_id: "peer-a".to_string(),
            selected_peer_id: Some("12D3KooWABCDEFGH".to_string()),
            selected_peer_name: Some("Peer B".to_string()),
            has_completed: true,
        };
        let parsed = parse_setup_state(&dto);

        assert_eq!(
            parsed.selected_peer_label,
            Some("Peer B (ABCDEFGH)".to_string())
        );
    }
}
