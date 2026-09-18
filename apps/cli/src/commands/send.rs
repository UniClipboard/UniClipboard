//! `uniclip send` — text, file, and resend dispatch through the daemon.

use std::io::Read;
use std::path::PathBuf;

use serde::Serialize;

use uc_daemon_client::DaemonService;
use uc_daemon_contract::api::dto::clipboard_command::{
    DispatchOutcomeResponse, PerTargetOutcomeDto,
};
use uc_daemon_contract::api::dto::clipboard_delivery::{
    EntryDeliveryStatusDto, EntryDeliveryTargetDto, EntryDeliveryViewDto,
};

use crate::commands::app_session::connect_or_spawn_oneshot_daemon;
use crate::exit_codes;
use crate::ui;

pub struct SendArgs {
    /// Plaintext to dispatch in **new-entry** mode. When `None` and
    /// neither `file` nor `resend` is set, the command reads from stdin
    /// until EOF — handy for `echo hi | uniclip send` and the
    /// dual-profile test recipe. Ignored when `resend` or `file` is set
    /// (clap enforces mutual exclusion at the parser layer).
    pub input: Option<String>,
    /// Whether `--text` explicitly disables path auto-detection.
    pub force_text: bool,
    /// Path to a file to send instead of text. Mutually exclusive with
    /// positional text and `--resend`. The daemon owns the blob provider and
    /// the CLI waits for the selected targets to reach terminal states.
    pub file: Option<PathBuf>,
    /// Entry id to **resend**. When set, the daemon pulls the original snapshot.
    pub resend: Option<String>,
    /// Optional list of target device IDs. Empty vec means "no filter"
    /// (full fan-out for new entry; derived diff for resend).
    pub peers: Vec<String>,
}

