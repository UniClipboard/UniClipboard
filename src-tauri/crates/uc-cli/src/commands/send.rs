//! `uniclip send` — one-shot clipboard dispatch with optional resend.
//!
//! Self-contained direct-mode command (no daemon), mirroring the `init` /
//! `invite` / `join` / `members` pattern. Builds a `SpaceSetupAssembly`,
//! resumes the cached session, refreshes presence so dispatch can route to
//! online peers, then either:
//!
//! * **New entry** — wraps the user-supplied text as a single-text-rep
//!   `SystemClipboardSnapshot` and fans it out via
//!   `AppFacade::dispatch_clipboard_snapshot`.
//! * **Resend** — looks up a previously captured entry and re-fans it out
//!   via `AppFacade::resend_entry`. Reconstructed snapshots travel the
//!   same publish + V3 envelope + dispatch path; failure modes are
//!   surfaced as discrete CLI errors.
//!
//! `--peer <DEVICE-ID>` (repeatable) restricts fan-out to listed devices
//! in either mode. Without it, new-entry mode targets all online peers
//! and resend targets the `trusted_peer \ (Delivered ∪ Duplicate)` diff.
//!
//! Still doesn't read the system clipboard — bootstrap sets
//! `UC_DISABLE_SYSTEM_CLIPBOARD=1` so non-bundled CLI runs don't panic
//! in `+[NSPasteboard generalPasteboard]`. The daemon owns the OS
//! clipboard in production.

use std::io::Read;

use serde::Serialize;

use uc_application::facade::space_setup::TryResumeSessionError;
use uc_application::facade::{
    ClipboardSyncError, DispatchEntryOutcome, DispatchEntryPerTarget, NotResendableReason,
    ResendEntryCommand, ResendEntryError, ResendReport,
};
use uc_core::ids::{DeviceId, EntryId, FormatId, RepresentationId};
use uc_core::ports::DispatchAck;
use uc_core::{
    ClipboardChangeOrigin, MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot,
};

use crate::commands::app_session::{build_app_session, refuse_if_daemon_running};
use crate::exit_codes;
use crate::ui;

pub struct SendArgs {
    /// Plaintext to dispatch in **new-entry** mode. When `None`, the
    /// command reads from stdin until EOF — handy for
    /// `echo hi | uniclip send` and the dual-profile test recipe.
    /// Ignored when `resend` is set (clap enforces mutual exclusion at
    /// the parser layer).
    pub text: Option<String>,
    /// Entry id to **resend**. When set, the command pulls the original
    /// snapshot from local storage via `AppFacade::resend_entry`.
    pub resend: Option<String>,
    /// Optional list of target device IDs. Empty vec means "no filter"
    /// (full fan-out for new entry; derived diff for resend).
    pub peers: Vec<String>,
}

