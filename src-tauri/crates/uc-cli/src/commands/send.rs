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
use std::path::PathBuf;

use serde::Serialize;

use uc_application::facade::space_setup::TryResumeSessionError;
use uc_application::facade::{
    BlobTransferError, ClipboardSyncError, DispatchEntryOutcome, DispatchEntryPerTarget,
    NotResendableReason, PublishBlobPathCommand, ResendEntryCommand, ResendEntryError,
    ResendReport, V3BlobRef,
};
use uc_core::ids::{DeviceId, EntryId, FormatId, RepresentationId};
use uc_core::ports::DispatchAck;
use uc_core::{
    ClipboardChangeOrigin, MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot,
};

use uc_daemon_client::DaemonService;
use uc_daemon_contract::api::dto::clipboard_command::DispatchOutcomeResponse;

use crate::commands::app_session::{
    build_app_session, refuse_if_daemon_running, resolve_execution_mode, CliAppSession,
    CliExecutionMode,
};
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
        return run_send_file(file, json, verbose).await;
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

    let exec_mode = match resolve_execution_mode(verbose).await {
        Ok(m) => m,
        Err(code) => return code,
    };

    match exec_mode {
        CliExecutionMode::DaemonClient(service) => {
            run_send_via_daemon(&*service, mode, plaintext, args.resend, peers_str, json).await
        }
        CliExecutionMode::InProcess(cli) => {
            run_send_in_process(cli, mode, plaintext, args.resend, &args.peers, json).await
        }
    }
}

async fn run_send_via_daemon(
    service: &dyn DaemonService,
    mode: SendMode,
    plaintext: Option<String>,
    resend_id: Option<String>,
    peers: Option<Vec<String>>,
    json: bool,
) -> i32 {
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

async fn run_send_in_process(
    cli: CliAppSession,
    mode: SendMode,
    plaintext: Option<String>,
    resend_id: Option<String>,
    peers: &[String],
    json: bool,
) -> i32 {
    let target_filter: Option<Vec<DeviceId>> = if peers.is_empty() {
        None
    } else {
        Some(peers.iter().map(DeviceId::new).collect())
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

    let view = match mode {
        SendMode::New => {
            let plaintext = plaintext.expect("plaintext populated in new-entry mode");
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
            let entry_id_str = resend_id.expect("resend id present");
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
                exit_codes::EXIT_ERROR
            } else {
                exit_codes::EXIT_SUCCESS
            }
        }
        SendOutcomeView::Resend { report, .. } => {
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
