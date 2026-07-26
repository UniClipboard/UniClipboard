//! `uniclip watch` — foreground inbound clipboard observer
//! (Slice 2 Phase 2 · T11).
//!
//! Self-contained direct-mode command (no daemon). The backing
//! `SyncEngineAssembly` auto-spawns the ingest loop at construction
//! (Phase 2 · T10), so this command's job is purely to subscribe to the
//! application-level notice broadcast and render each delivery until
//! Ctrl-C.
//!
//! Phase 2 deliberately does **not** write to the system clipboard
//! (plan §5.3): a short-lived CLI process writing the OS clipboard would
//! collide with the daemon's own watcher and trigger a sync echo. Daemon
//! integration arrives in Phase 3 / Slice 4.

use serde::Serialize;

use uc_daemon_client::DaemonService;
use uc_daemon_contract::api::dto::clipboard_command::{
    InboundNoticeEvent, InboundRepresentationSummaryDto,
};

use crate::commands::app_session::{connect_or_spawn_oneshot_daemon, wait_and_reconnect_daemon};
use crate::exit_codes;
use crate::ui;

const RECONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

pub async fn run(json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Watch inbound clipboard");
    }

    let service = match connect_or_spawn_oneshot_daemon(verbose).await {
        Ok(s) => s,
        Err(code) => return code,
    };
    run_watch_via_daemon(&*service, json).await
}

async fn run_watch_via_daemon(service: &dyn DaemonService, json: bool) -> i32 {
    let subscribe_spinner = ui::spinner("Subscribing to daemon clipboard events...");
    let mut rx = match service.subscribe_inbound_notices().await {
        Ok(rx) => {
            ui::spinner_finish_success(&subscribe_spinner, "Subscribed via daemon WS");
            rx
        }
        Err(err) => {
            ui::spinner_finish_error(&subscribe_spinner, &format!("Failed to subscribe: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    if !json {
        ui::info("status", "Listening via daemon — press Ctrl-C to stop");
        ui::bar();
    }
    emit_watch_ready();

    let mut reconnected = false;
    loop {
        tokio::select! {
            biased;
            _ = tokio::signal::ctrl_c() => {
                if !json { ui::end("Stopped"); }
                return exit_codes::EXIT_SUCCESS;
            }
            recv = rx.recv() => match recv {
                Some(event) => render_daemon_notice(&event, json),
                None => {
                    if reconnected {
                        if !json { ui::warn("Daemon WS channel closed again; exiting."); }
                        return exit_codes::EXIT_ERROR;
                    }
                    if !json {
                        ui::warn("Daemon connection lost — reconnecting...");
                    }
                    let new_service = match wait_and_reconnect_daemon(RECONNECT_TIMEOUT).await {
                        Ok(s) => s,
                        Err(code) => return code,
                    };
                    rx = match new_service.subscribe_inbound_notices().await {
                        Ok(new_rx) => new_rx,
                        Err(err) => {
                            ui::error(&format!("Failed to re-subscribe after reconnect: {err}"));
                            return exit_codes::EXIT_ERROR;
                        }
                    };
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

fn render_daemon_notice(event: &InboundNoticeEvent, json: bool) {
    let text_preview = event.text_preview.clone();
    let rep_summary =
        (!event.representations.is_empty()).then(|| rep_summary_line(&event.representations));

    if json {
        #[derive(Serialize)]
        struct DaemonNoticeDto {
            from_device: String,
            snapshot_hash: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            text: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            rep_summary: Option<String>,
            action: String,
            at_ms: i64,
        }
        let dto = DaemonNoticeDto {
            from_device: event.from_device.clone(),
            snapshot_hash: event.snapshot_hash.clone(),
            text: text_preview.clone(),
            rep_summary: rep_summary.clone(),
            action: event.action.clone(),
            at_ms: event.at_ms,
        };
        if let Ok(line) = serde_json::to_string(&dto) {
            println!("{line}");
        }
        return;
    }

    let body = match text_preview {
        Some(t) => truncate_preview(&t),
        None => rep_summary.unwrap_or_else(|| "(undecodable envelope)".to_string()),
    };
    ui::info(
        "·",
        &format!("[{}] {} ({})", event.from_device, body, event.action),
    );
}

fn emit_watch_ready() {
    use std::io::Write;
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "WATCH_READY");
    let _ = err.flush();
}

/// One-line summary when the envelope has only non-text reps (e.g.
/// image/png). Useful for operator eyeballing; not meant for parsing.
fn rep_summary_line(representations: &[InboundRepresentationSummaryDto]) -> String {
    let parts: Vec<String> = representations
        .iter()
        .map(|rep| {
            let mime = rep.mime_type.as_deref().unwrap_or("?");
            format!("{}/{}B", mime, rep.size_bytes)
        })
        .collect();
    format!(
        "[envelope:{} rep(s) {}]",
        representations.len(),
        parts.join(", ")
    )
}

fn truncate_preview(text: &str) -> String {
    const MAX: usize = 120;
    let single_line = text.replace('\n', "\\n");
    if single_line.chars().count() > MAX {
        let truncated: String = single_line.chars().take(MAX).collect();
        format!("{truncated}…")
    } else {
        single_line
    }
}
