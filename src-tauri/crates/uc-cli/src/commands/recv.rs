//! `uniclip recv` — single-shot inbound file receiver.
//!
//! Self-contained direct-mode command (no daemon). Brings up the same
//! application session as `watch` / `send` — `SpaceSetupAssembly` already
//! auto-spawns the inbound ingest loop at construction time — subscribes
//! to the inbound clipboard notice broadcast, waits for the first
//! envelope carrying a file blob ref, and streams that blob to a local
//! file via `BlobTransferFacade::fetch_blob_to_path`.
//!
//! ## Difference from `start`
//!
//! `start` runs the daemon, which writes received clipboard content
//! straight into the OS clipboard. `recv` deliberately does NOT touch
//! the system clipboard. It is a one-shot file sink for CLI users who
//! want to receive a file from another paired device into a known
//! filesystem location, without the daemon's "make this the active
//! clipboard payload" behaviour.
//!
//! ## Cancellation
//!
//! While `fetch_blob_to_path` is running, the receiver-side iroh-blobs
//! fetch task is registered in the inflight registry via the supplied
//! `FetchTransferContext`. Pressing Ctrl-C calls
//! `AppFacade::cancel_inbound_transfer(transfer_id, LocalUser)` — the
//! exact same code path the Tauri command + GUI cancel button use —
//! which tears down the fetch token, shuts the QUIC endpoint, and
//! appends a `Cancelled` domain event. The partial output file is
//! removed before exit.
//!
//! ## Scope (P1)
//!
//! Picks **one** free-file blob (i.e. `blob_ref.representation_index ==
//! None && filename.is_some()`) from the first usable envelope and
//! exits. Multi-file envelopes get the first file only; the rest are
//! ignored (sender can re-send if needed). Inline image blobs
//! (`representation_index = Some(_)`) are skipped — they belong to the
//! clipboard pipeline, not a file sink.

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

use crate::commands::app_session::{build_app_session, refuse_if_daemon_running, CliAppSession};
use crate::exit_codes;
use crate::ui;

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

    // Find the first inbound notice that actually contains a free-file
    // blob ref. Notices arrive as soon as the sender's envelope decodes
    // on our side; we may see text-only envelopes first while another
    // peer is mid-send. Skip those (with a tracing breadcrumb) until a
    // file shows up.
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
        // CLI `uniclip recv` 一次只 fetch 一个 blob, batch 唯一一帧 ——
        // facade 既 seed 也 complete lifecycle, 行为与改造前等价。
        batch_position: Default::default(),
    };

    // Ctrl-C → cancel. We arm a background task that watches both
    // signals: a fresh ctrl_c() future and a `done` token tripped when
    // fetch returns. Whichever fires first wins. Without the `done`
    // token, the spawned task would linger after a successful fetch and
    // swallow a subsequent unrelated ctrl_c.
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

    // Tell the cancel-arm task to stand down (success path) — if Ctrl-C
    // already fired this is a no-op (the spawned task already
    // progressed past the select! arm).
    let _ = done_tx.send(());
    // Best-effort: give the cancel-arm a moment to wind down so its
    // tracing line lands before our final summary.
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
                    transfer_id: &transfer_id,
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
            // The fetch may have left a partial file on disk.
            // `fetch_blob_to_path` doesn't auto-clean; do it here so
            // recv's contract — "successful path or nothing" — holds.
            cleanup_partial(&target_path).await;
            if json {
                let dto = RecvOutcomeDto {
                    from_device: notice.from_device.as_str(),
                    path: &target_path.display().to_string(),
                    bytes_written: 0,
                    entry_id: &blob_ref.entry_id.to_string(),
                    transfer_id: &transfer_id,
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

/// Pick the first **free-file** blob from a notice — i.e. one with a
/// real filename and no `representation_index` (the latter means the
/// blob's bytes belong inside a snapshot rep, not as a standalone file).
fn pick_first_file_blob_ref(notice: &InboundNotice) -> Option<V3BlobRef> {
    let (_snapshot, blob_refs) =
        decode_v3_bytes_to_snapshot_and_blob_refs(&notice.plaintext).ok()?;
    blob_refs.into_iter().find(|r| {
        r.representation_index.is_none() && r.filename.as_deref().is_some_and(|s| !s.is_empty())
    })
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

async fn cleanup_partial(path: &Path) {
    if let Err(err) = tokio::fs::remove_file(path).await {
        if err.kind() != std::io::ErrorKind::NotFound {
            warn!(error = %err, path = %path.display(), "Failed to remove partial file");
        }
    }
}

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

fn short_hash(s: &str) -> &str {
    if s.len() > 8 {
        &s[..8]
    } else {
        s
    }
}

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

#[derive(Serialize)]
struct RecvOutcomeDto<'a> {
    from_device: &'a str,
    path: &'a str,
    bytes_written: u64,
    entry_id: &'a str,
    transfer_id: &'a str,
    /// `received` | `cancelled` | `failed`.
    outcome: &'static str,
}
