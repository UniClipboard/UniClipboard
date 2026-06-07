//! `uniclip watch` — foreground inbound clipboard observer
//! (Slice 2 Phase 2 · T11).
//!
//! Self-contained direct-mode command (no daemon). The backing
//! `SpaceSetupAssembly` auto-spawns the ingest loop at construction
//! (Phase 2 · T10), so this command's job is purely to subscribe to the
//! application-level notice broadcast and render each delivery until
//! Ctrl-C.
//!
//! Phase 2 deliberately does **not** write to the system clipboard
//! (plan §5.3): a short-lived CLI process writing the OS clipboard would
//! collide with the daemon's own watcher and trigger a sync echo. Daemon
//! integration arrives in Phase 3 / Slice 4.

use serde::Serialize;

use uc_core::clipboard::normalize_wire_mime;
use uc_core::ids::{FormatId, RepresentationId};
use uc_core::network::protocol::ClipboardBinaryPayload;
use uc_core::{ObservedClipboardRepresentation, SystemClipboardSnapshot};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use uc_daemon_client::DaemonService;
use uc_daemon_contract::api::dto::clipboard_command::InboundNoticeEvent;

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
    let plaintext_bytes = STANDARD.decode(&event.plaintext_base64).ok();
    let snapshot = plaintext_bytes
        .as_deref()
        .and_then(|b| decode_v3_envelope(b).ok());
    let text_preview = snapshot.as_ref().and_then(first_text_preview);
    let rep_summary = snapshot.as_ref().map(rep_summary_line);

    if json {
        #[derive(Serialize)]
        struct DaemonNoticeDto {
            from_device: String,
            content_hash: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            text: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            rep_summary: Option<String>,
            action: String,
            at_ms: i64,
        }
        let dto = DaemonNoticeDto {
            from_device: event.from_device.clone(),
            content_hash: event.content_hash.clone(),
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

/// Return the first `text/*`-mime representation's UTF-8 string (if any).
fn first_text_preview(snapshot: &SystemClipboardSnapshot) -> Option<String> {
    for rep in &snapshot.representations {
        let is_text = rep
            .mime
            .as_ref()
            .map(|m| {
                let s = m.as_str();
                s.eq_ignore_ascii_case("text/plain")
                    || s.eq_ignore_ascii_case("public.utf8-plain-text")
                    || s.to_ascii_lowercase().starts_with("text/")
            })
            .unwrap_or(false);
        if !is_text {
            continue;
        }
        if let Ok(s) = std::str::from_utf8(rep.inline_bytes().unwrap_or(&[])) {
            return Some(s.to_string());
        }
    }
    None
}

/// One-line summary when the envelope has only non-text reps (e.g.
/// image/png). Useful for operator eyeballing; not meant for parsing.
fn rep_summary_line(snapshot: &SystemClipboardSnapshot) -> String {
    let parts: Vec<String> = snapshot
        .representations
        .iter()
        .map(|rep| {
            let mime = rep.mime.as_ref().map(|m| m.as_str()).unwrap_or("?");
            format!("{}/{}B", mime, rep.size_bytes())
        })
        .collect();
    format!(
        "[envelope:{} rep(s) {}]",
        snapshot.representations.len(),
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

/// Minimal V3 envelope decoder using only `uc-core` types. Avoids pulling
/// the heavy `uc-application` crate (and its transitive iroh dependency)
/// just for client-side display rendering.
fn decode_v3_envelope(bytes: &[u8]) -> anyhow::Result<SystemClipboardSnapshot> {
    let mut cursor = bytes;
    let payload = ClipboardBinaryPayload::decode_from(&mut cursor)
        .map_err(|e| anyhow::anyhow!("decode V3 envelope: {e}"))?;

    let representations = payload
        .representations
        .into_iter()
        .map(|rep| {
            ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from(rep.format_id),
                normalize_wire_mime(rep.mime),
                rep.data,
            )
        })
        .collect();

    Ok(SystemClipboardSnapshot {
        ts_ms: payload.ts_ms,
        representations,
    })
}
