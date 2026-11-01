//! Desktop diagnostic package assembly for live and offline export.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use serde_json::Value;
use zip::write::SimpleFileOptions;

use crate::network_archive::{self, NetworkArchiveReport, NetworkArchiveRequest};
use crate::startup_failure::{MAX_STARTUP_FAILURE_BYTES, STARTUP_FAILURE_FILE};

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticArchiveMode {
    Online,
    Offline,
}

#[derive(Debug, Clone)]
pub struct DiagnosticArchiveRequest {
    pub mode: DiagnosticArchiveMode,
    pub since: Option<DateTime<Utc>>,
    pub engine_preparation: Option<Value>,
    pub startup_status: Option<Value>,
    pub host: Option<HostManifest>,
    /// The independent network-library log. `live` is `None` when no running
    /// daemon prepared the export.
    pub network: NetworkArchiveRequest,
    pub otel: Option<OtelArchiveSnapshot>,
}

/// Frozen records supplied by the process observability owner.
#[derive(Debug, Clone)]
pub struct OtelArchiveSnapshot {
    pub metrics: String,
    pub logs: String,
    pub traces: String,
    pub summary: Value,
}

/// Facts about the exporting host that explain how the logs were produced.
///
/// Carries no device, peer or account identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostManifest {
    pub app_version: String,
    /// Commit the exporting binary was built from; `unknown` when the build
    /// carried none. Working-tree changes are not detected.
    pub build_commit: String,
    pub os: String,
    pub arch: String,
    /// Named profile; `None` for the default one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub portable: bool,
    /// Log profile the running process installed (not the persisted setting,
    /// which only applies at the next start).
    pub installed_log_profile: String,
    pub telemetry_enabled: bool,
    pub timezone_offset_minutes: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticArchiveReport {
    pub included_files: Vec<String>,
    pub unreadable_files: Vec<String>,
    pub truncated_files: Vec<String>,
    pub concurrent_writes_possible: bool,
    /// Log sources that contributed no file to the package.
    pub missing_sources: Vec<MissingSource>,
    /// The network-library log; independent of every field above.
    pub network: NetworkArchiveReport,
}

/// A log source absent from an export and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingSource {
    pub source: &'static str,
    pub reason: MissingReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingReason {
    /// The log directory holds no file of this source.
    Missing,
    /// Files exist but none could be exported.
    Unreadable,
}

/// Where an export failed, so callers can tell the user what to look at
/// without parsing error text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFailureCause {
    /// No managed log lies inside the requested window.
    NoLogs,
    /// The log directory could not be listed.
    SourceRead,
    /// The archive could not be built or saved.
    ArchiveWrite,
}

/// Failure of [`export_diagnostic_logs`], tagged with its [`ExportFailureCause`].
#[derive(Debug)]
pub struct ExportError {
    cause: ExportFailureCause,
    source: anyhow::Error,
}

impl ExportError {
    fn new(cause: ExportFailureCause, source: impl Into<anyhow::Error>) -> Self {
        Self {
            cause,
            source: source.into(),
        }
    }

    fn archive_write(source: impl Into<anyhow::Error>) -> Self {
        Self::new(ExportFailureCause::ArchiveWrite, source)
    }

    pub fn cause(&self) -> ExportFailureCause {
        self.cause
    }
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:#}", self.source)
    }
}

impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

const NO_LOGS_MESSAGE: &str = "No application logs are available to export";

/// Package retained logs while the daemon is unavailable.
pub fn export_startup_logs(logs_dir: &Path, destination: &Path) -> Result<(), ExportError> {
    export_startup_logs_with_status(logs_dir, destination, None)
}

/// Package retained logs together with the terminal startup snapshot, when the
/// startup service is still available.
pub fn export_startup_logs_with_status(
    logs_dir: &Path,
    destination: &Path,
    startup_status: Option<Value>,
) -> Result<(), ExportError> {
    export_diagnostic_logs(
        logs_dir,
        destination,
        DiagnosticArchiveRequest {
            mode: DiagnosticArchiveMode::Offline,
            since: None,
            engine_preparation: None,
            startup_status,
            host: None,
            network: NetworkArchiveRequest::default(),
            otel: None,
        },
    )?;
    Ok(())
}

