//! `uniclip send` — text, file, and resend dispatch through the daemon.

use std::collections::{HashMap, HashSet};
use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::time::Duration;

use indicatif::ProgressBar;
use serde::Serialize;
use tokio::sync::mpsc;

use uc_daemon_client::realtime::FileTransferProgressEvent;
use uc_daemon_client::{
    ControlLeaseGuard, DaemonClientContext, DaemonService, InboundActivityEvent,
};
use uc_daemon_contract::api::dto::clipboard_command::{
    DispatchOutcomeResponse, PerTargetOutcomeDto,
};
use uc_daemon_contract::api::dto::clipboard_delivery::{
    EntryDeliveryStatusDto, EntryDeliveryTargetDto, EntryDeliveryViewDto,
};
use uc_daemon_contract::api::types::{FileTransferDirection, PeerSnapshotDto};

use crate::commands::app_session::connect_or_spawn_oneshot_daemon_until;
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
    /// Whether file mode is enabled. A positional value is one path; without
    /// one, stdin supplies one complete path per line.
    pub file: bool,
    /// Entry id to **resend**. When set, the daemon pulls the original snapshot.
    pub resend: Option<String>,
    /// Optional list of target device IDs. Empty vec means "no filter"
    /// (full fan-out for new entry; derived diff for resend).
    pub peers: Vec<String>,
    /// Total wait budget for daemon readiness and target-device connection
    /// before dispatch. `None` keeps the legacy behavior: default daemon
    /// startup budget and no wait for target devices.
    pub connect_timeout: Option<Duration>,
}

/// Poll interval while waiting for target devices to connect.
const TARGET_POLL_INTERVAL: Duration = Duration::from_millis(500);

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
            Ok(SendInput::StdinFiles) => match read_file_paths_from_stdin() {
                Ok(paths) => Some(SendInput::Files(paths)),
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

    // One absolute deadline bounds daemon readiness and target connection so
    // no stage can restart the clock. Dispatch happens at most once, after
    // this phase, so a wait can never duplicate a send.
    let deadline = args
        .connect_timeout
        .map(|timeout| tokio::time::Instant::now() + timeout);
    let wait_for_targets = deadline.is_some() && matches!(mode, SendMode::New);
    let prepared = tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            ui::warn("Cancelled before dispatch; nothing was sent.");
            return exit_codes::EXIT_ERROR;
        }
        prepared = prepare_dispatch(
            verbose,
            deadline,
            wait_for_targets.then_some(args.peers.as_slice()),
            args.connect_timeout.unwrap_or_default(),
        ) => prepared,
    };
    // Hold the control-WS lease to the end of the command (bind to a named
    // var, NOT `_`) so a transient Oneshot daemon does not self-terminate
    // mid-fan-out (ADR-008 P5-1a).
    let (service, _lease) = match prepared {
        Ok(prepared) => prepared,
        Err(code) => return code,
    };
    match input {
        Some(SendInput::File(path)) => {
            run_send_file_via_daemon(&*service, path, peers_str, json, true)
                .await
                .exit_code
        }
        Some(SendInput::Files(paths)) => {
            run_send_files_via_daemon(&*service, paths, peers_str, json).await
        }
        Some(SendInput::Text(text)) => {
            run_send_via_daemon(&*service, mode, Some(text), args.resend, peers_str, json).await
        }
        Some(SendInput::Stdin | SendInput::StdinFiles) => {
            unreachable!("stdin is resolved before daemon connection")
        }
        None => run_send_via_daemon(&*service, mode, None, args.resend, peers_str, json).await,
    }
}