pub async fn run(args: SendArgs, json: bool, verbose: bool) -> i32 {
    let mode = if args.resend.is_some() {
        SendMode::Resend
    } else {
        SendMode::New
    };

    if !json {
        ui::header(mode.header());
    }

    if let Err(code) = refuse_if_daemon_running().await {
        return code;
    }

    // Defense in depth: clap already declares `text` conflicts with
    // `--resend`, but if both leaked through (e.g. via a future refactor),
    // bail out before touching stdin / facades.
    if args.resend.is_some() && args.text.is_some() {
        ui::error("--resend cannot be combined with positional text.");
        return exit_codes::EXIT_ERROR;
    }

    // For new-entry mode read text/stdin up front so stdin errors surface
    // before we spin a session up. Resend mode never reads stdin.
    let plaintext = match mode {
        SendMode::Resend => None,
        SendMode::New => match read_plaintext(args.text) {
            Ok(text) if text.is_empty() => {
                ui::error("Empty plaintext — nothing to send.");
                return exit_codes::EXIT_ERROR;
            }
            Ok(text) => Some(text),
            Err(msg) => {
                ui::error(&msg);
                return exit_codes::EXIT_ERROR;
            }
        },
    };

    let target_filter: Option<Vec<DeviceId>> = if args.peers.is_empty() {
        None
    } else {
        Some(args.peers.iter().map(DeviceId::new).collect())
    };

    let cli = match build_app_session(verbose).await {
        Ok(bundle) => bundle,
        Err(code) => return code,
    };

    let resume_spinner = ui::spinner("Resuming space session...");
    match cli.app_facade().try_resume_session().await {
        Ok(true) => ui::spinner_finish_success(&resume_spinner, "Session resumed"),
        Ok(false) => {
            ui::spinner_finish_error(
                &resume_spinner,
                "No space on this profile — run `init` or `join` first.",
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
                "Keychain cannot silently unlock this space.",
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

    // Refresh presence so `dispatch_entry`'s Online-only filter sees the
    // current peer state instead of stale `Unknown`. Same pattern as the
    // `members` command.
    let probe_spinner = ui::spinner("Probing paired peers...");
    match cli.app_facade().refresh_presence().await {
        Ok(report) => ui::spinner_finish_success(
            &probe_spinner,
            &format!(
                "Probed {} peer(s): {} online, {} offline, {} error(s)",
                report.total,
                report.online,
                report.offline,
                report.errors.len()
            ),
        ),
        Err(err) => ui::spinner_finish_error(
            &probe_spinner,
            &format!("Probe round failed: {err} (proceeding with last-known state)"),
        ),
    }

    // Branch on mode. Each branch builds the same `SendOutcomeView` so
    // human / JSON rendering is shared below.
    let view = match mode {
        SendMode::New => {
            // `unwrap` is sound: New branch always populates `plaintext`
            // above (or already returned with EXIT_ERROR).
            let plaintext = plaintext.expect("plaintext populated in new-entry mode");

            // Phase 3 · T9: wrap the CLI text into a single-representation
            // `SystemClipboardSnapshot` with `text/plain` mime — same shape
            // the daemon would emit if the user copied this text through a
            // real `SystemClipboardPort`. `dispatch_snapshot` handles V3
            // envelope encoding + canonical `snapshot_hash` (which matches
            // the receiver's local `clipboard_event.snapshot_hash` for
            // dedup).
            let snapshot = SystemClipboardSnapshot {
                ts_ms: chrono::Utc::now().timestamp_millis(),
                representations: vec![ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    FormatId::from("text"),
                    Some(MimeType("text/plain".to_string())),
                    plaintext.into_bytes(),
                )],
            };

            let dispatch_spinner = ui::spinner("Dispatching to online peers...");
            let outcome = cli
                .app_facade()
                .dispatch_clipboard_snapshot(
                    snapshot,
                    ClipboardChangeOrigin::LocalCapture,
                    target_filter.clone(),
                )
                .await;

            match outcome {
                Ok(o) => {
                    ui::spinner_finish_success(
                        &dispatch_spinner,
                        &format!(
                            "{} accepted, {} duplicate, {} offline, {} error(s)",
                            o.total_accepted, o.total_duplicate, o.total_offline, o.total_errored
                        ),
                    );
                    SendOutcomeView::New(o)
                }
                Err(err) => {
                    ui::spinner_finish_error(&dispatch_spinner, &render_dispatch_error(&err));
                    cli.shutdown().await;
                    return exit_codes::EXIT_ERROR;
                }
            }
        }
        SendMode::Resend => {
            // `unwrap` is sound: Resend branch only entered when
            // `args.resend.is_some()`.
            let entry_id_str = args.resend.clone().expect("resend id present");
            let cmd = ResendEntryCommand {
                entry_id: EntryId::from(entry_id_str.as_str()),
                target_filter: target_filter.clone(),
            };

            let resend_spinner = ui::spinner("Resending entry to eligible peers...");
            match cli.app_facade().resend_entry(cmd).await {
                Ok(report) => {
                    ui::spinner_finish_success(
                        &resend_spinner,
                        &format!(
                            "{} accepted, {} duplicate, {} offline, {} error(s), {} pending",
                            report.accepted,
                            report.duplicate,
                            report.offline,
                            report.errored,
                            report.pending,
                        ),
                    );
                    SendOutcomeView::Resend {
                        entry_id: entry_id_str,
                        report,
                    }
                }
                Err(err) => {
                    ui::spinner_finish_error(&resend_spinner, &render_resend_error(&err));
                    cli.shutdown().await;
                    return exit_codes::EXIT_ERROR;
                }
            }
        }
    };

    if json {
        let dto = SendOutcomeDto::from_view(&view);
        match serde_json::to_string_pretty(&dto) {
            Ok(s) => println!("{s}"),
            Err(err) => {
                ui::error(&format!("Failed to serialize outcome: {err}"));
                cli.shutdown().await;
                return exit_codes::EXIT_ERROR;
            }
        }
    } else {
        render_human(&view);
    }

    cli.shutdown().await;
    match &view {
        SendOutcomeView::New(o) => {
            if o.total_accepted == 0 && o.total_duplicate == 0 {
                // Nothing actually landed: at least one peer was online but
                // every attempt errored (or no peers paired at all). Surface
                // as non-zero so dual-profile shell harness can detect
                // failure.
                exit_codes::EXIT_ERROR
            } else {
                exit_codes::EXIT_SUCCESS
            }
        }
        SendOutcomeView::Resend { report, .. } => {
            // Resend success criterion mirrors new-entry: at least one
            // delivery materialized (accepted or duplicate-on-peer). All-
            // pending is treated as success because the work has been
            // accepted into the background and will resolve via host events.
            if report.accepted == 0 && report.duplicate == 0 && report.pending == 0 {
                exit_codes::EXIT_ERROR
            } else {
                exit_codes::EXIT_SUCCESS
            }
        }
    }
}

#[derive(Clone, Copy)]
enum SendMode {
    New,
    Resend,
}

impl SendMode {
    fn header(self) -> &'static str {
        match self {
            SendMode::New => "Send clipboard",
            SendMode::Resend => "Resend clipboard entry",
        }
    }
}

fn read_plaintext(arg: Option<String>) -> Result<String, String> {
    if let Some(text) = arg {
        return Ok(text);
    }
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|err| format!("read stdin failed: {err}"))?;
    // Trim a single trailing newline so `echo foo | send` matches `send foo`.
    if buf.ends_with('\n') {
        buf.pop();
        if buf.ends_with('\r') {
            buf.pop();
        }
    }
    Ok(buf)
}

