//! `uniclip join` — join a space via daemon HTTP API. The route is chosen by
//! explicit intent, not by the device's setup state.
//!
//! * Default (no `--switch`) → calls `POST /v2/setup/redeem` (joiner side of
//!   Slice 1 pairing). A single blocking RPC — the daemon drives the dial/wait
//!   loop internally, so we simply await the result. Safe to run when already
//!   in the *same* space: stale rows are replaced in the new handshake (issue
//!   #1023), so this is also the re-pair-after-unpair path.
//! * `--switch` → calls `POST /v2/setup/switch-space`, which drives the
//!   4-phase re-encryption migration internally for moving to a *different*
//!   space. This is destructive, so we confirm first (unless `--yes`) and show
//!   a spinner while it runs.
//!
//! Routing on explicit intent (rather than a local setup-state check) keeps
//! same-space re-pair non-destructive: a set-up device re-joining its own
//! space must redeem, not migrate. Both paths handle Ctrl+C for clean
//! cancellation.

use serde::Serialize;
use tokio::select;
use tokio::signal;

use uc_daemon_client::{
    ControlLeaseGuard, DaemonClientContext, DaemonRequestError, DaemonService, HttpWsDaemonService,
};
use uc_daemon_contract::api::dto::settings::{GeneralSettingsPatchDto, SettingsPatchDto};
use uc_daemon_contract::api::dto::v2::setup::{
    JoinSpaceResponse, RedeemRequest, SwitchSpaceRequest,
};

use crate::commands::app_session::{
    connect_setup_facade_with_lease, connect_with_lease, default_device_name,
    reconnect_setup_facade_with_lease,
};
use crate::exit_codes;
use crate::ui;

const EXIT_SIGINT: i32 = 130;
const JOIN_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);
const JOIN_RECONNECT_MAX_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

fn next_reconnect_delay(current: std::time::Duration) -> std::time::Duration {
    current.saturating_mul(2).min(JOIN_RECONNECT_MAX_INTERVAL)
}

/// Number of base32 chars in an invitation-code body (the `XXXX-XXXX`
/// shape carries 8 chars plus one middle hyphen).
const CODE_BODY_LEN: usize = 8;

/// Fold a typed invitation code into the canonical `XXXX-XXXX` form the
/// sponsor minted and published.
///
/// Codes use an all-uppercase Crockford base32 alphabet and are compared
/// byte-for-byte (rendezvous lookup key + handshake), so loose typing
/// would otherwise fail to pair. We drop separators (whitespace, hyphens),
/// uppercase, and — when exactly the 8-char body remains — re-insert the
/// single middle hyphen. Anything else is passed through compacted and
/// uppercased so a genuinely malformed code still surfaces a real
/// resolution error instead of being silently "fixed".
fn normalize_invitation_code(raw: &str) -> String {
    let compact: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect::<String>()
        .to_ascii_uppercase();
    if compact.is_ascii() && compact.len() == CODE_BODY_LEN {
        let mid = CODE_BODY_LEN / 2;
        format!("{}-{}", &compact[..mid], &compact[mid..])
    } else {
        compact
    }
}

pub struct JoinArgs {
    pub code: Option<String>,
    pub passphrase: Option<String>,
    pub device_name: Option<String>,
    pub switch: bool,
    pub yes: bool,
    pub preserve_unreadable_history: bool,
    pub no_wait: bool,
}

pub async fn run(args: JoinArgs, json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Join a space");
    }

    // Collect invitation code: --code wins; otherwise prompt. Shared by both
    // the redeem and switch paths (both are rendezvous lookup keys, so both
    // get the same byte-for-byte normalization).
    let code_str = match args.code {
        Some(c) if !c.trim().is_empty() => normalize_invitation_code(&c),
        Some(_) => {
            ui::error("--code is empty");
            return exit_codes::EXIT_ERROR;
        }
        None => match ui::password("Invitation code") {
            Ok(c) if !c.trim().is_empty() => normalize_invitation_code(&c),
            Ok(_) => {
                ui::error("Invitation code cannot be empty");
                return exit_codes::EXIT_ERROR;
            }
            Err(e) => {
                ui::error(&e);
                return exit_codes::EXIT_ERROR;
            }
        },
    };

    // Collect passphrase (single entry, no confirmation). Shared by both paths.
    let passphrase_str = match args.passphrase {
        Some(p) if !p.trim().is_empty() => p,
        Some(_) => {
            ui::error("--passphrase is empty");
            return exit_codes::EXIT_ERROR;
        }
        None => match ui::password("Space passphrase") {
            Ok(p) if !p.trim().is_empty() => p,
            Ok(_) => {
                ui::error("Passphrase cannot be empty");
                return exit_codes::EXIT_ERROR;
            }
            Err(e) => {
                ui::error(&e);
                return exit_codes::EXIT_ERROR;
            }
        },
    };

    // Route by explicit intent, not by setup state. Without `--switch` we
    // always take the non-destructive redeem path — which doubles as the
    // re-pair-after-unpair path (issue #1023), since redeeming an invitation
    // for the space this device is already in just replaces stale rows. Only
    // `--switch` opts into the destructive re-encryption migration to a
    // different space.
    if args.switch {
        if args.device_name.is_some() {
            ui::warn("--device-name is ignored when switching spaces");
        }
        return run_switch(
            code_str,
            passphrase_str,
            args.yes,
            args.preserve_unreadable_history,
            args.no_wait,
            json,
            verbose,
        )
        .await;
    }

    // Validate that preserve_unreadable_history is only accepted with --switch
    if args.preserve_unreadable_history {
        ui::error("--preserve-unreadable-history requires --switch");
        return exit_codes::EXIT_ERROR;
    }

    run_redeem(
        code_str,
        passphrase_str,
        args.device_name,
        args.no_wait,
        json,
        verbose,
    )
    .await
}