/// Package a bounded snapshot of managed desktop and Engine logs.
pub fn export_diagnostic_logs(
    logs_dir: &Path,
    destination: &Path,
    request: DiagnosticArchiveRequest,
) -> Result<DiagnosticArchiveReport, ExportError> {
    let (mut candidates, unreadable_files) = collect_log_files(logs_dir)
        .map_err(|error| ExportError::new(ExportFailureCause::SourceRead, error))?;
    candidates.sort_by(|left, right| left.name.cmp(&right.name));
    let network_selection = network_archive::select(logs_dir, request.since, &request.network);
    if candidates.is_empty()
        && network_selection.is_empty()
        && !has_offline_history(logs_dir)
        && matches!(request.mode, DiagnosticArchiveMode::Offline)
    {
        return Err(ExportError::new(
            ExportFailureCause::NoLogs,
            anyhow::anyhow!(NO_LOGS_MESSAGE),
        ));
    }

    let parent = destination
        .parent()
        .context("missing destination directory")
        .map_err(ExportError::archive_write)?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(ExportError::archive_write)?;
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o600);
    let mut report = DiagnosticArchiveReport {
        included_files: Vec::new(),
        unreadable_files,
        truncated_files: Vec::new(),
        concurrent_writes_possible: true,
        missing_sources: Vec::new(),
        network: NetworkArchiveReport::empty(),
    };

    // Files that belong to the requested window, whether or not they could be
    // read. An offline export with none of them has nothing to report.
    let mut in_window = 0;
    {
        let mut archive = zip::ZipWriter::new(temporary.as_file_mut());
        for candidate in candidates {
            // Size and modification time come from the opened handle. On
            // Windows the directory entry of a file that another process keeps
            // open for appending is only refreshed when that writer closes it,
            // so its size can be far behind or even zero.
            let Ok((input, metadata)) = File::open(&candidate.path)
                .and_then(|input| input.metadata().map(|metadata| (input, metadata)))
            else {
                // Without a handle, fall back to the listed metadata so an
                // unreadable file outside the window stays out of the report.
                match &candidate.listed {
                    Some(listed)
                        if !within_window(
                            candidate.date,
                            listed.modified().ok(),
                            request.since,
                        ) => {}
                    Some(_) => {
                        in_window += 1;
                        report.unreadable_files.push(candidate.name);
                    }
                    None => report.unreadable_files.push(candidate.name),
                }
                continue;
            };
            if !within_window(candidate.date, metadata.modified().ok(), request.since) {
                continue;
            }
            in_window += 1;
            // Copy the length observed at open time rather than reading to
            // EOF, so an export converges while writers keep appending; later
            // records are covered by `concurrent_writes_possible`.
            let length = if candidate.date.is_none() {
                metadata.len().min(MAX_STARTUP_FAILURE_BYTES)
            } else {
                metadata.len()
            };
            archive
                .start_file(format!("logs/{}", candidate.name), options)
                .map_err(ExportError::archive_write)?;
            let copied = io::copy(&mut input.take(length), &mut archive)
                .map_err(ExportError::archive_write)?;
            if copied != length {
                report.truncated_files.push(candidate.name.clone());
            }
            report.included_files.push(candidate.name);
        }

        report.missing_sources = missing_sources(&report);
        report.network =
            network_archive::collect(network_selection, &request.network, &mut archive, options)
                .map_err(ExportError::archive_write)?;
        in_window += report.network.file_count();
        let otel_has_files = write_otel(&mut archive, options, request.otel.as_ref(), logs_dir)
            .map_err(ExportError::archive_write)?;
        in_window += usize::from(otel_has_files);
        let manifest = DiagnosticArchiveManifest {
            schema_version: 1,
            mode: request.mode,
            exported_at: Utc::now(),
            since: request.since,
            engine_preparation: request.engine_preparation,
            startup_status: request.startup_status,
            host: request.host,
            collection: &report,
        };
        archive
            .start_file("manifest.json", options)
            .map_err(ExportError::archive_write)?;
        serde_json::to_writer_pretty(&mut archive, &manifest)
            .map_err(ExportError::archive_write)?;
        archive.finish().map_err(ExportError::archive_write)?;
    }

    // The window is checked against the opened handle, so an offline export
    // with no file inside it is only known here. Dropping the temporary
    // archive leaves any existing destination untouched.
    if in_window == 0 && matches!(request.mode, DiagnosticArchiveMode::Offline) {
        return Err(ExportError::new(
            ExportFailureCause::NoLogs,
            anyhow::anyhow!(NO_LOGS_MESSAGE),
        ));
    }

    temporary
        .as_file()
        .sync_all()
        .map_err(ExportError::archive_write)?;
    temporary
        .persist(destination)
        .context("save log archive")
        .map_err(ExportError::archive_write)?;
    Ok(report)
}

