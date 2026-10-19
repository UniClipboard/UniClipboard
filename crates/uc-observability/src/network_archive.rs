//! The independent network-library log inside a diagnostic package.
//!
//! The Engine writes `<logs>/network/<role>/<run_id>/` with `run.json`, the
//! `lease.lock` that proves a live writer, and `network.<YYYYMMDD>.<n>.jsonl`
//! segments; the network root also holds `.cleanup.lock`. Only `run.json` and
//! the segments are exported, so a package can never carry a lock file or an
//! unexpected file that happens to sit in the directory. Everything is read
//! without following links.
//!
//! A package says what it holds: every copied file gets its own SHA-256, the
//! current run is compared with the digests the Engine computed after flushing,
//! and anything that is not fully accounted for is listed as a reason rather
//! than rounded up to "complete".

use std::fs::{self, File};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

/// Directory under the log root; also the archive prefix of the exported files.
pub const NETWORK_LOG_DIRECTORY: &str = "network";
const RUN_MANIFEST_FILE: &str = "run.json";
const LEASE_FILE: &str = "lease.lock";
const CLEANUP_LOCK_FILE: &str = ".cleanup.lock";
const SEGMENT_PREFIX: &str = "network.";
const SEGMENT_SUFFIX: &str = ".jsonl";
/// Process roles the Engine names run directories after.
pub const NETWORK_ROLES: [&str; 5] = [
    "daemon",
    "mobile_app",
    "keyboard_extension",
    "share_extension",
    "background_service",
];
/// A run directory written to this recently may still have a live writer.
const RECENT_RUN_WINDOW: Duration = Duration::from_secs(300);
const COPY_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FlushOutcome {
    Completed,
    Failed,
    TimedOut,
    AlreadyShutdown,
}

/// What the Engine reported when the running daemon prepared this export.
#[derive(Debug, Clone)]
pub struct NetworkLive {
    /// Whether the daemon configured the network log at all.
    pub configured: bool,
    pub role: String,
    pub run_id: String,
    pub flush: Option<FlushOutcome>,
    pub state_ready: bool,
    /// The Engine counted records it did not write.
    pub records_dropped: bool,
    pub prepared_files: Vec<PreparedFile>,
}

#[derive(Debug, Clone)]
pub struct PreparedFile {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}

