//! `uniclip send` — clipboard dispatch via daemon (text/resend) or in-process
//! (file-send, dev-tools only).
//!
//! ## Text / resend mode (always available)
//!
//! Routes through `connect_or_spawn_oneshot_daemon` → HTTP dispatch.
//!
//! ## File-send mode (`--features dev-tools` only)
//!
//! Builds an in-process `CliAppSession`, publishes the blob to the local
//! iroh-blobs store, dispatches a V3 envelope with a blob-ref extension,
//! then keeps the iroh router alive (passive provider) until Ctrl-C so
//! the receiver has time to fetch. Requires the heavy application +
//! bootstrap stack.

use std::io::Read;
use std::path::PathBuf;

#[cfg(feature = "dev-tools")]
use serde::Serialize;

#[cfg(feature = "dev-tools")]
use uc_application::facade::{
    BlobTransferError, ClipboardSyncError, DispatchEntryPerTarget, PublishBlobPathCommand,
    V3BlobRef,
};
#[cfg(feature = "dev-tools")]
use uc_core::ids::{EntryId, FormatId, RepresentationId};
#[cfg(feature = "dev-tools")]
use uc_core::ports::DispatchAck;
#[cfg(feature = "dev-tools")]
use uc_core::{
    ClipboardChangeOrigin, MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot,
};

use uc_daemon_client::DaemonService;
use uc_daemon_contract::api::dto::clipboard_command::DispatchOutcomeResponse;

use crate::commands::app_session::connect_or_spawn_oneshot_daemon;
#[cfg(feature = "dev-tools")]
use crate::commands::app_session::{build_app_session, refuse_if_daemon_running, CliAppSession};
use crate::exit_codes;
use crate::ui;

pub struct SendArgs {
    /// Plaintext to dispatch in **new-entry** mode. When `None` and
    /// neither `file` nor `resend` is set, the command reads from stdin
    /// until EOF — handy for `echo hi | uniclip send` and the
    /// dual-profile test recipe. Ignored when `resend` or `file` is set
    /// (clap enforces mutual exclusion at the parser layer).
    pub text: Option<String>,
    /// Path to a file to send instead of text. Mutually exclusive with
    /// positional text and `--resend`. The file is published as a blob,
    /// dispatched as a clipboard envelope referencing the blob, and the
    /// CLI keeps the iroh router alive (passive provider) until Ctrl-C
    /// so the receiver has time to fetch.
    pub file: Option<PathBuf>,
    /// Entry id to **resend**. When set, the command pulls the original
    /// snapshot from local storage via `AppFacade::resend_entry`.
    pub resend: Option<String>,
    /// Optional list of target device IDs. Empty vec means "no filter"
    /// (full fan-out for new entry; derived diff for resend).
    pub peers: Vec<String>,
}

pub async fn run(args: SendArgs, json: bool, verbose: bool) -> i32 {
    if let Some(file) = args.file {
        if args.text.is_some() || args.resend.is_some() {
            ui::error("`--file` cannot be combined with positional text or `--resend`.");
            return exit_codes::EXIT_ERROR;
        }
        if !args.peers.is_empty() {
            ui::error("`--peer` is not supported with `--file` yet.");
            return exit_codes::EXIT_ERROR;
        }
        #[cfg(feature = "dev-tools")]
        {
            return run_send_file(file, json, verbose).await;
        }
        #[cfg(not(feature = "dev-tools"))]
        {
            let _ = file;
            ui::error(
                "`send --file` requires the in-process blob stack (build with --features dev-tools).",
            );
            return exit_codes::EXIT_ERROR;
        }
    }

    let mode = if args.resend.is_some() {
        SendMode::Resend
    } else {
        SendMode::New
    };

    if !json {
        ui::header(mode.header());
    }

    if args.resend.is_some() && args.text.is_some() {
        ui::error("--resend cannot be combined with positional text.");
        return exit_codes::EXIT_ERROR;
    }

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

    let peers_str: Option<Vec<String>> = if args.peers.is_empty() {
        None
    } else {
        Some(args.peers.clone())
    };

    let service = match connect_or_spawn_oneshot_daemon(verbose).await {
        Ok(s) => s,
        Err(code) => return code,
    };
    run_send_via_daemon(&*service, mode, plaintext, args.resend, peers_str, json).await
}