struct LogCandidate {
    path: PathBuf,
    name: String,
    /// Calendar day of a dated log; `None` for the undated startup failure
    /// record, which is always part of the window.
    date: Option<NaiveDate>,
    /// Directory-entry metadata; only used when the file cannot be opened.
    listed: Option<fs::Metadata>,
}

fn collect_log_files(logs_dir: &Path) -> Result<(Vec<LogCandidate>, Vec<String>)> {
    if !logs_dir.exists() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut files = Vec::new();
    let mut unreadable = Vec::new();
    for entry in fs::read_dir(logs_dir).context("read log directory")? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str().map(str::to_owned) else {
            continue;
        };
        let date = if name == STARTUP_FAILURE_FILE {
            None
        } else if let Some(date) = managed_log_date(&name) {
            Some(date)
        } else {
            continue;
        };
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => {
                unreadable.push(name);
                continue;
            }
        };
        if !file_type.is_file() {
            // The startup failure record is the one file whose absence from an
            // export must never be silent: report it instead of skipping it.
            if date.is_none() {
                unreadable.push(name);
            }
            continue;
        }
        files.push(LogCandidate {
            path: entry.path(),
            name,
            date,
            listed: entry.metadata().ok(),
        });
    }
    Ok((files, unreadable))
}

/// Managed daily logs: source name, the file name around the date
/// (`<prefix><YYYY-MM-DD><suffix>`) and whether a current Desktop process
/// writes the source: an absent file of such a source is reported as missing.
/// The Go GUI host and CLI write `gui` and `cli` through `hostlog`. The Go
/// offline export keeps the same list; `tests/log_source_patterns.rs` and its Go counterpart check both
/// against one fixture.
const LOG_SOURCES: [(&str, &str, &str, bool); 5] = [
    ("gui", "uniclipboard-gui.json.", "", true),
    ("daemon", "uniclipboard-daemon.json.", "", true),
    ("cli", "uniclipboard-cli.json.", "", true),
    ("quick-panel", "uniclipboard-quick-panel.json.", "", true),
    ("engine", "engine.", ".jsonl", true),
];

/// Whether a current Desktop process writes `source`; `None` for an unknown name.
pub fn source_written_by_current_host(source: &str) -> Option<bool> {
    LOG_SOURCES
        .iter()
        .find(|(name, ..)| *name == source)
        .map(|&(.., current)| current)
}

/// Source and calendar day of a managed daily log, `None` for any other name.
fn managed_log(name: &str) -> Option<(&'static str, NaiveDate)> {
    LOG_SOURCES.iter().find_map(|&(source, prefix, suffix, _)| {
        let date = name.strip_prefix(prefix)?.strip_suffix(suffix)?;
        // Zero-padded, as every writer produces it. Engine names additionally
        // go through the Engine-compatible parser below.
        let parsed = if source == "engine" {
            engine_log_file_date(name)?
        } else if date.len() == 10 {
            NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?
        } else {
            return None;
        };
        Some((source, parsed))
    })
}

/// Sources of [`LOG_SOURCES`] without an exported file, in table order.
fn missing_sources(report: &DiagnosticArchiveReport) -> Vec<MissingSource> {
    let contributes = |names: &[String], source: &str| {
        names
            .iter()
            .any(|name| managed_log_source(name) == Some(source))
    };
    LOG_SOURCES
        .iter()
        .filter_map(|&(source, _, _, current)| {
            if !current || contributes(&report.included_files, source) {
                None
            } else if contributes(&report.unreadable_files, source) {
                Some(MissingSource {
                    source,
                    reason: MissingReason::Unreadable,
                })
            } else {
                Some(MissingSource {
                    source,
                    reason: MissingReason::Missing,
                })
            }
        })
        .collect()
}

/// Source name (`gui`, `daemon`, `cli`, `quick-panel`, `engine`) of a managed
/// daily log file name.
pub fn managed_log_source(name: &str) -> Option<&'static str> {
    managed_log(name).map(|(source, _)| source)
}

fn managed_log_date(name: &str) -> Option<NaiveDate> {
    managed_log(name).map(|(_, date)| date)
}