pub async fn run(args: SendArgs, json: bool, verbose: bool) -> i32 {
    let mode = if args.resend.is_some() {
        SendMode::Resend
    } else {
        SendMode::New
    };

    if !json {
        ui::header(mode.header());
    }

    if args.resend.is_some() && args.input.is_some() {
        ui::error("--resend cannot be combined with positional text.");
        return exit_codes::EXIT_ERROR;
    }

    let input = match mode {
        SendMode::Resend => None,
        SendMode::New => match classify_input(args.input, args.file, args.force_text) {
            Ok(SendInput::Stdin) => match read_plaintext(None) {
                Ok(text) if text.is_empty() => {
                    ui::error("Empty plaintext — nothing to send.");
                    return exit_codes::EXIT_ERROR;
                }
                Ok(text) => Some(SendInput::Text(text)),
                Err(message) => {
                    ui::error(&message);
                    return exit_codes::EXIT_ERROR;
                }
            },
            Ok(SendInput::Text(text)) if text.is_empty() => {
                ui::error("Empty plaintext — nothing to send.");
                return exit_codes::EXIT_ERROR;
            }
            Ok(input) => Some(input),
            Err(message) => {
                ui::error(&message);
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
    match input {
        Some(SendInput::File(path)) => {
            run_send_file_via_daemon(&*service, path, peers_str, json).await
        }
        Some(SendInput::Text(text)) => {
            run_send_via_daemon(&*service, mode, Some(text), args.resend, peers_str, json).await
        }
        Some(SendInput::Stdin) => unreachable!("stdin is resolved before daemon connection"),
        None => run_send_via_daemon(&*service, mode, None, args.resend, peers_str, json).await,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SendInput {
    Stdin,
    Text(String),
    File(PathBuf),
}

fn classify_input(
    positional: Option<String>,
    explicit_file: Option<PathBuf>,
    force_text: bool,
) -> Result<SendInput, String> {
    if let Some(path) = explicit_file {
        return classify_file(path);
    }
    let Some(value) = positional else {
        return Ok(SendInput::Stdin);
    };
    if force_text {
        return Ok(SendInput::Text(value));
    }
    let path = PathBuf::from(&value);
    match std::fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => classify_file(path),
        Ok(metadata) if metadata.is_dir() => Err(format!(
            "Directory sending is not supported: {}",
            path.display()
        )),
        Ok(_) => Err(format!("Path is not a regular file: {}", path.display())),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound && looks_like_path(&path, &value) =>
        {
            Err(format!("Path does not exist: {}", path.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(SendInput::Text(value)),
        Err(error) => Err(format!(
            "Failed to inspect path {}: {error}",
            path.display()
        )),
    }
}

fn classify_file(path: PathBuf) -> Result<SendInput, String> {
    let metadata = std::fs::metadata(&path)
        .map_err(|error| format!("Failed to inspect file {}: {error}", path.display()))?;
    if metadata.is_dir() {
        return Err(format!(
            "Directory sending is not supported: {}",
            path.display()
        ));
    }
    if !metadata.is_file() {
        return Err(format!("Path is not a regular file: {}", path.display()));
    }
    path.canonicalize()
        .map(SendInput::File)
        .map_err(|error| format!("Failed to resolve file path {}: {error}", path.display()))
}

fn looks_like_path(path: &std::path::Path, raw: &str) -> bool {
    path.is_absolute()
        || raw.starts_with("./")
        || raw.starts_with("../")
        || raw.starts_with(".\\")
        || raw.starts_with("..\\")
        || raw.contains('/')
        || raw.contains('\\')
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
    ui::info("hash", short_hash(&resp.snapshot_hash));
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

// ── File send through the daemon ──────────────────────────────────────

async fn run_send_file_via_daemon(
    service: &dyn DaemonService,
    path: PathBuf,
    peers: Option<Vec<String>>,
    json: bool,
) -> i32 {
    let Some(source_path) = path.to_str() else {
        ui::error("File path is not valid Unicode.");
        return exit_codes::EXIT_ERROR;
    };
    let metadata = match std::fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) => {
            ui::error(&format!("Failed to inspect file: {error}"));
            return exit_codes::EXIT_ERROR;
        }
    };
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
        .to_string();
    let _lease = match service.hold_control_lease().await {
        Ok(lease) => lease,
        Err(error) => {
            ui::error(&format!("Failed to hold daemon session lease: {error}"));
            return exit_codes::EXIT_ERROR;
        }
    };
    let spinner = ui::spinner("Dispatching file via daemon...");
    let outcome = match service.dispatch_file(source_path, peers).await {
        Ok(outcome) => outcome,
        Err(error) => {
            ui::spinner_finish_error(&spinner, &format!("File send failed: {error}"));
            return exit_codes::EXIT_ERROR;
        }
    };
    ui::spinner_finish_success(
        &spinner,
        &format!(
            "{} accepted, {} duplicate, {} offline, {} error(s)",
            outcome.total_accepted,
            outcome.total_duplicate,
            outcome.total_offline,
            outcome.total_errored
        ),
    );

    let accepted_targets: std::collections::HashSet<String> = outcome
        .per_target
        .iter()
        .filter(|target| target.outcome == "accepted")
        .map(|target| target.device_id.clone())
        .collect();
    let related_targets: std::collections::HashSet<String> = outcome
        .per_target
        .iter()
        .map(|target| target.device_id.clone())
        .collect();
    let delivery = if accepted_targets.is_empty() {
        None
    } else {
        match wait_for_file_delivery(service, &outcome.entry_id, &accepted_targets).await {
            Ok(view) => Some(view),
            Err(WaitError::Cancelled) => {
                ui::warn("Cancelled while waiting; the daemon may continue active transfers.");
                return exit_codes::EXIT_ERROR;
            }
            Err(WaitError::Request(error)) => {
                ui::error(&format!("Failed to query file delivery: {error}"));
                return exit_codes::EXIT_ERROR;
            }
        }
    };

    let result = SendFileOutcomeDto {
        entry_id: outcome.entry_id,
        snapshot_hash: outcome.snapshot_hash,
        filename,
        size_bytes: metadata.len(),
        total_accepted: outcome.total_accepted,
        total_duplicate: outcome.total_duplicate,
        total_offline: outcome.total_offline,
        total_errored: outcome.total_errored,
        per_target: outcome.per_target,
        deliveries: delivery
            .as_ref()
            .map(|view| {
                view.deliveries
                    .iter()
                    .filter(|target| related_targets.contains(&target.target_device_id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
    };
    if json {
        match serde_json::to_string_pretty(&result) {
            Ok(value) => println!("{value}"),
            Err(error) => {
                ui::error(&format!("Failed to serialize outcome: {error}"));
                return exit_codes::EXIT_ERROR;
            }
        }
    } else {
        ui::bar();
        ui::info("file", &result.filename);
        ui::info("size", &human_size(result.size_bytes));
        ui::info("hash", short_hash(&result.snapshot_hash));
        for target in &result.deliveries {
            ui::info(
                "·",
                &format!(
                    "{} → {}",
                    target.target_device_id,
                    delivery_status_label(&target.status)
                ),
            );
        }
        if result.deliveries.is_empty() {
            for target in &result.per_target {
                ui::info("·", &format!("{} → {}", target.device_id, target.outcome));
            }
        }
        ui::bar();
        ui::end("File send finished");
    }
    if result.total_accepted == 0 && result.total_duplicate == 0 {
        exit_codes::EXIT_ERROR
    } else if result
        .deliveries
        .iter()
        .any(|target| matches!(target.status, EntryDeliveryStatusDto::Failed { .. }))
    {
        exit_codes::EXIT_ERROR
    } else {
        exit_codes::EXIT_SUCCESS
    }
}

#[derive(Debug)]
enum WaitError {
    Cancelled,
    Request(anyhow::Error),
}

async fn wait_for_file_delivery(
    service: &dyn DaemonService,
    entry_id: &str,
    accepted_targets: &std::collections::HashSet<String>,
) -> Result<EntryDeliveryViewDto, WaitError> {
    loop {
        let view = service
            .entry_delivery(entry_id)
            .await
            .map_err(WaitError::Request)?;
        if all_targets_terminal(&view, accepted_targets) {
            return Ok(view);
        }
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Err(WaitError::Cancelled),
            _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {}
        }
    }
}

fn all_targets_terminal(
    view: &EntryDeliveryViewDto,
    target_ids: &std::collections::HashSet<String>,
) -> bool {
    target_ids.iter().all(|target_id| {
        view.deliveries
            .iter()
            .find(|delivery| delivery.target_device_id == *target_id)
            .is_some_and(|delivery| is_terminal_delivery(&delivery.status))
    })
}

fn is_terminal_delivery(status: &EntryDeliveryStatusDto) -> bool {
    !matches!(status, EntryDeliveryStatusDto::Pending)
}

fn delivery_status_label(status: &EntryDeliveryStatusDto) -> &'static str {
    match status {
        EntryDeliveryStatusDto::Pending => "pending",
        EntryDeliveryStatusDto::Delivered => "delivered",
        EntryDeliveryStatusDto::Duplicate => "duplicate",
        EntryDeliveryStatusDto::Unreachable => "offline",
        EntryDeliveryStatusDto::Superseded => "superseded",
        EntryDeliveryStatusDto::Failed { .. } => "failed",
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SendFileOutcomeDto {
    entry_id: String,
    snapshot_hash: String,
    filename: String,
    size_bytes: u64,
    total_accepted: usize,
    total_duplicate: usize,
    total_offline: usize,
    total_errored: usize,
    per_target: Vec<PerTargetOutcomeDto>,
    deliveries: Vec<EntryDeliveryTargetDto>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_file_is_detected_but_force_text_wins() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("résumé file.txt");
        std::fs::write(&path, b"hello").unwrap();
        let raw = path.to_string_lossy().into_owned();

        assert!(matches!(
            classify_input(Some(raw.clone()), None, false).unwrap(),
            SendInput::File(_)
        ));
        assert_eq!(
            classify_input(Some(raw.clone()), None, true).unwrap(),
            SendInput::Text(raw)
        );
    }

    #[test]
    fn directory_and_obvious_missing_path_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let directory_error = classify_input(
            Some(directory.path().to_string_lossy().into_owned()),
            None,
            false,
        )
        .unwrap_err();
        assert!(directory_error.contains("Directory sending is not supported"));

        let missing_error = classify_input(Some("./missing.pdf".into()), None, false).unwrap_err();
        assert!(missing_error.contains("Path does not exist"));
        assert_eq!(
            classify_input(Some("hello world".into()), None, false).unwrap(),
            SendInput::Text("hello world".into())
        );
    }

    #[test]
    fn all_non_pending_delivery_states_are_terminal() {
        assert!(!is_terminal_delivery(&EntryDeliveryStatusDto::Pending));
        assert!(is_terminal_delivery(&EntryDeliveryStatusDto::Delivered));
        assert!(is_terminal_delivery(&EntryDeliveryStatusDto::Unreachable));
        assert!(is_terminal_delivery(&EntryDeliveryStatusDto::Failed {
            reason: uc_daemon_contract::api::dto::clipboard_delivery::DeliveryFailureReasonDto::Io,
        }));
    }

    #[test]
    fn multiple_targets_do_not_finish_when_only_the_first_is_terminal() {
        use uc_daemon_contract::api::dto::clipboard_delivery::{
            EntryDeliveryTargetDto, EntrySourceDto,
        };

        let targets = ["device-a".to_string(), "device-b".to_string()]
            .into_iter()
            .collect();
        let mut view = EntryDeliveryViewDto {
            entry_id: "entry-1".into(),
            source: EntrySourceDto::Local,
            deliveries: vec![
                EntryDeliveryTargetDto {
                    target_device_id: "device-a".into(),
                    target_device_name: None,
                    status: EntryDeliveryStatusDto::Delivered,
                    reason_detail: None,
                    updated_at_ms: Some(1),
                },
                EntryDeliveryTargetDto {
                    target_device_id: "device-b".into(),
                    target_device_name: None,
                    status: EntryDeliveryStatusDto::Pending,
                    reason_detail: None,
                    updated_at_ms: None,
                },
            ],
        };
        assert!(!all_targets_terminal(&view, &targets));
        view.deliveries[1].status = EntryDeliveryStatusDto::Delivered;
        assert!(all_targets_terminal(&view, &targets));
    }
}