async fn run_send_via_daemon(
    service: &dyn DaemonService,
    mode: SendMode,
    plaintext: Option<String>,
    resend_id: Option<String>,
    peers: Option<Vec<String>>,
    json: bool,
) -> i32 {
    // ADR-008 P5-1a: hold a control-WS lease across the dispatch call so a
    // transient Oneshot daemon does not self-terminate mid-fan-out. The HTTP
    // dispatch blocks until the daemon's bounded fan-out deadline, so holding
    // the lease to the end of this fn covers the in-flight send. Bind to a named
    // var (NOT `_`) so it lives to scope end; `_` would drop it immediately.
    let _lease = match service.hold_control_lease().await {
        Ok(guard) => guard,
        Err(err) => {
            ui::error(&format!("Failed to hold daemon session lease: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    match mode {
        SendMode::New => {
            let text = plaintext.expect("plaintext populated in new-entry mode");
            let dispatch_spinner = ui::spinner("Dispatching to online peers via daemon...");
            match service.dispatch_text(&text, peers).await {
                Ok(resp) => {
                    ui::spinner_finish_success(
                        &dispatch_spinner,
                        &format!(
                            "{} accepted, {} duplicate, {} offline, {} error(s)",
                            resp.total_accepted,
                            resp.total_duplicate,
                            resp.total_offline,
                            resp.total_errored
                        ),
                    );
                    if json {
                        if let Ok(s) = serde_json::to_string_pretty(&resp) {
                            println!("{s}");
                        }
                    } else {
                        render_daemon_dispatch(&resp);
                    }
                    if resp.total_accepted == 0 && resp.total_duplicate == 0 {
                        exit_codes::EXIT_ERROR
                    } else {
                        exit_codes::EXIT_SUCCESS
                    }
                }
                Err(err) => {
                    ui::spinner_finish_error(&dispatch_spinner, &format!("Dispatch failed: {err}"));
                    exit_codes::EXIT_ERROR
                }
            }
        }
        SendMode::Resend => {
            let entry_id_str = resend_id.expect("resend id present");
            let resend_spinner = ui::spinner("Resending entry via daemon...");
            match service.resend_entry(&entry_id_str, peers).await {
                Ok(resp) => {
                    ui::spinner_finish_success(
                        &resend_spinner,
                        &format!(
                            "{} accepted, {} duplicate, {} offline, {} error(s), {} pending",
                            resp.accepted, resp.duplicate, resp.offline, resp.errored, resp.pending,
                        ),
                    );
                    if json {
                        if let Ok(s) = serde_json::to_string_pretty(&resp) {
                            println!("{s}");
                        }
                    } else {
                        ui::bar();
                        ui::info("entry", &entry_id_str);
                        ui::info(
                            "summary",
                            &format!(
                                "{} accepted, {} duplicate, {} offline, {} error(s), {} pending",
                                resp.accepted,
                                resp.duplicate,
                                resp.offline,
                                resp.errored,
                                resp.pending,
                            ),
                        );
                        ui::bar();
                    }
                    if resp.accepted == 0 && resp.duplicate == 0 && resp.pending == 0 {
                        exit_codes::EXIT_ERROR
                    } else {
                        exit_codes::EXIT_SUCCESS
                    }
                }
                Err(err) => {
                    ui::spinner_finish_error(&resend_spinner, &format!("Resend failed: {err}"));
                    exit_codes::EXIT_ERROR
                }
            }
        }
    }
}

fn render_daemon_dispatch(resp: &DispatchOutcomeResponse) {
    ui::bar();
    ui::info("hash", short_hash(&resp.content_hash));
    if resp.per_target.is_empty() {
        ui::info("targets", "(none — no online peers)");
    } else {
        for t in &resp.per_target {
            let detail = match t.outcome.as_str() {
                "accepted" => "accepted".to_string(),
                "duplicate" => "duplicate (peer already had it)".to_string(),
                _ => format!("failed: {}", t.error.as_deref().unwrap_or("unknown")),
            };
            ui::info("·", &format!("{} → {}", t.device_id, detail));
        }
    }
    ui::bar();
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

fn short_hash(hash: &str) -> &str {
    if hash.len() > 16 {
        &hash[..16]
    } else {
        hash
    }
}

// ── File-send (dev-tools only) ────────────────────────────────────────

#[cfg(feature = "dev-tools")]
fn render_dispatch_error(err: &ClipboardSyncError) -> String {
    match err {
        ClipboardSyncError::LockedSpace => {
            "Space is locked — unlock or re-init before sending.".to_string()
        }
        ClipboardSyncError::CipherFailure(msg) => format!("Encryption failed: {msg}"),
        ClipboardSyncError::Repository(msg) => format!("Peer address lookup failed: {msg}"),
    }
}

#[cfg(feature = "dev-tools")]
fn render_per_target(entry: &DispatchEntryPerTarget) -> String {
    match &entry.outcome {
        Ok(DispatchAck::Accepted) => "accepted".to_string(),
        Ok(DispatchAck::DuplicateIgnored) => "duplicate (peer already had it)".to_string(),
        Err(reason) => format!("failed: {reason}"),
    }
}

/// `send -f <path>` path (requires dev-tools feature).
///
/// Unlike the text path, dispatch returns but the process must stay alive.
/// `publish_blob_path` adds the file to the local iroh-blobs store + encodes
/// the ticket into a V3 envelope; the actual bytes transfer happens when the
/// receiver's fetch task pulls from this CLI acting as passive provider.
#[cfg(feature = "dev-tools")]
async fn run_send_file(path: PathBuf, json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Send file");
    }

    // metadata before daemon check — bail early on obviously wrong paths.
    let abs_path = match path.canonicalize() {
        Ok(p) => p,
        Err(err) => {
            ui::error(&format!("Failed to resolve file path: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };
    let metadata = match tokio::fs::metadata(&abs_path).await {
        Ok(m) => m,
        Err(err) => {
            ui::error(&format!("Failed to stat file: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };
    if !metadata.is_file() {
        ui::error("Path is not a regular file.");
        return exit_codes::EXIT_ERROR;
    }
    let size_bytes = metadata.len();
    let filename = abs_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "file".to_string());

    if let Err(code) = refuse_if_daemon_running().await {
        return code;
    }

    let cli = match build_app_session(verbose).await {
        Ok(bundle) => bundle,
        Err(code) => return code,
    };

    if let Err(code) = resume_and_probe(&cli).await {
        cli.shutdown().await;
        return code;
    }

    let entry_id = EntryId::new();
    let publish_spinner = ui::spinner(&format!("Publishing '{filename}' to local blob store..."));
    let publish_result = match cli
        .app_facade()
        .publish_blob_path(PublishBlobPathCommand {
            path: abs_path.clone(),
            entry_id: Some(entry_id.clone()),
        })
        .await
    {
        Ok(r) => {
            ui::spinner_finish_success(&publish_spinner, "Blob published");
            r
        }
        Err(err) => {
            let msg = match &err {
                BlobTransferError::Publish(s) | BlobTransferError::Fetch(s) => s.clone(),
                BlobTransferError::Cancelled => "publish cancelled".to_string(),
            };
            ui::spinner_finish_error(&publish_spinner, &format!("Publish failed: {msg}"));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    };

    let uri_bytes = format!("file://{}\n", abs_path.display()).into_bytes();
    let snapshot = SystemClipboardSnapshot {
        ts_ms: chrono::Utc::now().timestamp_millis(),
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            uri_bytes,
        )],
    };
    let blob_refs = vec![V3BlobRef {
        ticket: publish_result.ticket,
        entry_id: publish_result.entry_id.clone(),
        filename: Some(filename.clone()),
        mime: None,
        size_bytes,
        representation_index: None,
    }];

    let dispatch_spinner = ui::spinner("Dispatching envelope to online peers...");
    let outcome = match cli
        .app_facade()
        .dispatch_clipboard_snapshot_with_blob_refs(
            snapshot,
            blob_refs,
            ClipboardChangeOrigin::LocalCapture,
        )
        .await
    {
        Ok(o) => {
            ui::spinner_finish_success(
                &dispatch_spinner,
                &format!(
                    "{} accepted, {} duplicate, {} offline, {} error(s)",
                    o.total_accepted, o.total_duplicate, o.total_offline, o.total_errored
                ),
            );
            o
        }
        Err(err) => {
            ui::spinner_finish_error(&dispatch_spinner, &render_dispatch_error(&err));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    };

    if json {
        let dto = SendFileOutcomeDto {
            content_hash: outcome.content_hash.clone(),
            filename: filename.clone(),
            size_bytes,
            entry_id: entry_id.to_string(),
            total_accepted: outcome.total_accepted,
            total_duplicate: outcome.total_duplicate,
            total_offline: outcome.total_offline,
            total_errored: outcome.total_errored,
            at_ms: outcome.at_ms,
        };
        match serde_json::to_string_pretty(&dto) {
            Ok(s) => println!("{s}"),
            Err(err) => {
                ui::error(&format!("Failed to serialize outcome: {err}"));
                cli.shutdown().await;
                return exit_codes::EXIT_ERROR;
            }
        }
    } else {
        ui::bar();
        ui::info("file", &filename);
        ui::info("size", &human_size(size_bytes));
        ui::info("hash", short_hash(&outcome.content_hash));
        if outcome.per_target.is_empty() {
            ui::info("targets", "(none — no online peers)");
        } else {
            for entry in &outcome.per_target {
                ui::info(
                    "·",
                    &format!(
                        "{} → {}",
                        entry.device_id.as_str(),
                        render_per_target(entry),
                    ),
                );
            }
        }
        ui::bar();
        ui::info("status", "Serving file — press Ctrl-C to stop");
    }

    // Keep the process alive until Ctrl-C so the receiver can fetch.
    let _ = tokio::signal::ctrl_c().await;
    if !json {
        ui::end("Stopped");
    }
    cli.shutdown().await;
    exit_codes::EXIT_SUCCESS
}

#[cfg(feature = "dev-tools")]
async fn resume_and_probe(cli: &CliAppSession) -> Result<(), i32> {
    let resume_spinner = ui::spinner("Resuming space session...");
    match cli.app_facade().try_resume_session().await {
        Ok(true) => ui::spinner_finish_success(&resume_spinner, "Session resumed"),
        Ok(false) => {
            ui::spinner_finish_error(
                &resume_spinner,
                "No space on this profile — run `init` or `join` first.",
            );
            return Err(exit_codes::EXIT_ERROR);
        }
        Err(err) => {
            ui::spinner_finish_error(&resume_spinner, &format!("Resume failed: {err}"));
            return Err(exit_codes::EXIT_ERROR);
        }
    }

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
            &format!("Probe round failed: {err} (proceeding)"),
        ),
    }

    Ok(())
}

#[cfg(feature = "dev-tools")]
fn human_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.2} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.2} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(feature = "dev-tools")]
#[derive(Serialize)]
struct SendFileOutcomeDto {
    content_hash: String,
    filename: String,
    size_bytes: u64,
    entry_id: String,
    total_accepted: usize,
    total_duplicate: usize,
    total_offline: usize,
    total_errored: usize,
    at_ms: i64,
}
