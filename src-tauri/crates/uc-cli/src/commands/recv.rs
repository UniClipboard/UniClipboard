//! `uniclip recv` — single-shot inbound file receiver (daemon-client).
//!
//! ADR-008 P5-1b: `recv` is a pure daemon client. It connects to a running
//! compatible daemon (or spawns a transient Oneshot one), holds a control
//! lease so a transient daemon stays alive while a large free-file
//! materializes, waits for the first inbound clipboard entry that carries a
//! materialized free-file, exports its bytes from the daemon, and writes them
//! into the user-chosen output directory. It NEVER fetches blobs in-process
//! (no iroh / diesel edge) and never touches the system clipboard.
//!
//! ## Readiness signal
//!
//! The daemon emits `clipboard.inbound_notice` BEFORE the inbound free-file is
//! materialized (the file is not on disk yet), then emits
//! `clipboard.new_content` AFTER `apply_notice` — including materialization —
//! completes. `recv` therefore waits for `new_content` (the reliable readiness
//! signal) carrying the **receiver-side** `entry_id`, and exports against that
//! id. No polling is required.
//!
//! ## Origin filter
//!
//! `clipboard.new_content` also fires for the daemon's own local clipboard
//! captures. The daemon-client subscription only forwards events with
//! `origin == "remote"`, so a local copy on the daemon host never triggers a
//! spurious receive here.
//!
//! ## Difference from `start`
//!
//! `start` runs the daemon, which writes received clipboard content straight
//! into the OS clipboard. `recv` deliberately does NOT touch the system
//! clipboard. It is a one-shot file sink for CLI users who want to receive a
//! file from another paired device into a known filesystem location.
//!
//! ## Known behaviour difference
//!
//! An inbound entry that is an exact duplicate of an existing local entry is
//! dropped by the daemon as `DuplicateSkipped` and does NOT emit
//! `clipboard.new_content` — so `recv` will not observe it. This is rare; the
//! remedy is to re-send the same file. (Pre-P5-1b in-process `recv` keyed off
//! `inbound_notice` and so could observe duplicates; that path is retired.)

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{broadcast, oneshot};
use tracing::warn;
use uuid::Uuid;

use uc_application::facade::space_setup::TryResumeSessionError;
use uc_application::facade::{
    decode_v3_bytes_to_snapshot_and_blob_refs, BlobTransferError, FetchBlobToPathCommand,
    FetchTransferContext, InboundNotice, V3BlobRef,
};
use uc_core::FileTransferCancellationReason;

use uc_daemon_client::DaemonService;

use crate::commands::app_session::{
    build_app_session, connect_or_spawn_oneshot_daemon, refuse_if_daemon_running,
    wait_and_reconnect_daemon, CliAppSession,
};
use crate::exit_codes;
use crate::ui;

const RECONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

pub async fn run(out: Option<PathBuf>, json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Receive file");
    }

    let out_dir = match resolve_out_dir(out).await {
        Ok(p) => p,
        Err(msg) => {
            ui::error(&msg);
            return exit_codes::EXIT_ERROR;
        }
    };

    let service = match connect_or_spawn_oneshot_daemon(verbose).await {
        Ok(s) => s,
        Err(code) => return code,
    };

    run_recv_via_daemon(&*service, out_dir, json).await
}

