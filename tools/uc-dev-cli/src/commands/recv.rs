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

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::commands::app_session::connect_or_spawn_oneshot_daemon;
use crate::commands::inbound_wait::InboundWaitSession;
use crate::exit_codes;
use crate::ui;

pub async fn run(out: Option<PathBuf>, json: bool, verbose: bool) -> i32 {
    ui::warn("uniclip recv is deprecated; use uniclip get --wait");
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

    run_recv_via_daemon(service, out_dir, json).await
}

async fn run_recv_via_daemon(
    service: Box<dyn uc_daemon_client::DaemonService>,
    out_dir: PathBuf,
    json: bool,
) -> i32 {
    let mut session = match InboundWaitSession::connect(service).await {
        Ok(session) => session,
        Err(code) => return code,
    };

    if !json {
        ui::info("out", &out_dir.display().to_string());
        ui::info("status", "Waiting for incoming file — press Ctrl-C to stop");
        ui::bar();
    }

    loop {
        match session.next().await {
            Ok(Some(entry)) => match session.service().export_entry_file(&entry.entry_id).await {
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
                        ui::info(
                            "·",
                            &format!(
                                "entry {} carried no file — waiting for next",
                                short_hash(&entry.entry_id),
                            ),
                        );
                    }
                    continue;
                }
                Err(err) => {
                    ui::error(&format!("Failed to export file: {err}"));
                    return exit_codes::EXIT_ERROR;
                }
            },
            Ok(None) => {
                if !json {
                    ui::end("Stopped");
                }
                return exit_codes::EXIT_SUCCESS;
            }
            Err(code) => return code,
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

    let path = target_path.display().to_string();
    let dto = RecvOutcomeDto {
        from_device,
        path: &path,
        bytes_written,
        entry_id,
        outcome: "received",
    };
    let rendered = match render_outcome(&dto, json) {
        Ok(rendered) => rendered,
        Err(error) => {
            ui::error(&format!("Failed to serialize receive result: {error}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    // Waiting and transfer progress belong on stderr. Keep the completed
    // value on stdout so command substitution captures only the path.
    println!("{rendered}");
    let _ = std::io::stdout().flush();

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

fn render_outcome(outcome: &RecvOutcomeDto<'_>, json: bool) -> Result<String, serde_json::Error> {
    if json {
        serde_json::to_string_pretty(outcome)
    } else {
        Ok(outcome.path.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{render_outcome, RecvOutcomeDto};

    fn outcome<'a>(path: &'a str) -> RecvOutcomeDto<'a> {
        RecvOutcomeDto {
            from_device: "device-1",
            path,
            bytes_written: 42,
            entry_id: "entry-1",
            outcome: "received",
        }
    }

    #[test]
    fn human_output_is_only_the_received_path() {
        assert_eq!(
            render_outcome(&outcome("/tmp/received.png"), false)
                .expect("human output should render"),
            "/tmp/received.png"
        );
    }

    #[test]
    fn json_output_keeps_receive_metadata() {
        let rendered =
            render_outcome(&outcome("/tmp/received.png"), true).expect("JSON output should render");
        let value: serde_json::Value =
            serde_json::from_str(&rendered).expect("JSON output should parse");

        assert_eq!(
            value,
            serde_json::json!({
                "from_device": "device-1",
                "path": "/tmp/received.png",
                "bytes_written": 42,
                "entry_id": "entry-1",
                "outcome": "received"
            })
        );
    }
}