#[derive(Serialize)]
struct JoinErrorOutput {
    ok: bool,
    code: String,
    message: String,
}

#[derive(Serialize)]
struct JoinSuccessOutput<'a> {
    ok: bool,
    status: &'static str,
    join_id: &'a str,
    space_id: &'a str,
    self_device_id: &'a str,
    self_device_name: Option<&'a str>,
    self_fingerprint: &'a str,
    sponsor_device_id: &'a str,
    sponsor_fingerprint: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    migrated_records: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preserved_unreadable_records: Option<u64>,
}

#[derive(Serialize)]
struct JoinPendingOutput<'a> {
    ok: bool,
    status: &'static str,
    join_id: &'a str,
    target_space_id: Option<&'a str>,
    sponsor_device_id: Option<&'a str>,
    sponsor_fingerprint: Option<&'a str>,
    cancel_requested: bool,
}

#[derive(Serialize)]
struct JoinRejectedOutput<'a> {
    ok: bool,
    status: &'static str,
    join_id: &'a str,
    reason: &'a str,
}

#[derive(Serialize)]
struct JoinNoneOutput {
    ok: bool,
    status: &'static str,
}

fn emit_join_error(json: bool, code: &str, message: impl Into<String>, exit_code: i32) -> i32 {
    let message = message.into();
    if json {
        crate::output::emit_json_with_code(
            &JoinErrorOutput {
                ok: false,
                code: code.to_string(),
                message,
            },
            "join error",
            exit_code,
        )
    } else {
        ui::error(&message);
        exit_code
    }
}

#[derive(Debug, PartialEq, Eq)]
enum JoinPollDecision {
    Continue,
    Finished(JoinSpaceResponse),
    Missing,
    Replaced,
}

fn join_id(response: &JoinSpaceResponse) -> &str {
    match response {
        JoinSpaceResponse::Active { join_id, .. }
        | JoinSpaceResponse::Pending { join_id, .. }
        | JoinSpaceResponse::Rejected { join_id, .. } => join_id,
    }
}

fn should_wait_for_join(response: &JoinSpaceResponse, no_wait: bool) -> bool {
    !no_wait && matches!(response, JoinSpaceResponse::Pending { .. })
}

