//! `uniclip send` — one-shot clipboard dispatch.
//!
//! Self-contained direct-mode command (no daemon), mirroring the `init` /
//! `invite` / `join` / `members` pattern. Builds a `SpaceSetupAssembly`,
//! resumes the cached session, refreshes presence so dispatch can route to
//! online peers, then wraps the user-supplied text as a single-text-rep
//! `SystemClipboardSnapshot` and fans it out via
//! `ClipboardSyncFacade::dispatch_snapshot`.
//!
//! Phase 3 upgrade(T9):text → V3 envelope。Phase 2 raw-bytes path retired
//! so daemon-on-the-other-side(Phase 3 T7/T8)can decode us uniformly.
//! CLI callers with daemon receivers will now interoperate natively.
//!
//! Still doesn't read the system clipboard — bootstrap sets
//! `UC_DISABLE_SYSTEM_CLIPBOARD=1` so non-bundled CLI runs don't panic
//! in `+[NSPasteboard generalPasteboard]`. The daemon owns the OS
//! clipboard in production.

use std::io::Read;
use std::path::PathBuf;

use serde::Serialize;

use uc_application::facade::space_setup::TryResumeSessionError;
use uc_application::facade::{
    BlobTransferError, ClipboardSyncError, DispatchEntryOutcome, DispatchEntryPerTarget,
    PublishBlobPathCommand,
};
use uc_application::V3BlobRef;
use uc_core::ids::{EntryId, FormatId, RepresentationId};
use uc_core::ports::DispatchAck;
use uc_core::{
    ClipboardChangeOrigin, MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot,
};

use crate::commands::app_session::{build_app_session, refuse_if_daemon_running, CliAppSession};
use crate::exit_codes;
use crate::ui;

pub struct SendArgs {
    /// Plaintext to dispatch. When `None` and `file` is also `None`, the
    /// command reads text from stdin until EOF — handy for
    /// `echo hi | uniclip send` and the dual-profile test recipe.
    pub text: Option<String>,
    /// Path to a file to send instead of text. When set, the file is
    /// published as a blob, dispatched as a clipboard envelope referencing
    /// the blob, and the CLI keeps the iroh router alive (passive
    /// provider) until Ctrl-C so the receiver has time to fetch.
    pub file: Option<PathBuf>,
}

pub async fn run(args: SendArgs, json: bool, verbose: bool) -> i32 {
    if let Some(file) = args.file {
        if args.text.is_some() {
            ui::error("Cannot pass both `--file` and a positional text argument.");
            return exit_codes::EXIT_ERROR;
        }
        return run_send_file(file, json, verbose).await;
    }

    if !json {
        ui::header("Send clipboard");
    }

    if let Err(code) = refuse_if_daemon_running().await {
        return code;
    }

    let plaintext = match read_plaintext(args.text) {
        Ok(text) if text.is_empty() => {
            ui::error("Empty plaintext — nothing to send.");
            return exit_codes::EXIT_ERROR;
        }
        Ok(text) => text,
        Err(msg) => {
            ui::error(&msg);
            return exit_codes::EXIT_ERROR;
        }
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

    // Phase 3 · T9:wrap the CLI text into a single-representation
    // `SystemClipboardSnapshot` with `text/plain` mime — same shape the
    // daemon would emit if the user copied this text through a real
    // `SystemClipboardPort`. `dispatch_snapshot` handles V3 envelope
    // encoding + canonical `snapshot_hash` (which matches the receiver's
    // local `clipboard_event.snapshot_hash` for dedup).
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
        .dispatch_clipboard_snapshot(snapshot, ClipboardChangeOrigin::LocalCapture)
        .await;

    let outcome = match outcome {
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
        let dto = SendOutcomeDto::from_outcome(&outcome);
        match serde_json::to_string_pretty(&dto) {
            Ok(s) => println!("{s}"),
            Err(err) => {
                ui::error(&format!("Failed to serialize outcome: {err}"));
                cli.shutdown().await;
                return exit_codes::EXIT_ERROR;
            }
        }
    } else {
        render_human(&outcome);
    }

    cli.shutdown().await;
    if outcome.total_accepted == 0 && outcome.total_duplicate == 0 {
        // Nothing actually landed: at least one peer was online but every
        // attempt errored (or no peers paired at all). Surface as non-zero
        // so dual-profile shell harness can detect failure.
        exit_codes::EXIT_ERROR
    } else {
        exit_codes::EXIT_SUCCESS
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

fn render_human(outcome: &DispatchEntryOutcome) {
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
struct SendOutcomeDto<'a> {
    content_hash: &'a str,
    total_accepted: usize,
    total_duplicate: usize,
    total_offline: usize,
    total_errored: usize,
    at_ms: i64,
    per_target: Vec<PerTargetDto<'a>>,
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
    fn from_outcome(o: &'a DispatchEntryOutcome) -> Self {
        Self {
            content_hash: &o.content_hash,
            total_accepted: o.total_accepted,
            total_duplicate: o.total_duplicate,
            total_offline: o.total_offline,
            total_errored: o.total_errored,
            at_ms: o.at_ms,
            per_target: o
                .per_target
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
        }
    }
}

/// `send -f <path>` 路径。
///
/// 与文本路径不同:dispatch 返回后必须**保持进程驻留**。`publish_blob_path`
/// 只是把文件加进本地 iroh-blobs store + 把 ticket 编进 V3 envelope;真正
/// 把字节"传"出去的是 receiver 端的 fetch task 反向拉取。这里 CLI 进程作
/// 为 passive provider,直到收到 Ctrl-C 才能 shutdown(否则 iroh router
/// drop 就会把 connection 撕掉,receiver 拿到的就是 partial file)。
async fn run_send_file(path: PathBuf, json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Send file");
    }

    // metadata 在 daemon 检测前先看 —— 路径明显错的话不要白启 session。
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

    // publish_blob_path:流式入库,内存峰值与文件大小无关。EntryId 是 sender
    // 端 entry 归属;CLI 这次发送不与本地 entry 绑定 (CLI 不写本地剪贴板),
    // 但 V3 envelope 仍然需要一个稳定的 entry_id 让 receiver 端可以 dedup
    // / 索引,所以这里 mint 一个新的。
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

    // V3 envelope 主体:一个 file-uri-list rep 指向 abs_path —— receiver 端
    // 的 V3 decoder 会看到这个 rep 配合尾部 blob_ref。free-file 形态:
    // `representation_index = None`,blob 是独立文件,receiver 用 filename
    // 落地到 cache;不是把 bytes 回填到某个 rep。
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

    // 关键:**保持进程**直到 Ctrl-C。dispatch 已经返回,但 receiver 的 fetch
    // 任务可能还没开始 (presence 慢) 或正在中段 —— 一旦本进程 shutdown,
    // iroh-blobs Router 跟着 drop,connection 撕掉,receiver 拿到 partial file。
    let _ = tokio::signal::ctrl_c().await;
    if !json {
        ui::end("Stopped");
    }
    cli.shutdown().await;
    exit_codes::EXIT_SUCCESS
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