#[allow(unused_variables, unused_assignments)]
async fn run_recv_via_daemon(service: &dyn DaemonService, out_dir: PathBuf, json: bool) -> i32 {
    // Hold a control-WS lease for the whole receive window. Free-file
    // materialization on the daemon can take a while for large files, so a
    // transient Oneshot daemon must not self-terminate while we wait. Bind to a
    // named var (NOT `_`) so it lives to scope end; `_` would drop it at once.
    // Reassigned on reconnect to keep the new lease alive.
    let mut lease = match service.hold_control_lease().await {
        Ok(guard) => guard,
        Err(err) => {
            ui::error(&format!("Failed to hold daemon session lease: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let mut rx = match service.subscribe_inbound_entries().await {
        Ok(rx) => rx,
        Err(err) => {
            ui::error(&format!("Failed to subscribe inbound entries: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    if !json {
        ui::info("out", &out_dir.display().to_string());
        ui::info("status", "Waiting for incoming file — press Ctrl-C to stop");
        ui::bar();
    }

    // Track the active service for export calls after a potential reconnect.
    // The initial `service` arg is borrowed; after reconnect we own the new one.
    let mut owned_service: Option<Box<dyn DaemonService>> = None;
    let mut reconnected = false;

    loop {
        let active_service: &dyn DaemonService = match &owned_service {
            Some(s) => &**s,
            None => service,
        };

        tokio::select! {
            biased;
            _ = tokio::signal::ctrl_c() => {
                if !json { ui::end("Stopped"); }
                return exit_codes::EXIT_SUCCESS;
            }
            recv = rx.recv() => match recv {
                Some(entry) => {
                    match active_service.export_entry_file(&entry.entry_id).await {
                        Ok(Some(export)) => {
                            return finish_export(
                                &out_dir,
                                &entry.entry_id,
                                &entry.from_device,
                                export,
                                json,
                            );
                        }
                        Ok(None) => {
                            if !json {
                                ui::info("·", &format!(
                                    "entry {} carried no file — waiting for next",
                                    short_hash(&entry.entry_id),
                                ));
                            }
                            continue;
                        }
                        Err(err) => {
                            ui::error(&format!("Failed to export file: {err}"));
                            return exit_codes::EXIT_ERROR;
                        }
                    }
                }
                None => {
                    if reconnected {
                        ui::error("Inbound channel closed again; exiting.");
                        return exit_codes::EXIT_ERROR;
                    }
                    if !json {
                        ui::warn("Daemon connection lost — reconnecting...");
                    }
                    let new_service = match wait_and_reconnect_daemon(RECONNECT_TIMEOUT).await {
                        Ok(s) => s,
                        Err(code) => return code,
                    };
                    lease = match new_service.hold_control_lease().await {
                        Ok(guard) => guard,
                        Err(err) => {
                            ui::error(&format!("Failed to re-acquire lease after reconnect: {err}"));
                            return exit_codes::EXIT_ERROR;
                        }
                    };
                    rx = match new_service.subscribe_inbound_entries().await {
                        Ok(new_rx) => new_rx,
                        Err(err) => {
                            ui::error(&format!("Failed to re-subscribe after reconnect: {err}"));
                            return exit_codes::EXIT_ERROR;
                        }
                    };
                    owned_service = Some(new_service);
                    reconnected = true;
                    if !json {
                        ui::warn(
                            "Reconnected — events during daemon restart may have been missed",
                        );
                    }
                }
            }
        }
    }
}

fn finish_export(
    out_dir: &Path,
    entry_id: &str,
    from_device: &str,
    export: uc_daemon_client::FileExport,
    json: bool,
) -> i32 {
    let filename = sanitize_filename(&export.filename);
    let target_path = out_dir.join(&filename);
    let bytes_written = export.bytes.len() as u64;

    if let Err(err) = std::fs::write(&target_path, &export.bytes) {
        ui::error(&format!("Failed to write file: {err}"));
        return exit_codes::EXIT_ERROR;
    }

    if json {
        let dto = RecvOutcomeDto {
            from_device,
            path: &target_path.display().to_string(),
            bytes_written,
            entry_id,
            outcome: "received",
        };
        if let Ok(s) = serde_json::to_string_pretty(&dto) {
            println!("{s}");
        }
    } else {
        ui::info("file", &filename);
        ui::info("→", &target_path.display().to_string());
        ui::bar();
        ui::info("bytes", &bytes_written.to_string());
        ui::end("Done");
    }

    exit_codes::EXIT_SUCCESS
}

async fn resolve_out_dir(out: Option<PathBuf>) -> Result<PathBuf, String> {
    let dir = match out {
        Some(p) => p,
        None => std::env::current_dir()
            .map_err(|err| format!("Failed to resolve current directory: {err}"))?,
    };
    if !dir.exists() {
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|err| format!("Failed to create output directory: {err}"))?;
    } else if !dir.is_dir() {
        return Err(format!("Output path is not a directory: {}", dir.display()));
    }
    dir.canonicalize()
        .map_err(|err| format!("Failed to canonicalize output directory: {err}"))
}

/// Strip any path separators a malicious sender might inject into the
/// filename. We never trust the remote-supplied filename verbatim.
fn sanitize_filename(name: &str) -> String {
    let stripped: String = name
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | '\0'))
        .collect();
    if stripped.is_empty() || stripped == "." || stripped == ".." {
        "uniclip-recv.bin".to_string()
    } else {
        stripped
    }
}

fn short_hash(s: &str) -> &str {
    if s.len() > 8 {
        &s[..8]
    } else {
        s
    }
}

#[derive(Serialize)]
struct RecvOutcomeDto<'a> {
    /// Sending device id; empty when the source device is not available.
    from_device: &'a str,
    path: &'a str,
    bytes_written: u64,
    entry_id: &'a str,
    /// `received` | `cancelled` | `failed`.
    outcome: &'static str,
}

// ─────────────────────────────────────────────────────────────────
// Retired in-process path (ADR-008 P5-1b). Kept as dead code until the
// dependency edge is deleted in P5-4; preserving it keeps this slice's diff
// small and revert-safe. None of this runs in the daemon-client path above.
// ─────────────────────────────────────────────────────────────────

// Retired in-process body; clippy lints are silenced here because this whole
// path is removed in P5-4 along with the iroh dependency edge.
#[allow(dead_code, clippy::unnecessary_to_owned)]
async fn run_recv_in_process(out_dir: PathBuf, json: bool, verbose: bool) -> i32 {
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

    let mut rx = match cli.app_facade().subscribe_inbound_clipboard_notices() {
        Ok(rx) => rx,
        Err(err) => {
            ui::error(&format!(
                "Failed to subscribe inbound clipboard notices: {err}"
            ));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    };

    if !json {
        ui::info("out", &out_dir.display().to_string());
        ui::info("status", "Waiting for incoming file — press Ctrl-C to stop");
        ui::bar();
    }

    let (notice, blob_ref) = loop {
        tokio::select! {
            biased;
            _ = tokio::signal::ctrl_c() => {
                if !json { ui::end("Stopped"); }
                cli.shutdown().await;
                return exit_codes::EXIT_SUCCESS;
            }
            recv = rx.recv() => match recv {
                Ok(notice) => match pick_first_file_blob_ref(&notice) {
                    Some(blob_ref) => break (notice, blob_ref),
                    None => {
                        if !json {
                            ui::info("·", &format!(
                                "[{}] envelope carried no file blob — waiting for next",
                                notice.from_device.as_str(),
                            ));
                        }
                        continue;
                    }
                },
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    if !json {
                        ui::warn(&format!("Lagged: dropped {missed} notice(s)"));
                    }
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    ui::error("Inbound channel closed before any file arrived.");
                    cli.shutdown().await;
                    return exit_codes::EXIT_ERROR;
                }
            }
        }
    };

    let filename = blob_ref
        .filename
        .clone()
        .unwrap_or_else(|| format!("uniclip-{}.bin", short_hash(&blob_ref.entry_id.to_string())));
    let target_path = out_dir.join(sanitize_filename(&filename));
    let transfer_id = Uuid::new_v4().to_string();

    if !json {
        ui::info("from", notice.from_device.as_str());
        ui::info("file", &filename);
        ui::info("size", &human_size(blob_ref.size_bytes));
        ui::info("→", &target_path.display().to_string());
    }

    let context = FetchTransferContext {
        transfer_id: transfer_id.clone(),
        peer_id: notice.from_device.as_str().to_string(),
        total_bytes: Some(blob_ref.size_bytes),
        filename: filename.clone(),
        outbound_transfer_id: None,
        outbound_target: None,
        batch_position: Default::default(),
    };

    let app_facade = Arc::clone(cli.app_facade());
    let (done_tx, done_rx) = oneshot::channel::<()>();
    let cancel_arm = {
        let app_facade = Arc::clone(&app_facade);
        let transfer_id = transfer_id.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = done_rx => {}
                res = tokio::signal::ctrl_c() => {
                    if res.is_err() { return; }
                    if let Err(err) = app_facade
                        .cancel_inbound_transfer(
                            &transfer_id,
                            FileTransferCancellationReason::LocalUser,
                        )
                        .await
                    {
                        warn!(error = %err, transfer_id, "cancel_inbound_transfer failed");
                    }
                }
            }
        })
    };

    let fetch_spinner = ui::spinner(&format!("Fetching '{filename}'..."));
    let result = app_facade
        .fetch_blob_to_path(FetchBlobToPathCommand {
            ticket: blob_ref.ticket.clone(),
            entry_id: blob_ref.entry_id.clone(),
            target_path: target_path.clone(),
            transfer_context: Some(context),
        })
        .await;

    let _ = done_tx.send(());
    let _ = tokio::time::timeout(Duration::from_millis(50), cancel_arm).await;

    let exit_code = match result {
        Ok(r) => {
            ui::spinner_finish_success(&fetch_spinner, "File received");
            if json {
                let dto = RecvOutcomeDto {
                    from_device: notice.from_device.as_str(),
                    path: &target_path.display().to_string(),
                    bytes_written: r.bytes_written,
                    entry_id: &blob_ref.entry_id.to_string(),
                    outcome: "received",
                };
                if let Ok(s) = serde_json::to_string_pretty(&dto) {
                    println!("{s}");
                }
            } else {
                ui::bar();
                ui::info("bytes", &r.bytes_written.to_string());
                ui::end("Done");
            }
            exit_codes::EXIT_SUCCESS
        }
        Err(BlobTransferError::Cancelled) => {
            ui::spinner_finish_error(&fetch_spinner, "Cancelled");
            cleanup_partial(&target_path).await;
            if json {
                let dto = RecvOutcomeDto {
                    from_device: notice.from_device.as_str(),
                    path: &target_path.display().to_string(),
                    bytes_written: 0,
                    entry_id: &blob_ref.entry_id.to_string(),
                    outcome: "cancelled",
                };
                if let Ok(s) = serde_json::to_string_pretty(&dto) {
                    println!("{s}");
                }
            } else {
                ui::end("Cancelled — partial file removed");
            }
            exit_codes::EXIT_ERROR
        }
        Err(err) => {
            let msg = match &err {
                BlobTransferError::Publish(s) | BlobTransferError::Fetch(s) => s.clone(),
                BlobTransferError::Cancelled => unreachable!(),
            };
            ui::spinner_finish_error(&fetch_spinner, &format!("Fetch failed: {msg}"));
            cleanup_partial(&target_path).await;
            exit_codes::EXIT_ERROR
        }
    };

    cli.shutdown().await;
    exit_code
}

/// Pick the first **free-file** blob from a notice — i.e. one with a
/// real filename and no `representation_index` (the latter means the
/// blob's bytes belong inside a snapshot rep, not as a standalone file).
#[allow(dead_code)]
fn pick_first_file_blob_ref(notice: &InboundNotice) -> Option<V3BlobRef> {
    let (_snapshot, blob_refs) =
        decode_v3_bytes_to_snapshot_and_blob_refs(&notice.plaintext).ok()?;
    blob_refs.into_iter().find(|r| {
        r.representation_index.is_none() && r.filename.as_deref().is_some_and(|s| !s.is_empty())
    })
}

#[allow(dead_code)]
async fn cleanup_partial(path: &Path) {
    if let Err(err) = tokio::fs::remove_file(path).await {
        if err.kind() != std::io::ErrorKind::NotFound {
            warn!(error = %err, path = %path.display(), "Failed to remove partial file");
        }
    }
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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
        Err(TryResumeSessionError::Internal(msg)) => {
            ui::spinner_finish_error(&resume_spinner, &format!("Resume failed: {msg}"));
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