/// Connect to the daemon (spawning a transient one when absent), take the
/// control lease, then optionally wait for the send targets to connect.
async fn prepare_dispatch(
    verbose: bool,
    deadline: Option<tokio::time::Instant>,
    wait_for_peers: Option<&[String]>,
    budget: Duration,
) -> Result<(Box<dyn DaemonService>, ControlLeaseGuard), i32> {
    let service = connect_or_spawn_oneshot_daemon_until(verbose, deadline).await?;
    let lease = service.hold_control_lease().await.map_err(|error| {
        ui::error(&format!("Failed to hold daemon session lease: {error}"));
        exit_codes::EXIT_ERROR
    })?;
    if let (Some(requested), Some(deadline)) = (wait_for_peers, deadline) {
        wait_for_targets(requested, deadline, budget).await?;
    }
    Ok((service, lease))
}

#[derive(Debug, PartialEq, Eq)]
enum TargetReadiness {
    /// Enough targets are connected to dispatch.
    Ready,
    /// No target is known, so waiting cannot help; dispatch reports it.
    NothingToWaitFor,
    /// Still waiting; carries a human-readable reason.
    Waiting(String),
}

/// Decide whether the send targets are connected, using only the daemon's
/// reported `connected` state. With `--peer`, every listed device must be
/// connected. Without it, one connected paired device is enough.
fn target_readiness(peers: &[PeerSnapshotDto], requested: &[String]) -> TargetReadiness {
    if requested.is_empty() {
        let paired: Vec<&PeerSnapshotDto> = peers.iter().filter(|peer| peer.is_paired).collect();
        if paired.is_empty() {
            return TargetReadiness::NothingToWaitFor;
        }
        if paired.iter().any(|peer| peer.connected) {
            return TargetReadiness::Ready;
        }
        return TargetReadiness::Waiting(format!(
            "none of {} paired device(s) is connected",
            paired.len()
        ));
    }
    let pending: Vec<String> = requested
        .iter()
        .filter_map(|id| match peers.iter().find(|peer| peer.peer_id == *id) {
            Some(peer) if peer.connected => None,
            Some(_) => Some(format!("{id} (offline)")),
            None => Some(format!("{id} (unknown device)")),
        })
        .collect();
    if pending.is_empty() {
        TargetReadiness::Ready
    } else {
        TargetReadiness::Waiting(pending.join(", "))
    }
}

/// Wait until [`target_readiness`] is satisfied or `deadline` passes. Only
/// read-only queries are repeated; nothing is dispatched here.
async fn wait_for_targets(
    requested: &[String],
    deadline: tokio::time::Instant,
    budget: Duration,
) -> Result<(), i32> {
    let context = DaemonClientContext::from_env().map_err(|error| {
        ui::error(&format!("Failed to connect to daemon: {error}"));
        exit_codes::EXIT_ERROR
    })?;
    let query = context.query_client();
    let mut spinner: Option<ProgressBar> = None;
    loop {
        let waiting = match query.get_peers().await {
            Ok(peers) => match target_readiness(&peers, requested) {
                TargetReadiness::Ready | TargetReadiness::NothingToWaitFor => {
                    if let Some(spinner) = spinner {
                        spinner.finish_and_clear();
                    }
                    return Ok(());
                }
                TargetReadiness::Waiting(reason) => reason,
            },
            // The peer list is only an optimization for waiting. If it cannot
            // be read (no space yet, locked session, daemon error), stop
            // waiting and let dispatch report the authoritative error.
            Err(error) => {
                tracing::debug!(%error, "peer list unavailable; skipping target wait");
                if let Some(spinner) = spinner {
                    spinner.finish_and_clear();
                }
                return Ok(());
            }
        };
        if spinner.is_none() {
            spinner = Some(ui::spinner("Waiting for target device(s) to connect..."));
        }
        if tokio::time::Instant::now() >= deadline {
            if let Some(spinner) = spinner {
                spinner.finish_and_clear();
            }
            ui::error(&format!(
                "Timed out after {}s waiting for target device(s) to connect: {waiting}. \
                 Nothing was sent.",
                budget.as_secs()
            ));
            ui::warn(
                "Bring the device online, check `uniclip members`, or raise --connect-timeout.",
            );
            return Err(exit_codes::EXIT_ERROR);
        }
        let pause = TARGET_POLL_INTERVAL
            .min(deadline.saturating_duration_since(tokio::time::Instant::now()));
        tokio::time::sleep(pause).await;
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SendInput {
    Stdin,
    StdinFiles,
    Text(String),
    File(PathBuf),
    Files(Vec<PathBuf>),
}

fn classify_input(
    positional: Option<String>,
    force_file: bool,
    force_text: bool,
) -> Result<SendInput, String> {
    let Some(value) = positional else {
        return Ok(if force_file {
            SendInput::StdinFiles
        } else {
            SendInput::Stdin
        });
    };
    if force_file {
        return classify_file(PathBuf::from(value));
    }
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
    std::fs::File::open(&path)
        .map_err(|error| format!("File is not readable {}: {error}", path.display()))?;
    path.canonicalize()
        .map(SendInput::File)
        .map_err(|error| format!("Failed to resolve file path {}: {error}", path.display()))
}

fn parse_file_paths(input: &str) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();

    for raw_line in input.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() {
            continue;
        }
        let path = match classify_file(PathBuf::from(line))? {
            SendInput::File(path) => path,
            _ => unreachable!("classify_file only returns file input"),
        };
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }

    if paths.is_empty() {
        Err("No file paths were provided on stdin.".to_string())
    } else {
        Ok(paths)
    }
}