fn render_dispatch_error(err: &ClipboardSyncError) -> String {
    match err {
        ClipboardSyncError::LockedSpace => {
            "Space is locked — unlock or re-init before sending.".to_string()
        }
        ClipboardSyncError::CipherFailure(msg) => format!("Encryption failed: {msg}"),
        ClipboardSyncError::Repository(msg) => format!("Peer address lookup failed: {msg}"),
    }
}

fn render_resend_error(err: &ResendEntryError) -> String {
    match err {
        ResendEntryError::EntryNotFound(id) => {
            format!("Entry {id} not found in local storage.")
        }
        ResendEntryError::EntryNotResendable { entry_id, reason } => match reason {
            NotResendableReason::RemoteOrigin => format!(
                "Entry {entry_id} originated on a remote peer — resend is only supported for locally captured entries."
            ),
            NotResendableReason::PayloadLost => format!(
                "Entry {entry_id} payload is no longer cached locally — cannot reconstruct snapshot."
            ),
        },
        ResendEntryError::TargetNotTrusted(d) => format!(
            "Device {d} is not a trusted peer for this space.",
        ),
        ResendEntryError::NoEligibleTargets => {
            "All trusted peers have already received this entry.".to_string()
        }
        ResendEntryError::Storage(msg) => format!("Storage failure: {msg}"),
        ResendEntryError::Dispatch(msg) => format!("Dispatch failure: {msg}"),
    }
}

enum SendOutcomeView {
    New(DispatchEntryOutcome),
    Resend {
        entry_id: String,
        report: ResendReport,
    },
}

