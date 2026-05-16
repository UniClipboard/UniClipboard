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
use tokio::sync::broadcast;

use uc_application::decode_v3_bytes_to_snapshot;
use uc_application::facade::space_setup::TryResumeSessionError;
use uc_application::facade::{InboundAction, InboundNotice};
use uc_core::SystemClipboardSnapshot;

use crate::commands::app_session::{build_app_session, refuse_if_daemon_running};
use crate::exit_codes;
use crate::ui;

pub async fn run(json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Watch inbound clipboard");
    }

    if let Err(code) = refuse_if_daemon_running().await {
        return code;
    }

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

    // Refresh presence so the sender side, when it dispatches, doesn't
    // skip us under "Unknown" — same routine as `members` / `send`.
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
        ui::info("status", "Listening — press Ctrl-C to stop");
        ui::bar();
    }
    // Machine-readable handshake marker: scripts (e.g. the single-machine
    // clipboard e2e) can wait for this on stderr before driving the
    // sender side. Without it, a sender racing the watch's subscribe
    // setup time emits notices to a not-yet-connected public broadcast
    // and the assertion times out. Explicit flush because stderr is
    // line-buffered when piped, but we want zero delay regardless.
    {
        use std::io::Write;
        let mut err = std::io::stderr().lock();
        let _ = writeln!(err, "WATCH_READY");
        let _ = err.flush();
    }

    let exit_code = loop {
        tokio::select! {
            biased;
            _ = tokio::signal::ctrl_c() => {
                if !json {
                    ui::end("Stopped");
                }
                break exit_codes::EXIT_SUCCESS;
            }
            recv = rx.recv() => {
                match recv {
                    Ok(notice) => render_notice(&notice, json),
                    Err(broadcast::error::RecvError::Lagged(missed)) => {
                        if !json {
                            ui::warn(&format!(
                                "Lagged: dropped {missed} notice(s); next frame catches up"
                            ));
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        if !json {
                            ui::warn("Inbound channel closed; exiting.");
                        }
                        break exit_codes::EXIT_ERROR;
                    }
                }
            }
        }
    };

    cli.shutdown().await;
    exit_code
}

fn render_notice(notice: &InboundNotice, json: bool) {
    // Phase 3 · T10:`notice.plaintext` is a V3 envelope, not raw text.
    // Decode and show the first text representation (falls back to a
    // per-rep summary if no text rep is present — e.g. image-only).
    let snapshot = decode_v3_bytes_to_snapshot(&notice.plaintext).ok();
    let text_preview = snapshot.as_ref().and_then(first_text_preview);
    let rep_summary = snapshot.as_ref().map(rep_summary_line);

    if json {
        let dto = NoticeDto::from_notice(notice, text_preview.clone(), rep_summary.clone());
        match serde_json::to_string(&dto) {
            Ok(line) => println!("{line}"),
            Err(err) => ui::error(&format!("Failed to serialize notice: {err}")),
        }
        return;
    }

    let body = match text_preview {
        Some(t) => truncate_preview(&t),
        None => rep_summary.unwrap_or_else(|| "(undecodable envelope)".to_string()),
    };
    let line = format!(
        "[{}] {} ({})",
        notice.from_device.as_str(),
        body,
        format_action(notice.action),
    );
    ui::info("·", &line);
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

fn format_action(action: InboundAction) -> &'static str {
    match action {
        InboundAction::NewEntry => "new",
        InboundAction::DuplicateIgnored => "duplicate",
    }
}

#[derive(Serialize)]
struct NoticeDto<'a> {
    from_device: &'a str,
    content_hash: &'a str,
    /// First `text/*` representation's UTF-8 content, if any. Absent for
    /// image-only / binary-only envelopes.
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    /// Per-representation summary `[envelope:N reps mime/Nbytes, ...]`
    /// for human / script eyeballing when the envelope has non-text
    /// reps. Present only when decode succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    rep_summary: Option<String>,
    action: &'static str,
    at_ms: i64,
}

impl<'a> NoticeDto<'a> {
    fn from_notice(
        n: &'a InboundNotice,
        text: Option<String>,
        rep_summary: Option<String>,
    ) -> Self {
        Self {
            from_device: n.from_device.as_str(),
            content_hash: &n.content_hash,
            text,
            rep_summary,
            action: format_action(n.action),
            at_ms: n.at_ms,
        }
    }
}