fn join_poll_decision(
    expected_join_id: &str,
    current: Option<JoinSpaceResponse>,
) -> JoinPollDecision {
    let Some(current) = current else {
        return JoinPollDecision::Missing;
    };
    if join_id(&current) != expected_join_id {
        return JoinPollDecision::Replaced;
    }
    if matches!(current, JoinSpaceResponse::Pending { .. }) {
        JoinPollDecision::Continue
    } else {
        JoinPollDecision::Finished(current)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JoinResponseIntent {
    Start,
    Status,
    Cancel,
}

#[derive(Debug, PartialEq, Eq)]
enum JoinCancelDecision<'a> {
    None,
    Cancel(&'a str),
    AlreadyRequested(&'a JoinSpaceResponse),
    Report(&'a JoinSpaceResponse),
}

fn join_cancel_decision(current: Option<&JoinSpaceResponse>) -> JoinCancelDecision<'_> {
    match current {
        Some(
            response @ JoinSpaceResponse::Pending {
                cancel_requested: true,
                ..
            },
        ) => JoinCancelDecision::AlreadyRequested(response),
        Some(JoinSpaceResponse::Pending { join_id, .. }) => JoinCancelDecision::Cancel(join_id),
        Some(response) => JoinCancelDecision::Report(response),
        None => JoinCancelDecision::None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JoinResponseOutcome {
    ok: bool,
    exit_code: i32,
}

fn join_response_outcome(
    response: &JoinSpaceResponse,
    intent: JoinResponseIntent,
) -> JoinResponseOutcome {
    let ok = match intent {
        JoinResponseIntent::Start => !matches!(response, JoinSpaceResponse::Rejected { .. }),
        JoinResponseIntent::Status => true,
        JoinResponseIntent::Cancel => matches!(
            response,
            JoinSpaceResponse::Pending {
                cancel_requested: true,
                ..
            } | JoinSpaceResponse::Rejected {
                reason:
                    uc_daemon_contract::api::dto::v2::setup::JoinSpaceRejectionReason::Cancelled,
                ..
            }
        ),
    };
    JoinResponseOutcome {
        ok,
        exit_code: if ok {
            exit_codes::EXIT_SUCCESS
        } else {
            exit_codes::EXIT_ERROR
        },
    }
}

fn render_join_response(
    response: &JoinSpaceResponse,
    spinner: &indicatif::ProgressBar,
    completed_message: &str,
    device_name: Option<&str>,
    json: bool,
    intent: JoinResponseIntent,
) -> i32 {
    let outcome = join_response_outcome(response, intent);
    match response {
        JoinSpaceResponse::Active {
            join_id,
            joined_space,
        } => {
            if json {
                spinner.finish_and_clear();
                crate::output::emit_json_with_code(
                    &JoinSuccessOutput {
                        ok: outcome.ok,
                        status: "active",
                        join_id,
                        space_id: &joined_space.space_id,
                        self_device_id: &joined_space.self_device_id,
                        self_device_name: device_name,
                        self_fingerprint: &joined_space.self_identity_fingerprint,
                        sponsor_device_id: &joined_space.sponsor_device_id,
                        sponsor_fingerprint: &joined_space.sponsor_identity_fingerprint,
                        migrated_records: joined_space.migrated_records,
                        preserved_unreadable_records: joined_space.preserved_unreadable_records,
                    },
                    "join response",
                    outcome.exit_code,
                )
            } else {
                if outcome.ok {
                    ui::spinner_finish_success(spinner, completed_message);
                } else {
                    ui::spinner_finish_error(spinner, "Join request was not cancelled");
                }
                ui::info("join_id", join_id);
                ui::info("space_id", &joined_space.space_id);
                ui::info("self_device_id", &joined_space.self_device_id);
                if let Some(device_name) = device_name {
                    ui::info("self_device_name", device_name);
                }
                ui::info("self_fingerprint", &joined_space.self_identity_fingerprint);
                ui::info("sponsor_device_id", &joined_space.sponsor_device_id);
                ui::info(
                    "sponsor_fingerprint",
                    &joined_space.sponsor_identity_fingerprint,
                );
                if let Some(migrated_records) = joined_space.migrated_records {
                    ui::info("migrated_records", &migrated_records.to_string());
                }
                if let Some(preserved) = joined_space.preserved_unreadable_records {
                    ui::info("preserved_unreadable_records", &preserved.to_string());
                }
                outcome.exit_code
            }
        }
        JoinSpaceResponse::Pending {
            join_id,
            target_space_id,
            sponsor_device_id,
            sponsor_identity_fingerprint,
            cancel_requested,
        } => {
            if json {
                spinner.finish_and_clear();
                crate::output::emit_json_with_code(
                    &JoinPendingOutput {
                        ok: outcome.ok,
                        status: "pending",
                        join_id,
                        target_space_id: target_space_id.as_deref(),
                        sponsor_device_id: sponsor_device_id.as_deref(),
                        sponsor_fingerprint: sponsor_identity_fingerprint.as_deref(),
                        cancel_requested: *cancel_requested,
                    },
                    "join response",
                    outcome.exit_code,
                )
            } else {
                if outcome.ok {
                    ui::spinner_finish_success(
                        spinner,
                        if intent == JoinResponseIntent::Cancel {
                            "Join cancellation requested"
                        } else {
                            "Join request is pending"
                        },
                    );
                } else {
                    ui::spinner_finish_error(spinner, "Join request was not cancelled");
                }
                ui::info("join_id", join_id);
                if let Some(space_id) = target_space_id {
                    ui::info("target_space_id", space_id);
                }
                if let Some(sponsor_device_id) = sponsor_device_id {
                    ui::info("sponsor_device_id", sponsor_device_id);
                }
                if let Some(sponsor_fingerprint) = sponsor_identity_fingerprint {
                    ui::info("sponsor_fingerprint", sponsor_fingerprint);
                }
                ui::info("cancel_requested", &cancel_requested.to_string());
                outcome.exit_code
            }
        }
        JoinSpaceResponse::Rejected { join_id, reason } => {
            let reason = match reason {
                uc_daemon_contract::api::dto::v2::setup::JoinSpaceRejectionReason::InvitationUnavailable => "invitation_unavailable",
                uc_daemon_contract::api::dto::v2::setup::JoinSpaceRejectionReason::AuthenticationRejected => "authentication_rejected",
                uc_daemon_contract::api::dto::v2::setup::JoinSpaceRejectionReason::IdentityConflict => "identity_conflict",
                uc_daemon_contract::api::dto::v2::setup::JoinSpaceRejectionReason::BaseHistoryChanged => "base_history_changed",
                uc_daemon_contract::api::dto::v2::setup::JoinSpaceRejectionReason::JoinerHistoryAhead => "joiner_history_ahead",
                uc_daemon_contract::api::dto::v2::setup::JoinSpaceRejectionReason::HistoryConflict => "history_conflict",
                uc_daemon_contract::api::dto::v2::setup::JoinSpaceRejectionReason::PeerUpgradeRequired => "peer_upgrade_required",
                uc_daemon_contract::api::dto::v2::setup::JoinSpaceRejectionReason::Cancelled => "cancelled",
                uc_daemon_contract::api::dto::v2::setup::JoinSpaceRejectionReason::RemovedBeforeActivation => "removed_before_activation",
            };
            if json {
                spinner.finish_and_clear();
                crate::output::emit_json_with_code(
                    &JoinRejectedOutput {
                        ok: outcome.ok,
                        status: "rejected",
                        join_id,
                        reason,
                    },
                    "join response",
                    outcome.exit_code,
                )
            } else {
                match (intent, outcome.ok) {
                    (JoinResponseIntent::Cancel, true) => {
                        ui::spinner_finish_success(spinner, "Join cancelled");
                    }
                    (JoinResponseIntent::Status, true) => {
                        spinner.finish_and_clear();
                        ui::info("status", "rejected");
                    }
                    _ => ui::spinner_finish_error(spinner, "Join request was rejected"),
                }
                ui::info("join_id", join_id);
                ui::info("reason", reason);
                outcome.exit_code
            }
        }
    }
}

fn render_no_current_join(json: bool, message: &str) -> i32 {
    if json {
        crate::output::emit_json(
            &JoinNoneOutput {
                ok: true,
                status: "none",
            },
            "join status",
        )
    } else {
        ui::info("status", "none");
        ui::end(message);
        exit_codes::EXIT_SUCCESS
    }
}

struct JoinWaitContext<'a> {
    expected_join_id: String,
    completed_message: &'a str,
    device_name: Option<&'a str>,
    json: bool,
    verbose: bool,
}

async fn wait_for_join(
    mut _lease: ControlLeaseGuard,
    mut service: Box<dyn DaemonService>,
    spinner: &indicatif::ProgressBar,
    context: JoinWaitContext<'_>,
) -> i32 {
    spinner.set_message("Join request pending; waiting for final status...");
    let mut reconnecting = false;
    let mut reconnect_delay = JOIN_POLL_INTERVAL;
    loop {
        select! {
            _ = signal::ctrl_c() => {
                spinner.finish_and_clear();
                return emit_join_error(
                    context.json,
                    "interrupted",
                    "Stopped waiting; join request is still pending.",
                    EXIT_SIGINT,
                );
            }
            _ = tokio::time::sleep(JOIN_POLL_INTERVAL) => {}
        }

        let snapshot = match service.device_trust().await {
            Ok(snapshot) => {
                if reconnecting {
                    spinner.set_message("Join request pending; waiting for final status...");
                    reconnecting = false;
                }
                snapshot
            }
            Err(_) => {
                if !reconnecting {
                    spinner.set_message("Daemon connection interrupted; reconnecting...");
                    reconnecting = true;
                }
                loop {
                    select! {
                        _ = signal::ctrl_c() => {
                            spinner.finish_and_clear();
                            return emit_join_error(
                                context.json,
                                "interrupted",
                                "Stopped waiting; join request is still pending.",
                                EXIT_SIGINT,
                            );
                        }
                        _ = tokio::time::sleep(reconnect_delay) => {}
                    }
                    match reconnect_setup_facade_with_lease(context.verbose).await {
                        Ok((new_lease, new_service)) => {
                            _lease = new_lease;
                            service = new_service;
                            reconnect_delay = JOIN_POLL_INTERVAL;
                            break;
                        }
                        Err(_) => reconnect_delay = next_reconnect_delay(reconnect_delay),
                    }
                }
                continue;
            }
        };

        match join_poll_decision(&context.expected_join_id, snapshot.current_join) {
            JoinPollDecision::Continue => {}
            JoinPollDecision::Finished(response) => {
                return render_join_response(
                    &response,
                    spinner,
                    context.completed_message,
                    context.device_name,
                    context.json,
                    JoinResponseIntent::Start,
                );
            }
            JoinPollDecision::Missing => {
                spinner.finish_and_clear();
                return emit_join_error(
                    context.json,
                    "join_status_missing",
                    "The pending join is no longer available. Run `uniclip join status`.",
                    exit_codes::EXIT_ERROR,
                );
            }
            JoinPollDecision::Replaced => {
                spinner.finish_and_clear();
                return emit_join_error(
                    context.json,
                    "join_replaced",
                    "A newer join request replaced this one. Run `uniclip join status`.",
                    exit_codes::EXIT_ERROR,
                );
            }
        }
    }
}

pub async fn status(json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Join status");
    }
    let (_lease, service) = match connect_setup_facade_with_lease(verbose).await {
        Ok(session) => session,
        Err(code) => return code,
    };
    let snapshot = match service.device_trust().await {
        Ok(snapshot) => snapshot,
        Err(err) => {
            return emit_join_error(
                json,
                "join_status_unavailable",
                format!(
                    "Failed to query join status: {}",
                    crate::commands::daemon_error_message(&err)
                ),
                exit_codes::EXIT_ERROR,
            );
        }
    };
    match snapshot.current_join {
        Some(response) => {
            let spinner = indicatif::ProgressBar::hidden();
            render_join_response(
                &response,
                &spinner,
                "Join completed",
                None,
                json,
                JoinResponseIntent::Status,
            )
        }
        None => render_no_current_join(json, "No current join request."),
    }
}

pub async fn cancel(json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Cancel join");
    }
    let (_lease, service) = match connect_setup_facade_with_lease(verbose).await {
        Ok(session) => session,
        Err(code) => return code,
    };
    let snapshot = match service.device_trust().await {
        Ok(snapshot) => snapshot,
        Err(err) => {
            return emit_join_error(
                json,
                "join_status_unavailable",
                format!(
                    "Failed to query join status: {}",
                    crate::commands::daemon_error_message(&err)
                ),
                exit_codes::EXIT_ERROR,
            );
        }
    };
    let join_id = match join_cancel_decision(snapshot.current_join.as_ref()) {
        JoinCancelDecision::Cancel(join_id) => join_id,
        JoinCancelDecision::AlreadyRequested(response) => {
            let spinner = indicatif::ProgressBar::hidden();
            return render_join_response(
                response,
                &spinner,
                "Join cancellation requested",
                None,
                json,
                JoinResponseIntent::Cancel,
            );
        }
        JoinCancelDecision::Report(response) => {
            let spinner = indicatif::ProgressBar::hidden();
            return render_join_response(
                response,
                &spinner,
                "Join completed",
                None,
                json,
                JoinResponseIntent::Status,
            );
        }
        JoinCancelDecision::None => {
            return render_no_current_join(json, "No pending join request to cancel.")
        }
    };
    let response = match service.cancel_join(&join_id).await {
        Ok(response) => response,
        Err(err) => {
            return emit_join_error(
                json,
                "join_cancel_failed",
                format!(
                    "Failed to cancel join: {}",
                    crate::commands::daemon_error_message(&err)
                ),
                exit_codes::EXIT_ERROR,
            );
        }
    };
    let spinner = indicatif::ProgressBar::hidden();
    render_join_response(
        &response,
        &spinner,
        "Join cancelled",
        None,
        json,
        JoinResponseIntent::Cancel,
    )
}

fn join_error_output(err: &anyhow::Error) -> JoinErrorOutput {
    let request_error = err.downcast_ref::<DaemonRequestError>();
    JoinErrorOutput {
        ok: false,
        code: request_error
            .and_then(DaemonRequestError::code)
            .unwrap_or("unknown")
            .to_string(),
        message: request_error
            .and_then(DaemonRequestError::message)
            .map(str::to_string)
            .unwrap_or_else(|| err.to_string()),
    }
}

fn render_join_error(prefix: &str, err: &anyhow::Error, json: bool) -> i32 {
    if json {
        return crate::output::emit_json_with_code(
            &join_error_output(err),
            "join error response",
            exit_codes::EXIT_ERROR,
        );
    }
    emit_join_error(
        false,
        "join_failed",
        format!("{prefix}: {}", crate::commands::daemon_error_message(err)),
        exit_codes::EXIT_ERROR,
    )
}

/// First-time join: redeem an invitation and adopt the sponsor's space.
async fn run_redeem(
    code_str: String,
    passphrase_str: String,
    device_name_arg: Option<String>,
    no_wait: bool,
    json: bool,
    verbose: bool,
) -> i32 {
    // Determine device name.
    let device_name = device_name_arg
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(default_device_name);
    let device_name = match device_name {
        Some(n) => n,
        None => {
            ui::error("Device name is required (pass --device-name or set a system hostname)");
            return exit_codes::EXIT_ERROR;
        }
    };

    // Ensure daemon is running (no setup gate — we ARE the setup command).
    let (_lease, service) = match connect_setup_facade_with_lease(verbose).await {
        Ok(session) => session,
        Err(code) => return code,
    };

    let ctx = match DaemonClientContext::from_env() {
        Ok(c) => c,
        Err(err) => {
            ui::error(&format!("Failed to build daemon client context: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    // Set device name via settings BEFORE redeem — RedeemRequest has no
    // device_name field; the daemon reads it from persisted settings.
    let patch = SettingsPatchDto {
        general: Some(GeneralSettingsPatchDto {
            device_name: Some(Some(device_name.clone())),
            ..Default::default()
        }),
        ..Default::default()
    };
    if let Err(err) = ctx.settings_client().update_settings(patch).await {
        ui::warn(&format!("Failed to set device name: {err}"));
        // non-fatal — redeem might still work with hostname default
    }

    let spinner = ui::spinner("Dialing sponsor and running handshake...");
    let req = RedeemRequest {
        code: code_str,
        passphrase: passphrase_str,
    };

    let setup_client = ctx.setup_v2_client();
    let redeem_fut = setup_client.redeem_invitation(&req);
    tokio::pin!(redeem_fut);

    select! {
        result = &mut redeem_fut => match result {
            Ok(resp) if should_wait_for_join(&resp, no_wait) => wait_for_join(
                _lease,
                service,
                &spinner,
                JoinWaitContext {
                    expected_join_id: join_id(&resp).to_string(),
                    completed_message: "Joined space",
                    device_name: Some(&device_name),
                    json,
                    verbose,
                },
            ).await,
            Ok(resp) => render_join_response(
                &resp,
                &spinner,
                "Joined space",
                Some(&device_name),
                json,
                JoinResponseIntent::Start,
            ),
            Err(err) => {
                spinner.finish_and_clear();
                render_join_error("Join failed", &err, json)
            }
        },
        _ = signal::ctrl_c() => {
            spinner.finish_and_clear();
            emit_join_error(
                json,
                "interrupted",
                "Stopped waiting; the join operation may still continue in Engine.",
                EXIT_SIGINT,
            )
        }
    }
}

/// Already-set-up device: switch to another sponsor's space, re-encrypting
/// local clipboard history under the new master key (4-phase migration).
///
/// Destructive, so we confirm first unless `--yes` was passed. The daemon
/// drives the migration internally and persists `MigrationStatePort`, so a
/// crash mid-run auto-resumes on the next `uniclip` invocation.
async fn run_switch(
    code_str: String,
    new_passphrase: String,
    yes: bool,
    preserve_unreadable_history: bool,
    no_wait: bool,
    json: bool,
    verbose: bool,
) -> i32 {
    if !json {
        ui::warn(
            "This device is already in a space. Switching will re-encrypt all local \
             clipboard history under the new space's master key.",
        );
    }
    if !yes {
        match ui::confirm("Switch to the new space now?", false) {
            Ok(true) => {}
            Ok(false) => {
                ui::end("Cancelled — staying in the current space.");
                return exit_codes::EXIT_SUCCESS;
            }
            Err(e) => {
                ui::error(&e);
                return exit_codes::EXIT_ERROR;
            }
        }
    }

    // Device IS set up → normal connect path (vs. redeem's setup-gated spawn).
    let (_lease, ctx) = match connect_with_lease(verbose).await {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let service: Box<dyn DaemonService> = Box::new(HttpWsDaemonService::new(ctx.clone()));

    let spinner = ui::spinner(
        "Migrating local clipboard history to the new space (4 phases \u{2014} this may take a while)...",
    );

    let req = SwitchSpaceRequest {
        code: code_str,
        new_passphrase,
        preserve_unreadable_history,
    };

    let setup_client = ctx.setup_v2_client();
    let switch_fut = setup_client.switch_space(&req);
    tokio::pin!(switch_fut);

    select! {
        result = &mut switch_fut => match result {
            Ok(resp) if should_wait_for_join(&resp, no_wait) => wait_for_join(
                _lease,
                service,
                &spinner,
                JoinWaitContext {
                    expected_join_id: join_id(&resp).to_string(),
                    completed_message: "Switched space",
                    device_name: None,
                    json,
                    verbose,
                },
            ).await,
            Ok(resp) => render_join_response(
                &resp,
                &spinner,
                "Switched space",
                None,
                json,
                JoinResponseIntent::Start,
            ),
            Err(err) => {
                spinner.finish_and_clear();
                render_join_error("Switch-space failed", &err, json)
            }
        },
        _ = signal::ctrl_c() => {
            spinner.finish_and_clear();
            emit_join_error(
                json,
                "interrupted",
                "Stopped waiting; the space migration may still continue in Engine.",
                EXIT_SIGINT,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        join_cancel_decision, join_error_output, join_poll_decision, join_response_outcome,
        next_reconnect_delay, normalize_invitation_code, should_wait_for_join, JoinCancelDecision,
        JoinPendingOutput, JoinPollDecision, JoinRejectedOutput, JoinResponseIntent,
        JOIN_POLL_INTERVAL, JOIN_RECONNECT_MAX_INTERVAL,
    };
    use crate::exit_codes;
    use reqwest::StatusCode;
    use uc_daemon_client::DaemonRequestError;
    use uc_daemon_contract::api::dto::v2::setup::{
        JoinSpaceRejectionReason, JoinSpaceResponse, JoinedSpaceResponse,
    };

    #[test]
    fn reconnect_delay_doubles_until_the_maximum() {
        assert_eq!(
            next_reconnect_delay(JOIN_POLL_INTERVAL),
            JOIN_POLL_INTERVAL * 2
        );
        assert_eq!(
            next_reconnect_delay(JOIN_RECONNECT_MAX_INTERVAL),
            JOIN_RECONNECT_MAX_INTERVAL
        );
    }

    #[test]
    fn cancel_reports_active_and_rejected_current_join_states() {
        let active = JoinSpaceResponse::Active {
            join_id: "join-active".to_string(),
            joined_space: JoinedSpaceResponse {
                sponsor_device_id: "sponsor".to_string(),
                sponsor_identity_fingerprint: "sponsor-fingerprint".to_string(),
                space_id: "space".to_string(),
                self_device_id: "self".to_string(),
                self_identity_fingerprint: "self-fingerprint".to_string(),
                migrated_records: None,
                preserved_unreadable_records: None,
            },
        };
        let rejected = JoinSpaceResponse::Rejected {
            join_id: "join-rejected".to_string(),
            reason: JoinSpaceRejectionReason::AuthenticationRejected,
        };

        assert_eq!(
            join_cancel_decision(Some(&active)),
            JoinCancelDecision::Report(&active)
        );
        assert_eq!(
            join_cancel_decision(Some(&rejected)),
            JoinCancelDecision::Report(&rejected)
        );
    }

    #[test]
    fn cancel_does_not_resubmit_when_cancellation_is_already_requested() {
        let pending = JoinSpaceResponse::Pending {
            join_id: "join-pending".to_string(),
            target_space_id: None,
            sponsor_device_id: None,
            sponsor_identity_fingerprint: None,
            cancel_requested: true,
        };

        assert_eq!(
            join_cancel_decision(Some(&pending)),
            JoinCancelDecision::AlreadyRequested(&pending)
        );
    }

    #[test]
    fn start_outcome_distinguishes_completed_pending_and_rejected_admission() {
        let active = JoinSpaceResponse::Active {
            join_id: "join-active".to_string(),
            joined_space: JoinedSpaceResponse {
                sponsor_device_id: "sponsor".to_string(),
                sponsor_identity_fingerprint: "sponsor-fingerprint".to_string(),
                space_id: "space".to_string(),
                self_device_id: "self".to_string(),
                self_identity_fingerprint: "self-fingerprint".to_string(),
                migrated_records: None,
                preserved_unreadable_records: None,
            },
        };
        let pending = JoinSpaceResponse::Pending {
            join_id: "join-pending".to_string(),
            target_space_id: None,
            sponsor_device_id: None,
            sponsor_identity_fingerprint: None,
            cancel_requested: false,
        };
        let rejected = JoinSpaceResponse::Rejected {
            join_id: "join-rejected".to_string(),
            reason: JoinSpaceRejectionReason::AuthenticationRejected,
        };

        assert_eq!(
            join_response_outcome(&active, JoinResponseIntent::Start).exit_code,
            exit_codes::EXIT_SUCCESS
        );
        assert_eq!(
            join_response_outcome(&pending, JoinResponseIntent::Start).exit_code,
            exit_codes::EXIT_SUCCESS
        );
        assert_eq!(
            join_response_outcome(&rejected, JoinResponseIntent::Start).exit_code,
            exit_codes::EXIT_ERROR
        );
    }

    #[test]
    fn status_query_treats_rejected_join_as_observed_state() {
        let rejected = JoinSpaceResponse::Rejected {
            join_id: "join-rejected".to_string(),
            reason: JoinSpaceRejectionReason::AuthenticationRejected,
        };

        let outcome = join_response_outcome(&rejected, JoinResponseIntent::Status);

        assert!(outcome.ok);
        assert_eq!(outcome.exit_code, exit_codes::EXIT_SUCCESS);
    }

    #[test]
    fn cancel_succeeds_only_after_engine_accepts_cancellation() {
        let cancelled = JoinSpaceResponse::Rejected {
            join_id: "join-cancelled".to_string(),
            reason: JoinSpaceRejectionReason::Cancelled,
        };
        let active = JoinSpaceResponse::Active {
            join_id: "join-active".to_string(),
            joined_space: JoinedSpaceResponse {
                sponsor_device_id: "sponsor".to_string(),
                sponsor_identity_fingerprint: "sponsor-fingerprint".to_string(),
                space_id: "space".to_string(),
                self_device_id: "self".to_string(),
                self_identity_fingerprint: "self-fingerprint".to_string(),
                migrated_records: None,
                preserved_unreadable_records: None,
            },
        };

        let cancelled_outcome = join_response_outcome(&cancelled, JoinResponseIntent::Cancel);
        let active_outcome = join_response_outcome(&active, JoinResponseIntent::Cancel);

        assert!(cancelled_outcome.ok);
        assert_eq!(cancelled_outcome.exit_code, exit_codes::EXIT_SUCCESS);
        assert!(!active_outcome.ok);
        assert_eq!(active_outcome.exit_code, exit_codes::EXIT_ERROR);
    }

    #[test]
    fn join_json_shapes_use_stable_snake_case_fields() {
        let pending = serde_json::to_value(JoinPendingOutput {
            ok: true,
            status: "pending",
            join_id: "join-1",
            target_space_id: Some("space-1"),
            sponsor_device_id: Some("device-a"),
            sponsor_fingerprint: Some("fingerprint-a"),
            cancel_requested: false,
        })
        .expect("serialize pending join");
        assert_eq!(pending["join_id"], "join-1");
        assert_eq!(pending["target_space_id"], "space-1");
        assert!(pending.get("joinId").is_none());

        let rejected = serde_json::to_value(JoinRejectedOutput {
            ok: false,
            status: "rejected",
            join_id: "join-2",
            reason: "cancelled",
        })
        .expect("serialize rejected join");
        assert_eq!(rejected["reason"], "cancelled");
    }

    #[test]
    fn no_wait_only_detaches_from_pending_join() {
        let pending = JoinSpaceResponse::Pending {
            join_id: "join-pending".to_string(),
            target_space_id: None,
            sponsor_device_id: None,
            sponsor_identity_fingerprint: None,
            cancel_requested: false,
        };
        assert!(should_wait_for_join(&pending, false));
        assert!(!should_wait_for_join(&pending, true));

        let rejected = JoinSpaceResponse::Rejected {
            join_id: "join-rejected".to_string(),
            reason: JoinSpaceRejectionReason::AuthenticationRejected,
        };
        assert!(!should_wait_for_join(&rejected, false));
    }

    #[test]
    fn polling_never_attaches_to_a_different_join_attempt() {
        let same_pending = JoinSpaceResponse::Pending {
            join_id: "join-1".to_string(),
            target_space_id: None,
            sponsor_device_id: None,
            sponsor_identity_fingerprint: None,
            cancel_requested: false,
        };
        assert_eq!(
            join_poll_decision("join-1", Some(same_pending)),
            JoinPollDecision::Continue
        );

        let replaced = JoinSpaceResponse::Pending {
            join_id: "join-2".to_string(),
            target_space_id: None,
            sponsor_device_id: None,
            sponsor_identity_fingerprint: None,
            cancel_requested: false,
        };
        assert_eq!(
            join_poll_decision("join-1", Some(replaced)),
            JoinPollDecision::Replaced
        );
        assert_eq!(
            join_poll_decision("join-1", None),
            JoinPollDecision::Missing
        );
    }

    #[test]
    fn already_canonical_code_is_unchanged() {
        assert_eq!(normalize_invitation_code("ABCD-1234"), "ABCD-1234");
    }

    #[test]
    fn lowercase_is_uppercased() {
        assert_eq!(normalize_invitation_code("abcd-1234"), "ABCD-1234");
    }

    #[test]
    fn hyphenless_eight_chars_get_canonical_hyphen() {
        assert_eq!(normalize_invitation_code("abcd1234"), "ABCD-1234");
        assert_eq!(normalize_invitation_code("ABCD1234"), "ABCD-1234");
    }

    #[test]
    fn json_error_preserves_daemon_error_code() {
        let err = anyhow::Error::new(DaemonRequestError::Status {
            path: "/v2/setup/redeem".to_string(),
            status: StatusCode::CONFLICT,
            code: Some("sponsor_upgrade_required".to_string()),
            message: "Sponsor must be upgraded".to_string(),
        });

        let output = join_error_output(&err);
        let value = serde_json::to_value(output).expect("serialize join error");

        assert_eq!(value["ok"], false);
        assert_eq!(value["code"], "sponsor_upgrade_required");
        assert_eq!(value["message"], "Sponsor must be upgraded");
    }

    #[test]
    fn surrounding_and_inner_whitespace_is_dropped() {
        assert_eq!(normalize_invitation_code("  abcd 1234 "), "ABCD-1234");
        assert_eq!(normalize_invitation_code("ABCD - 1234"), "ABCD-1234");
    }

    #[test]
    fn malformed_length_is_passed_through_compacted() {
        // Not 8 body chars → no hyphen reconstruction, but still
        // separator-stripped + uppercased so resolution fails on the
        // real value rather than a half-normalised one.
        assert_eq!(normalize_invitation_code("abc123"), "ABC123");
        assert_eq!(normalize_invitation_code("abcde-12345"), "ABCDE12345");
    }

    #[test]
    fn non_ascii_input_is_passed_through_without_slicing() {
        // Non-ASCII means the `is_ascii()` guard skips hyphen
        // reconstruction (and byte-slicing), so we never panic on a char
        // boundary. ASCII letters still uppercase; `é` is left as-is.
        assert_eq!(normalize_invitation_code("abcdé123"), "ABCDé123");
    }
}
