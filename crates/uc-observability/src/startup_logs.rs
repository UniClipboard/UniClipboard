//! Desktop diagnostic package assembly for live and offline export.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use serde_json::Value;
use zip::write::SimpleFileOptions;

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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticArchiveReport {
    pub included_files: Vec<String>,
    pub unreadable_files: Vec<String>,
    pub truncated_files: Vec<String>,
    pub concurrent_writes_possible: bool,
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
    if candidates.is_empty() && matches!(request.mode, DiagnosticArchiveMode::Offline) {
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

        let manifest = DiagnosticArchiveManifest {
            schema_version: 1,
            mode: request.mode,
            exported_at: Utc::now(),
            since: request.since,
            engine_preparation: request.engine_preparation,
            startup_status: request.startup_status,
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

fn managed_log_date(name: &str) -> Option<NaiveDate> {
    if let Some(date) = engine_log_file_date(name) {
        return Some(date);
    }
    [
        "uniclipboard-gui.json.",
        "uniclipboard-daemon.json.",
        "uniclipboard-cli.json.",
        "uniclipboard-quick-panel.json.",
    ]
    .iter()
    .find_map(|prefix| {
        name.strip_prefix(prefix)
            .and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
    })
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
    collection: &'a DiagnosticArchiveReport,
}