fn read_file_paths_from_stdin() -> Result<Vec<PathBuf>, String> {
    if std::io::stdin().is_terminal() {
        return Err(
            "File mode needs a path argument or file paths piped through stdin.".to_string(),
        );
    }
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| format!("Failed to read file paths from stdin: {error}"))?;
    parse_file_paths(&input)
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
    Ok(normalize_plaintext_stdin(buf))
}

fn normalize_plaintext_stdin(mut text: String) -> String {
    // Trim a single trailing newline so `echo foo | send` matches `send foo`.
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    text
}

fn short_hash(hash: &str) -> &str {
    if hash.len() > 16 {
        &hash[..16]
    } else {
        hash
    }
}

// ── File send through the daemon ──────────────────────────────────────

async fn run_send_files_via_daemon(
    service: &dyn DaemonService,
    paths: Vec<PathBuf>,
    peers: Option<Vec<String>>,
    json: bool,
) -> i32 {
    let mut batch = FileSendBatchResult::default();
    for path in paths {
        let result = run_send_file_via_daemon(service, path, peers.clone(), json, false).await;
        batch.push(result);
    }
    if json {
        match serialize_file_outcomes(&batch.outcomes) {
            Ok(value) => println!("{value}"),
            Err(error) => {
                ui::error(&format!("Failed to serialize outcomes: {error}"));
                return exit_codes::EXIT_ERROR;
            }
        }
    }
    batch.exit_code
}

fn serialize_file_outcomes(outcomes: &[SendFileOutcomeDto]) -> serde_json::Result<String> {
    serde_json::to_string_pretty(outcomes)
}

struct FileSendRunResult {
    exit_code: i32,
    outcome: Option<SendFileOutcomeDto>,
}

impl FileSendRunResult {
    fn error() -> Self {
        Self {
            exit_code: exit_codes::EXIT_ERROR,
            outcome: None,
        }
    }
}

struct FileSendBatchResult {
    exit_code: i32,
    outcomes: Vec<SendFileOutcomeDto>,
}

impl Default for FileSendBatchResult {
    fn default() -> Self {
        Self {
            exit_code: exit_codes::EXIT_SUCCESS,
            outcomes: Vec::new(),
        }
    }
}

impl FileSendBatchResult {
    fn push(&mut self, result: FileSendRunResult) {
        if result.exit_code != exit_codes::EXIT_SUCCESS {
            self.exit_code = result.exit_code;
        }
        if let Some(outcome) = result.outcome {
            self.outcomes.push(outcome);
        }
    }
}

