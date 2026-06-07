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

use serde::Serialize;

use uc_daemon_client::DaemonService;

use crate::commands::app_session::{connect_or_spawn_oneshot_daemon, wait_and_reconnect_daemon};
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