fn render_human(view: &SendOutcomeView) {
    match view {
        SendOutcomeView::New(outcome) => {
            ui::bar();
            ui::info("hash", short_hash(&outcome.content_hash));
            if outcome.per_target.is_empty() {
                ui::info("targets", "(none — no online peers)");
            } else {
                for entry in &outcome.per_target {
                    let line = format!(
                        "{} → {}",
                        entry.device_id.as_str(),
                        render_per_target(entry),
                    );
                    ui::info("·", &line);
                }
            }
            ui::bar();
        }
        SendOutcomeView::Resend { entry_id, report } => {
            ui::bar();
            ui::info("entry", entry_id);
            ui::info(
                "summary",
                &format!(
                    "{} accepted, {} duplicate, {} offline, {} error(s), {} pending",
                    report.accepted,
                    report.duplicate,
                    report.offline,
                    report.errored,
                    report.pending,
                ),
            );
            ui::bar();
        }
    }
}

fn render_per_target(entry: &DispatchEntryPerTarget) -> String {
    match &entry.outcome {
        Ok(DispatchAck::Accepted) => "accepted".to_string(),
        Ok(DispatchAck::DuplicateIgnored) => "duplicate (peer already had it)".to_string(),
        Err(reason) => format!("failed: {reason}"),
    }
}

fn short_hash(hash: &str) -> &str {
    if hash.len() > 16 {
        &hash[..16]
    } else {
        hash
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SendOutcomeDto<'a> {
    /// `"new"` (positional text / stdin) or `"resend"` (`--resend`).
    mode: &'static str,
    /// Populated only when `mode == "new"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    content_hash: Option<&'a str>,
    /// Populated only when `mode == "resend"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    entry_id: Option<&'a str>,
    total_accepted: usize,
    total_duplicate: usize,
    total_offline: usize,
    total_errored: usize,
    /// Resend-only: targets accepted into background continuation.
    #[serde(skip_serializing_if = "Option::is_none")]
    total_pending: Option<usize>,
    /// Populated only when `mode == "new"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    at_ms: Option<i64>,
    /// Populated only when `mode == "new"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    per_target: Option<Vec<PerTargetDto<'a>>>,
}

#[derive(Serialize)]
struct PerTargetDto<'a> {
    device_id: &'a str,
    /// `accepted` | `duplicate` | `error`.
    outcome: &'static str,
    /// Populated only when `outcome == "error"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
}

impl<'a> SendOutcomeDto<'a> {
    fn from_view(view: &'a SendOutcomeView) -> Self {
        match view {
            SendOutcomeView::New(o) => Self {
                mode: "new",
                content_hash: Some(&o.content_hash),
                entry_id: None,
                total_accepted: o.total_accepted,
                total_duplicate: o.total_duplicate,
                total_offline: o.total_offline,
                total_errored: o.total_errored,
                total_pending: None,
                at_ms: Some(o.at_ms),
                per_target: Some(
                    o.per_target
                        .iter()
                        .map(|p| match &p.outcome {
                            Ok(DispatchAck::Accepted) => PerTargetDto {
                                device_id: p.device_id.as_str(),
                                outcome: "accepted",
                                error: None,
                            },
                            Ok(DispatchAck::DuplicateIgnored) => PerTargetDto {
                                device_id: p.device_id.as_str(),
                                outcome: "duplicate",
                                error: None,
                            },
                            Err(msg) => PerTargetDto {
                                device_id: p.device_id.as_str(),
                                outcome: "error",
                                error: Some(msg.as_str()),
                            },
                        })
                        .collect(),
                ),
            },
            SendOutcomeView::Resend { entry_id, report } => Self {
                mode: "resend",
                content_hash: None,
                entry_id: Some(entry_id.as_str()),
                total_accepted: report.accepted,
                total_duplicate: report.duplicate,
                total_offline: report.offline,
                total_errored: report.errored,
                total_pending: Some(report.pending),
                at_ms: None,
                per_target: None,
            },
        }
    }
}