async fn run_send_file_via_daemon(
    service: &dyn DaemonService,
    path: PathBuf,
    peers: Option<Vec<String>>,
    json: bool,
    emit_json: bool,
) -> FileSendRunResult {
    let Some(source_path) = path.to_str() else {
        ui::error("File path is not valid Unicode.");
        return FileSendRunResult::error();
    };
    let metadata = match std::fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) => {
            ui::error(&format!("Failed to inspect file: {error}"));
            return FileSendRunResult::error();
        }
    };
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
        .to_string();
    // Subscribe before dispatch so no early progress is missed. Progress is
    // optional: a failed subscription never blocks the send.
    let mut activity = match service.subscribe_inbound_activity().await {
        Ok(rx) => Some(rx),
        Err(error) => {
            tracing::debug!(%error, "file progress subscription unavailable");
            None
        }
    };
    let spinner = ui::spinner("Dispatching file via daemon...");
    let outcome = match service.dispatch_file(source_path, peers).await {
        Ok(outcome) => outcome,
        Err(error) => {
            ui::spinner_finish_error(&spinner, &format!("File send failed: {error}"));
            return FileSendRunResult::error();
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
        let interactive = !json && std::io::stderr().is_terminal();
        let mut progress = FileProgress::new(
            interactive,
            outcome.entry_id.clone(),
            accepted_targets.len(),
            &filename,
        );
        let waited = wait_for_file_delivery(
            service,
            &outcome.entry_id,
            &accepted_targets,
            activity.as_mut(),
            &mut progress,
        )
        .await;
        progress.finish();
        match waited {
            Ok(view) => Some(view),
            Err(WaitError::Cancelled) => {
                ui::warn("Cancelled while waiting; the daemon may continue active transfers.");
                return FileSendRunResult::error();
            }
            Err(WaitError::Request(error)) => {
                ui::error(&format!("Failed to query file delivery: {error}"));
                return FileSendRunResult::error();
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
    if json && emit_json {
        match serde_json::to_string_pretty(&result) {
            Ok(value) => println!("{value}"),
            Err(error) => {
                ui::error(&format!("Failed to serialize outcome: {error}"));
                return FileSendRunResult::error();
            }
        }
    } else if !json {
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
    let exit_code = if result.total_accepted == 0 && result.total_duplicate == 0 {
        exit_codes::EXIT_ERROR
    } else if result
        .deliveries
        .iter()
        .any(|target| matches!(target.status, EntryDeliveryStatusDto::Failed { .. }))
    {
        exit_codes::EXIT_ERROR
    } else {
        exit_codes::EXIT_SUCCESS
    };
    FileSendRunResult {
        exit_code,
        outcome: Some(result),
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
    mut activity: Option<&mut mpsc::Receiver<InboundActivityEvent>>,
    progress: &mut FileProgress,
) -> Result<EntryDeliveryViewDto, WaitError> {
    loop {
        if let Some(activity) = activity.as_deref_mut() {
            while let Ok(event) = activity.try_recv() {
                if let InboundActivityEvent::Progress(event) = event {
                    progress.observe(&event);
                }
            }
        }
        let view = service
            .entry_delivery(entry_id)
            .await
            .map_err(WaitError::Request)?;
        if all_targets_terminal(&view, accepted_targets) {
            return Ok(view);
        }
        progress.refresh();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Err(WaitError::Cancelled),
            _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {}
        }
    }
}

/// Interactive byte progress for one outbound file.
///
/// Bytes come from the receiving peers' fetch-progress reports relayed by the
/// Engine (`direction: sending`, keyed by the entry id); they measure what the
/// peers have received, not local read or network-write progress. With several
/// targets the bar sums bytes across targets and stays a spinner until every
/// accepted target has reported a known total. Terminal outcomes always come
/// from the delivery view, never from this display.
struct FileProgress {
    interactive: bool,
    entry_id: String,
    expected_targets: usize,
    label: String,
    reports: HashMap<String, (u64, Option<u64>)>,
    bar: Option<ProgressBar>,
    has_length: bool,
}

impl FileProgress {
    fn new(interactive: bool, entry_id: String, expected_targets: usize, filename: &str) -> Self {
        Self {
            interactive,
            entry_id,
            expected_targets,
            label: format!("Sending {filename}"),
            reports: HashMap::new(),
            bar: None,
            has_length: false,
        }
    }

    fn observe(&mut self, event: &FileTransferProgressEvent) {
        if event.direction != FileTransferDirection::Sending
            || event.entry_id.as_deref() != Some(self.entry_id.as_str())
        {
            return;
        }
        let entry = self
            .reports
            .entry(event.peer_id.clone())
            .or_insert((0, None));
        // Keep the display monotonic; reports are throttled and may reorder.
        entry.0 = entry.0.max(event.bytes_transferred);
        entry.1 = event.total_bytes.or(entry.1);
    }

    /// Aggregate `(received, total)` once every accepted target has a known
    /// non-zero total; `None` while any part is unknown.
    fn aggregate(&self) -> Option<(u64, u64)> {
        if self.reports.len() < self.expected_targets {
            return None;
        }
        let mut received = 0u64;
        let mut total = 0u64;
        for (bytes, target_total) in self.reports.values() {
            let target_total = (*target_total)?;
            received = received.saturating_add((*bytes).min(target_total));
            total = total.saturating_add(target_total);
        }
        (total > 0).then_some((received, total))
    }

    fn refresh(&mut self) {
        if !self.interactive {
            return;
        }
        match self.aggregate() {
            Some((received, total)) => {
                if !self.has_length {
                    if let Some(bar) = self.bar.take() {
                        bar.finish_and_clear();
                    }
                    self.bar = Some(ui::byte_progress(total, &self.label));
                    self.has_length = true;
                }
                if let Some(bar) = &self.bar {
                    bar.set_length(total);
                    bar.set_position(received);
                }
            }
            None if self.bar.is_none() => {
                self.bar = Some(ui::spinner(&format!(
                    "{}: waiting for the receiving device...",
                    self.label
                )));
            }
            None => {}
        }
    }

    fn finish(&mut self) {
        if let Some(bar) = self.bar.take() {
            bar.finish_and_clear();
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
            classify_input(Some(raw.clone()), false, false).unwrap(),
            SendInput::File(_)
        ));
        assert_eq!(
            classify_input(Some(raw.clone()), false, true).unwrap(),
            SendInput::Text(raw)
        );
        assert!(matches!(
            classify_input(Some(path.to_string_lossy().into_owned()), true, false).unwrap(),
            SendInput::File(_)
        ));
    }

    #[test]
    fn directory_and_obvious_missing_path_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let directory_error = classify_input(
            Some(directory.path().to_string_lossy().into_owned()),
            false,
            false,
        )
        .unwrap_err();
        assert!(directory_error.contains("Directory sending is not supported"));

        let missing_error = classify_input(Some("./missing.pdf".into()), false, false).unwrap_err();
        assert!(missing_error.contains("Path does not exist"));
        assert_eq!(
            classify_input(Some("hello world".into()), false, false).unwrap(),
            SendInput::Text("hello world".into())
        );
    }

    #[test]
    fn omitted_input_selects_text_or_file_stdin_mode() {
        assert_eq!(
            classify_input(None, false, false).unwrap(),
            SendInput::Stdin
        );
        assert_eq!(
            classify_input(None, true, false).unwrap(),
            SendInput::StdinFiles
        );
    }

    #[test]
    fn file_path_stdin_supports_multiple_lines_spaces_blanks_crlf_and_duplicates() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first file.txt");
        let second = directory.path().join("second.txt");
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&second, b"second").unwrap();
        let input = format!(
            "{}\r\n\n{}\r\n{}\n",
            first.display(),
            second.display(),
            first.display()
        );

        let paths = parse_file_paths(&input).unwrap();

        assert_eq!(
            paths,
            vec![
                first.canonicalize().unwrap(),
                second.canonicalize().unwrap()
            ]
        );
    }

    #[test]
    fn file_path_stdin_rejects_empty_input_invalid_paths_and_directories_atomically() {
        assert_eq!(
            parse_file_paths("\n\r\n").unwrap_err(),
            "No file paths were provided on stdin."
        );

        let directory = tempfile::tempdir().unwrap();
        let valid = directory.path().join("valid.txt");
        std::fs::write(&valid, b"valid").unwrap();
        let missing = directory.path().join("missing.txt");
        let invalid_input = format!("{}\n{}\n", valid.display(), missing.display());
        assert!(parse_file_paths(&invalid_input)
            .unwrap_err()
            .contains("Failed to inspect file"));

        assert!(
            parse_file_paths(&format!("{}\n", directory.path().display()))
                .unwrap_err()
                .contains("Directory sending is not supported")
        );
    }

    #[test]
    fn file_path_stdin_preserves_backslashes_and_non_newline_whitespace() {
        let error = parse_file_paths("C:\\Users\\Example\\file name.txt\r\n").unwrap_err();
        assert!(error.contains("C:\\Users\\Example\\file name.txt"));

        let directory = tempfile::tempdir().unwrap();
        let spaced = directory.path().join(" padded ");
        std::fs::write(&spaced, b"spaces").unwrap();
        let paths = parse_file_paths(&format!("{}\n", spaced.display())).unwrap();
        assert_eq!(paths, vec![spaced.canonicalize().unwrap()]);
    }

    #[cfg(unix)]
    #[test]
    fn file_path_stdin_rejects_unreadable_files() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("unreadable.txt");
        std::fs::write(&path, b"secret").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

        let error = parse_file_paths(&format!("{}\n", path.display())).unwrap_err();

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(error.contains("File is not readable"));
    }

    #[test]
    fn plaintext_stdin_is_never_reclassified_as_a_path() {
        assert_eq!(
            classify_input(None, false, false).unwrap(),
            SendInput::Stdin
        );
        assert_eq!(
            normalize_plaintext_stdin("./looks/like/a/path\r\n".to_string()),
            "./looks/like/a/path"
        );
    }

    #[test]
    fn multi_file_json_is_one_array() {
        let outcome = |filename: &str| SendFileOutcomeDto {
            entry_id: format!("entry-{filename}"),
            snapshot_hash: format!("hash-{filename}"),
            filename: filename.to_string(),
            size_bytes: 1,
            total_accepted: 1,
            total_duplicate: 0,
            total_offline: 0,
            total_errored: 0,
            per_target: Vec::new(),
            deliveries: Vec::new(),
        };

        let json = serialize_file_outcomes(&[outcome("one.txt"), outcome("two.txt")]).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value.as_array().unwrap().len(), 2);
        assert_eq!(value[0]["filename"], "one.txt");
        assert_eq!(value[1]["filename"], "two.txt");
    }

    #[test]
    fn multi_file_batch_collects_outcomes_and_preserves_failure_exit_code() {
        let outcome = |filename: &str| SendFileOutcomeDto {
            entry_id: format!("entry-{filename}"),
            snapshot_hash: format!("hash-{filename}"),
            filename: filename.to_string(),
            size_bytes: 1,
            total_accepted: 1,
            total_duplicate: 0,
            total_offline: 0,
            total_errored: 0,
            per_target: Vec::new(),
            deliveries: Vec::new(),
        };
        let mut batch = FileSendBatchResult::default();

        batch.push(FileSendRunResult {
            exit_code: exit_codes::EXIT_ERROR,
            outcome: Some(outcome("failed.txt")),
        });
        batch.push(FileSendRunResult {
            exit_code: exit_codes::EXIT_SUCCESS,
            outcome: Some(outcome("success.txt")),
        });

        assert_eq!(batch.exit_code, exit_codes::EXIT_ERROR);
        assert_eq!(batch.outcomes.len(), 2);
        assert_eq!(batch.outcomes[0].filename, "failed.txt");
        assert_eq!(batch.outcomes[1].filename, "success.txt");
    }

    #[test]
    fn multi_file_batch_keeps_error_without_an_outcome() {
        let mut batch = FileSendBatchResult::default();

        batch.push(FileSendRunResult::error());

        assert_eq!(batch.exit_code, exit_codes::EXIT_ERROR);
        assert!(batch.outcomes.is_empty());
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

    fn peer(id: &str, paired: bool, connected: bool) -> PeerSnapshotDto {
        PeerSnapshotDto {
            peer_id: id.to_string(),
            device_name: None,
            addresses: Vec::new(),
            is_paired: paired,
            connected,
            pairing_state: "paired".to_string(),
            channel: "unknown".to_string(),
            connection_address: None,
        }
    }

    #[test]
    fn target_readiness_requires_every_listed_peer_to_be_connected() {
        let peers = [peer("a", true, true), peer("b", true, false)];
        assert_eq!(
            target_readiness(&peers, &["a".into()]),
            TargetReadiness::Ready
        );
        assert_eq!(
            target_readiness(&peers, &["a".into(), "b".into(), "c".into()]),
            TargetReadiness::Waiting("b (offline), c (unknown device)".into())
        );
    }

    #[test]
    fn target_readiness_without_peer_flag_needs_one_connected_paired_device() {
        assert_eq!(
            target_readiness(&[], &[]),
            TargetReadiness::NothingToWaitFor
        );
        assert_eq!(
            target_readiness(&[peer("x", false, true)], &[]),
            TargetReadiness::NothingToWaitFor
        );
        assert!(matches!(
            target_readiness(&[peer("a", true, false)], &[]),
            TargetReadiness::Waiting(_)
        ));
        assert_eq!(
            target_readiness(&[peer("a", true, false), peer("b", true, true)], &[]),
            TargetReadiness::Ready
        );
    }

    fn progress_event(
        entry: &str,
        peer: &str,
        bytes: u64,
        total: Option<u64>,
    ) -> FileTransferProgressEvent {
        FileTransferProgressEvent {
            transfer_id: entry.to_string(),
            entry_id: Some(entry.to_string()),
            attempt_id: None,
            peer_id: peer.to_string(),
            direction: FileTransferDirection::Sending,
            bytes_transferred: bytes,
            total_bytes: total,
        }
    }

    #[test]
    fn file_progress_aggregates_only_when_every_target_reported_a_total() {
        let mut progress = FileProgress::new(false, "e1".into(), 2, "f.bin");
        assert_eq!(progress.aggregate(), None);
        progress.observe(&progress_event("e1", "a", 50, Some(100)));
        assert_eq!(progress.aggregate(), None, "second target has not reported");
        progress.observe(&progress_event("e1", "b", 10, None));
        assert_eq!(progress.aggregate(), None, "unknown total stays unknown");
        progress.observe(&progress_event("e1", "b", 20, Some(100)));
        assert_eq!(progress.aggregate(), Some((70, 200)));
    }

    #[test]
    fn file_progress_ignores_other_entries_and_never_goes_backwards() {
        let mut progress = FileProgress::new(false, "e1".into(), 1, "f.bin");
        progress.observe(&progress_event("other", "a", 99, Some(100)));
        assert_eq!(progress.aggregate(), None);
        progress.observe(&progress_event("e1", "a", 60, Some(100)));
        progress.observe(&progress_event("e1", "a", 30, Some(100)));
        assert_eq!(progress.aggregate(), Some((60, 100)));
        let mut receiving = progress_event("e1", "a", 100, Some(100));
        receiving.direction = FileTransferDirection::Receiving;
        progress.observe(&receiving);
        assert_eq!(progress.aggregate(), Some((60, 100)));
    }
}