/// Date of an Engine daily log (`engine.YYYY-MM-DD.jsonl`). Mirrors Engine
/// `managed_log_file_date` so the GUI need not link the Engine; the daemon's
/// `engine_log_file_name_contract` test keeps the two equal.
pub fn engine_log_file_date(name: &str) -> Option<NaiveDate> {
    let date = name.strip_prefix("engine.")?.strip_suffix(".jsonl")?;
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()
}

fn within_window(
    date: Option<NaiveDate>,
    modified: Option<SystemTime>,
    since: Option<DateTime<Utc>>,
) -> bool {
    let Some(since) = since else { return true };
    let Some(date) = date else { return true };
    if date >= since.date_naive() {
        return true;
    }
    modified.is_some_and(|modified| modified >= SystemTime::from(since))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticArchiveManifest<'a> {
    schema_version: u8,
    mode: DiagnosticArchiveMode,
    exported_at: DateTime<Utc>,
    since: Option<DateTime<Utc>>,
    engine_preparation: Option<Value>,
    startup_status: Option<Value>,
    host: Option<HostManifest>,
    collection: &'a DiagnosticArchiveReport,
}

fn write_otel<W: Write + std::io::Seek>(
    archive: &mut zip::ZipWriter<W>,
    options: SimpleFileOptions,
    snapshot: Option<&OtelArchiveSnapshot>,
    logs_dir: &Path,
) -> Result<bool> {
    use sha2::{Digest, Sha256};
    let mut files = Vec::new();
    if let Some(snapshot) = snapshot {
        for (signal, content) in [
            ("metrics", &snapshot.metrics),
            ("logs", &snapshot.logs),
            ("traces", &snapshot.traces),
        ] {
            let name = format!("otel/{signal}.jsonl");
            archive.start_file(&name, options)?;
            archive.write_all(content.as_bytes())?;
            files.push(serde_json::json!({"name": name, "bytes": content.len(), "sha256": format!("{:x}", Sha256::digest(content.as_bytes())), "requests": content.lines().count(), "state": if content.is_empty() { "missing" } else { "partial" }}));
        }
    }
    let mut offline_state = "unavailable";
    let mut offline_reason = "engine_offline_key_unavailable";
    if snapshot.is_none() && has_offline_history(logs_dir) {
        let content = File::open(logs_dir.join("otel-history/history.enc")).and_then(|input| {
            let mut content = Vec::new();
            input.take(8 * 1024 * 1024 + 1).read_to_end(&mut content)?;
            if content.len() > 8 * 1024 * 1024 {
                return Err(io::Error::other("history exceeds quota"));
            }
            Ok(content)
        });
        match content {
            Ok(content) => {
                archive.start_file("otel/history.enc", options)?;
                archive.write_all(&content)?;
                files.push(serde_json::json!({"name": "otel/history.enc", "bytes": content.len(), "sha256": format!("{:x}", Sha256::digest(&content)), "state": "encrypted_unavailable"}));
                offline_state = "encrypted_unavailable";
            }
            Err(_) => {
                offline_state = "partial";
                offline_reason = "history_unreadable";
            }
        }
    }
    let summary = serde_json::json!({"schemaVersion": 1, "format": "OTLP JSONL", "collectorVersion": "0.139.0", "liveFlush": snapshot.is_some(), "history": snapshot.map(|s| &s.summary), "state": if snapshot.is_some() { "partial" } else { offline_state }, "reason": if snapshot.is_some() { "registered_processes_only" } else { offline_reason }, "files": files});
    archive.start_file("otel/manifest.json", options)?;
    serde_json::to_writer_pretty(&mut *archive, &summary)?;
    for (name, content) in [
        (
            "import/import.py",
            include_str!("../../../packages/desktop-host-go/diaglogs/import/import.py"),
        ),
        (
            "import/collector.yaml",
            include_str!("../../../packages/desktop-host-go/diaglogs/import/collector.yaml"),
        ),
        (
            "import/README.md",
            include_str!("../../../packages/desktop-host-go/diaglogs/import/README.md"),
        ),
    ] {
        archive.start_file(name, options)?;
        archive.write_all(content.as_bytes())?;
    }
    Ok(!files.is_empty())
}

fn has_offline_history(logs_dir: &Path) -> bool {
    fs::symlink_metadata(logs_dir.join("otel-history/history.enc"))
        .is_ok_and(|info| info.is_file() && info.len() > 0 && info.len() <= 8 * 1024 * 1024)
}