/// `live` is `None` for an offline export, where no daemon flushed anything.
#[derive(Debug, Clone, Default)]
pub struct NetworkArchiveRequest {
    pub live: Option<NetworkLive>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NetworkArchiveOutcome {
    Complete,
    Partial,
    Missing,
    NotConfigured,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NetworkArchiveReason {
    EngineOffline,
    FlushFailed,
    FlushTimedOut,
    StateNotReady,
    RecordsDropped,
    FileUnreadable,
    FileTruncated,
    FileGrewAfterPrepare,
    FileChangedAfterPrepare,
    OtherProcessesNotFlushed,
    EntriesIgnored,
}

fn decimal_string<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(value)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkArchiveFile {
    pub name: String,
    /// Decimal string, like every counter the daemon reports.
    #[serde(serialize_with = "decimal_string")]
    pub bytes: u64,
    pub sha256: String,
    pub truncated: bool,
    pub matches_prepared: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkArchiveRun {
    pub role: String,
    pub run_id: String,
    pub current_run: bool,
    pub files: Vec<NetworkArchiveFile>,
    pub unreadable_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkArchiveReport {
    pub outcome: NetworkArchiveOutcome,
    pub reasons: Vec<NetworkArchiveReason>,
    pub runs: Vec<NetworkArchiveRun>,
    pub ignored_entries: u32,
}

impl NetworkArchiveReport {
    /// A report that says nothing was examined; replaced once collection runs.
    pub fn empty() -> Self {
        Self {
            outcome: NetworkArchiveOutcome::Unknown,
            reasons: Vec::new(),
            runs: Vec::new(),
            ignored_entries: 0,
        }
    }

    pub fn file_count(&self) -> usize {
        self.runs.iter().map(|run| run.files.len()).sum()
    }
}

/// One run directory selected for the package, files listed in name order.
struct CandidateRun {
    role: String,
    run_id: String,
    directory: PathBuf,
    current: bool,
    files: Vec<String>,
}

struct Listing {
    runs: Vec<CandidateRun>,
    ignored_entries: u32,
    unreadable_root: bool,
}

/// Whether `name` is a file the Engine writes into a run directory and an
/// export may carry: the run manifest or a dated segment.
fn exported_name(name: &str) -> bool {
    name == RUN_MANIFEST_FILE || segment_date(name).is_some()
}

/// UTC day of `network.<YYYYMMDD>.<sequence>.jsonl`.
pub fn segment_date(name: &str) -> Option<NaiveDate> {
    let rest = name
        .strip_prefix(SEGMENT_PREFIX)?
        .strip_suffix(SEGMENT_SUFFIX)?;
    let (date, sequence) = rest.split_once('.')?;
    if date.len() != 8
        || !date.bytes().all(|byte| byte.is_ascii_digit())
        || sequence.is_empty()
        || !sequence.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    NaiveDate::parse_from_str(date, "%Y%m%d").ok()
}

fn is_run_id(name: &str) -> bool {
    name.len() == 32
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn list_runs(logs_dir: &Path, since: Option<DateTime<Utc>>, live: Option<&NetworkLive>) -> Listing {
    let root = logs_dir.join(NETWORK_LOG_DIRECTORY);
    let mut listing = Listing {
        runs: Vec::new(),
        ignored_entries: 0,
        unreadable_root: false,
    };
    // A link or plain file where the network directory belongs is never followed.
    if fs::symlink_metadata(&root).is_ok_and(|meta| !meta.is_dir()) {
        listing.ignored_entries += 1;
        return listing;
    }
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return listing,
        Err(_) => {
            listing.unreadable_root = true;
            return listing;
        }
    };
    for entry in entries {
        let Ok(entry) = entry else {
            listing.unreadable_root = true;
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_real_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if name == CLEANUP_LOCK_FILE {
            continue;
        }
        if !is_real_dir || !NETWORK_ROLES.contains(&name.as_str()) {
            listing.ignored_entries += 1;
            continue;
        }
        collect_role(&entry.path(), &name, since, live, &mut listing);
    }
    listing
        .runs
        .sort_by(|left, right| (&left.role, &left.run_id).cmp(&(&right.role, &right.run_id)));
    listing
}

fn collect_role(
    role_dir: &Path,
    role: &str,
    since: Option<DateTime<Utc>>,
    live: Option<&NetworkLive>,
    listing: &mut Listing,
) {
    let Ok(entries) = fs::read_dir(role_dir) else {
        listing.unreadable_root = true;
        return;
    };
    for entry in entries.flatten() {
        let run_id = entry.file_name().to_string_lossy().into_owned();
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) || !is_run_id(&run_id) {
            listing.ignored_entries += 1;
            continue;
        }
        let current = live.is_some_and(|live| live.role == role && live.run_id == run_id);
        let directory = entry.path();
        let mut files = Vec::new();
        let mut newest_day = None;
        let mut newest_modified = None;
        let Ok(files_in_run) = fs::read_dir(&directory) else {
            listing.unreadable_root = true;
            continue;
        };
        for file in files_in_run.flatten() {
            let file_name = file.file_name().to_string_lossy().into_owned();
            if file_name == LEASE_FILE {
                continue;
            }
            let regular = file.file_type().is_ok_and(|kind| kind.is_file());
            if !regular || !exported_name(&file_name) {
                listing.ignored_entries += 1;
                continue;
            }
            if let Some(day) = segment_date(&file_name) {
                newest_day = newest_day.max(Some(day));
            }
            let modified = file.metadata().ok().and_then(|meta| meta.modified().ok());
            newest_modified = newest_modified.max(modified);
            files.push(file_name);
        }
        if files.is_empty() {
            continue;
        }
        if !current && !run_within_window(newest_day, newest_modified, since) {
            continue;
        }
        files.sort();
        listing.runs.push(CandidateRun {
            role: role.to_owned(),
            run_id,
            directory,
            current,
            files,
        });
    }
}

fn run_within_window(
    newest_day: Option<NaiveDate>,
    newest_modified: Option<SystemTime>,
    since: Option<DateTime<Utc>>,
) -> bool {
    let Some(since) = since else { return true };
    if newest_day.is_some_and(|day| day >= since.date_naive()) {
        return true;
    }
    newest_modified.is_some_and(|modified| modified >= SystemTime::from(since))
}

fn run_recently_written(run: &CandidateRun) -> bool {
    run.files.iter().any(|name| {
        fs::symlink_metadata(run.directory.join(name))
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age <= RECENT_RUN_WINDOW)
    })
}

struct CopyOutcome {
    copied: u64,
    sha256: String,
    prefix_sha256: Option<String>,
}

enum CopyError {
    /// Reading the source file failed; what was already written to the archive
    /// member is kept so the manifest can still account for it.
    Read(CopyOutcome),
    /// Writing into the archive failed.
    Write(io::Error),
}

/// Copies up to `length` bytes while hashing them, and also the first
/// `prefix_len` of them, so a file that grew after the Engine's digest can
/// still be checked against it.
fn copy_hashing(
    mut input: impl Read,
    output: &mut impl Write,
    length: u64,
    prefix_len: Option<u64>,
) -> Result<CopyOutcome, CopyError> {
    let mut full = Sha256::new();
    let mut prefix = Sha256::new();
    let mut prefix_digest = prefix_len
        .filter(|limit| *limit == 0)
        .map(|_| hex(&Sha256::new().finalize()));
    let mut copied: u64 = 0;
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    while copied < length {
        let want = usize::try_from((length - copied).min(COPY_BUFFER_BYTES as u64))
            .unwrap_or(COPY_BUFFER_BYTES);
        let read = match input.read(&mut buffer[..want]) {
            Ok(read) => read,
            Err(_) => {
                return Err(CopyError::Read(CopyOutcome {
                    copied,
                    sha256: hex(&full.finalize()),
                    prefix_sha256: prefix_digest,
                }));
            }
        };
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        output.write_all(chunk).map_err(CopyError::Write)?;
        full.update(chunk);
        if let (Some(limit), None) = (prefix_len, &prefix_digest) {
            let remaining = usize::try_from(limit - copied).unwrap_or(read).min(read);
            prefix.update(&chunk[..remaining]);
            if copied + read as u64 >= limit {
                prefix_digest = Some(hex(&prefix.clone().finalize()));
            }
        }
        copied += read as u64;
    }
    Ok(CopyOutcome {
        copied,
        sha256: hex(&full.finalize()),
        prefix_sha256: prefix_digest,
    })
}

/// Opens `path` without following a link, so an entry swapped for a symlink
/// after the directory listing is refused instead of exported.
fn open_regular_file(path: &Path) -> io::Result<(File, fs::Metadata)> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    Ok((file, meta))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Everything `write_archive` needs to add to the zip, produced before the
/// archive is opened so a listing failure cannot leave a half-written entry.
pub struct NetworkSelection {
    runs: Vec<CandidateRun>,
    ignored_entries: u32,
    unreadable_root: bool,
}

impl NetworkSelection {
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }
}

pub fn select(
    logs_dir: &Path,
    since: Option<DateTime<Utc>>,
    request: &NetworkArchiveRequest,
) -> NetworkSelection {
    let listing = list_runs(logs_dir, since, request.live.as_ref());
    NetworkSelection {
        runs: listing.runs,
        ignored_entries: listing.ignored_entries,
        unreadable_root: listing.unreadable_root,
    }
}

/// Adds the selected files to `archive` under `network/<role>/<run_id>/` and
/// returns the report. A failure to write into the archive is the caller's
/// error; a failure to read a source file is recorded in the report.
pub fn collect<W: Write + Seek>(
    selection: NetworkSelection,
    request: &NetworkArchiveRequest,
    archive: &mut ZipWriter<W>,
    options: SimpleFileOptions,
) -> io::Result<NetworkArchiveReport> {
    let mut reasons: Vec<NetworkArchiveReason> = Vec::new();
    let push = |reason: NetworkArchiveReason, reasons: &mut Vec<NetworkArchiveReason>| {
        if !reasons.contains(&reason) {
            reasons.push(reason);
        }
    };
    let mut runs = Vec::new();
    let mut other_run_may_be_live = false;
    for candidate in &selection.runs {
        if !candidate.current && run_recently_written(candidate) {
            other_run_may_be_live = true;
        }
        let prepared: &[PreparedFile] = match (&request.live, candidate.current) {
            (Some(live), true) => &live.prepared_files,
            _ => &[],
        };
        let mut run = NetworkArchiveRun {
            role: candidate.role.clone(),
            run_id: candidate.run_id.clone(),
            current_run: candidate.current,
            files: Vec::new(),
            unreadable_files: Vec::new(),
        };
        for name in &candidate.files {
            let path = candidate.directory.join(name);
            let Ok((file, meta)) = open_regular_file(&path) else {
                run.unreadable_files.push(name.clone());
                push(NetworkArchiveReason::FileUnreadable, &mut reasons);
                continue;
            };
            let length = meta.len();
            let expected = prepared.iter().find(|entry| &entry.name == name);
            let entry_name = format!(
                "{NETWORK_LOG_DIRECTORY}/{}/{}/{name}",
                candidate.role, candidate.run_id
            );
            archive
                .start_file(entry_name, options)
                .map_err(io::Error::other)?;
            let (outcome, read_failed) =
                match copy_hashing(file, archive, length, expected.map(|entry| entry.bytes)) {
                    Ok(outcome) => (outcome, false),
                    // A read error after the entry started cannot be undone inside
                    // the zip; the partial member stays, so it is listed with what
                    // was copied and also reported unreadable.
                    Err(CopyError::Read(outcome)) => (outcome, true),
                    Err(CopyError::Write(error)) => return Err(error),
                };
            let CopyOutcome {
                copied,
                sha256,
                prefix_sha256,
            } = outcome;
            if read_failed {
                run.unreadable_files.push(name.clone());
                push(NetworkArchiveReason::FileUnreadable, &mut reasons);
            }
            let truncated = copied != length;
            if truncated {
                push(NetworkArchiveReason::FileTruncated, &mut reasons);
            }
            let matches_prepared = expected.map(|entry| {
                if copied == entry.bytes {
                    sha256 == entry.sha256
                } else if copied > entry.bytes {
                    prefix_sha256.as_deref() == Some(entry.sha256.as_str())
                } else {
                    false
                }
            });
            match (expected, matches_prepared) {
                (Some(entry), Some(true)) if copied > entry.bytes => {
                    push(NetworkArchiveReason::FileGrewAfterPrepare, &mut reasons);
                }
                (Some(_), Some(false)) => {
                    push(NetworkArchiveReason::FileChangedAfterPrepare, &mut reasons);
                }
                (None, _) if candidate.current && segment_date(name).is_some() => {
                    push(NetworkArchiveReason::FileGrewAfterPrepare, &mut reasons);
                }
                _ => {}
            }
            run.files.push(NetworkArchiveFile {
                name: name.clone(),
                bytes: copied,
                sha256,
                truncated,
                matches_prepared,
            });
        }
        if candidate.current {
            for entry in prepared {
                if !candidate.files.contains(&entry.name) {
                    run.unreadable_files.push(entry.name.clone());
                    push(NetworkArchiveReason::FileChangedAfterPrepare, &mut reasons);
                }
            }
        }
        runs.push(run);
    }
    if selection.unreadable_root {
        push(NetworkArchiveReason::FileUnreadable, &mut reasons);
    }
    if selection.ignored_entries > 0 {
        push(NetworkArchiveReason::EntriesIgnored, &mut reasons);
    }
    if other_run_may_be_live {
        push(NetworkArchiveReason::OtherProcessesNotFlushed, &mut reasons);
    }
    let collected = runs.iter().map(|run| run.files.len()).sum::<usize>();
    let outcome = match &request.live {
        None => {
            push(NetworkArchiveReason::EngineOffline, &mut reasons);
            if collected == 0 {
                NetworkArchiveOutcome::Unknown
            } else {
                NetworkArchiveOutcome::Partial
            }
        }
        Some(live) if !live.configured => NetworkArchiveOutcome::NotConfigured,
        Some(live) => {
            match live.flush {
                Some(FlushOutcome::Completed) => {}
                Some(FlushOutcome::TimedOut) => {
                    push(NetworkArchiveReason::FlushTimedOut, &mut reasons)
                }
                Some(FlushOutcome::Failed | FlushOutcome::AlreadyShutdown) | None => {
                    push(NetworkArchiveReason::FlushFailed, &mut reasons)
                }
            }
            if !live.state_ready {
                push(NetworkArchiveReason::StateNotReady, &mut reasons);
            }
            if live.records_dropped {
                push(NetworkArchiveReason::RecordsDropped, &mut reasons);
            }
            let current_files = runs
                .iter()
                .filter(|run| run.current_run)
                .map(|run| run.files.len())
                .sum::<usize>();
            if current_files == 0 {
                NetworkArchiveOutcome::Missing
            } else if reasons.is_empty() {
                NetworkArchiveOutcome::Complete
            } else {
                NetworkArchiveOutcome::Partial
            }
        }
    };
    Ok(NetworkArchiveReport {
        outcome,
        reasons,
        runs,
        ignored_entries: selection.ignored_entries,
    })
}
