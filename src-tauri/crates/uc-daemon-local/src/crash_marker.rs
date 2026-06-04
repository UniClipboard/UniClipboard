//! Reverse crash-detection marker (ADR-008 D17 / P4-5).
//!
//! The daemon can die without running any cleanup — SIGKILL, OOM kill, power
//! loss, or `panic = "abort"` (the release profile). None of those reach a
//! "write a crash record on exit" path, so we invert the logic:
//!
//! - **start marker** (`daemon-run.json`): written at boot, **cleared on the
//!   graceful-shutdown path only** (never from `Drop` — a torn-down process must
//!   leave it behind). A start marker that survives into the next boot therefore
//!   *is* the "previous run exited abnormally" signal.
//! - **last-exit report** (`daemon-last-exit.json`): a machine-readable
//!   [`DaemonExitReport`] the GUI reads on startup to show a red banner. Written
//!   when an abnormal exit is detected, or when the daemon refuses to start
//!   (ADR-008 D9 unlock-contract violation, P4-2).
//!
//! Both files live in `app_data_root` (per-profile), reusing the atomic
//! temp+rename write of `lightweight.rs` so a torn write never corrupts them.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const START_MARKER_FILE: &str = "daemon-run.json";
const LAST_EXIT_FILE: &str = "daemon-last-exit.json";

/// Why the previous daemon run ended in a way worth surfacing to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonExitKind {
    /// A previous run left its start marker behind — it never reached the
    /// graceful-shutdown path (SIGKILL / OOM / power loss / `panic = abort`).
    UnexpectedExit,
    /// The daemon refused to start (e.g. ADR-008 D9 unlock-contract violation).
    StartupFailure,
}

/// In-flight start marker: present while a run is alive, cleared on graceful
/// shutdown. Its survival across a restart is the abnormal-exit signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartMarker {
    pid: u32,
    started_at_ms: u64,
}

/// Machine-readable record of the most recent abnormal daemon exit, read by the
/// GUI on startup to show a red banner (ADR-008 D17, P4-5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonExitReport {
    pub kind: DaemonExitKind,
    /// PID of the run that died (`UnexpectedExit`) or the process that refused
    /// to start (`StartupFailure`).
    pub pid: u32,
    /// When the dead run started; `0` when unknown (startup failure / legacy).
    pub started_at_ms: u64,
    /// When this report was recorded.
    pub detected_at_ms: u64,
    /// Optional human-readable cause (e.g. the unlock-contract violation text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Reverse crash-detection marker rooted at a per-profile `app_data_root`.
#[derive(Debug, Clone)]
pub struct DaemonRunMarker {
    app_data_root: PathBuf,
}

impl DaemonRunMarker {
    pub fn new(app_data_root: PathBuf) -> Self {
        Self { app_data_root }
    }

    fn start_path(&self) -> PathBuf {
        self.app_data_root.join(START_MARKER_FILE)
    }

    fn last_exit_path(&self) -> PathBuf {
        self.app_data_root.join(LAST_EXIT_FILE)
    }

    /// Begin a daemon run: detect whether the *previous* run exited abnormally
    /// (its start marker survived), record it as the last-exit report, then write
    /// THIS run's start marker. Returns the detected previous abnormal exit, if
    /// any. Crash visibility is best-effort, so callers should log — not abort —
    /// on `Err`.
    pub fn begin_run(&self, pid: u32) -> Result<Option<DaemonExitReport>> {
        let detected = match self.read_start_marker() {
            Some(prev) => {
                let report = DaemonExitReport {
                    kind: DaemonExitKind::UnexpectedExit,
                    pid: prev.pid,
                    started_at_ms: prev.started_at_ms,
                    detected_at_ms: now_ms(),
                    detail: None,
                };
                self.write_json_atomic(&self.last_exit_path(), &report)?;
                Some(report)
            }
            None => None,
        };

        let marker = StartMarker {
            pid,
            started_at_ms: now_ms(),
        };
        self.write_json_atomic(&self.start_path(), &marker)?;
        Ok(detected)
    }

    /// Mark this run as having exited cleanly: remove the start marker so the
    /// next boot does NOT report an abnormal exit. Call from the graceful
    /// shutdown path only — never `Drop`, since a killed process must leave the
    /// marker behind to be detected.
    pub fn mark_clean_exit(&self) -> Result<()> {
        remove_if_present(&self.start_path())
    }

    /// Record a startup failure (the daemon refused to start) for the GUI banner.
    pub fn record_startup_failure(&self, detail: impl Into<String>) -> Result<()> {
        let report = DaemonExitReport {
            kind: DaemonExitKind::StartupFailure,
            pid: std::process::id(),
            started_at_ms: 0,
            detected_at_ms: now_ms(),
            detail: Some(detail.into()),
        };
        self.write_json_atomic(&self.last_exit_path(), &report)
    }

    /// Read the most recent abnormal-exit report (GUI startup). `None` when no
    /// report is pending or the file is corrupt (best-effort — never blocks).
    pub fn read_last_exit(&self) -> Option<DaemonExitReport> {
        read_json(&self.last_exit_path())
    }

    /// Clear the last-exit report once the GUI has surfaced it to the user.
    pub fn clear_last_exit(&self) -> Result<()> {
        remove_if_present(&self.last_exit_path())
    }

    /// Read the in-flight start marker. A corrupt/torn marker is treated as
    /// absent (best-effort): we'd rather miss one crash report than wedge boot.
    fn read_start_marker(&self) -> Option<StartMarker> {
        read_json(&self.start_path())
    }

    fn write_json_atomic<T: Serialize>(&self, path: &Path, value: &T) -> Result<()> {
        fs::create_dir_all(&self.app_data_root).with_context(|| {
            format!(
                "failed to create app data root {}",
                self.app_data_root.display()
            )
        })?;
        let payload = serde_json::to_vec(value)
            .with_context(|| format!("failed to serialize {}", path.display()))?;
        // Append `.tmp` (don't replace the extension) so `daemon-run.json`
        // stages as `daemon-run.json.tmp`, matching lightweight.rs.
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        fs::write(&tmp, &payload).with_context(|| format!("failed to write {}", tmp.display()))?;
        fs::rename(&tmp, path)
            .with_context(|| format!("failed to rename {} -> {}", tmp.display(), path.display()))?;
        Ok(())
    }
}

/// Remove `path` if present; missing is success.
fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(anyhow::Error::new(error).context(format!("failed to remove {}", path.display())))
        }
    }
}

/// Best-effort JSON read: `None` on missing, empty, or corrupt file. A corrupt
/// crash marker must never block daemon boot or GUI startup.
fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = fs::read(path).ok()?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return None;
    }
    match serde_json::from_slice(&bytes) {
        Ok(value) => Some(value),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "ignoring corrupt daemon marker file");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn marker_in(temp: &TempDir) -> DaemonRunMarker {
        DaemonRunMarker::new(temp.path().to_path_buf())
    }

    #[test]
    fn first_run_detects_nothing_and_writes_start_marker() {
        let temp = TempDir::new().unwrap();
        let marker = marker_in(&temp);

        assert_eq!(marker.begin_run(100).unwrap(), None);
        assert!(temp.path().join(START_MARKER_FILE).exists());
        assert!(marker.read_last_exit().is_none());
    }

    #[test]
    fn surviving_start_marker_reports_unexpected_exit() {
        let temp = TempDir::new().unwrap();
        let marker = marker_in(&temp);

        // Run 1 starts but is "killed" (no mark_clean_exit).
        assert_eq!(marker.begin_run(111).unwrap(), None);

        // Run 2 boots: the surviving marker means run 1 died abnormally.
        let detected = marker.begin_run(222).unwrap().expect("crash detected");
        assert_eq!(detected.kind, DaemonExitKind::UnexpectedExit);
        assert_eq!(
            detected.pid, 111,
            "reports the DEAD run's pid, not the new one"
        );

        let report = marker.read_last_exit().expect("last-exit persisted");
        assert_eq!(report, detected);
    }

    #[test]
    fn clean_exit_clears_marker_so_next_boot_is_silent() {
        let temp = TempDir::new().unwrap();
        let marker = marker_in(&temp);

        marker.begin_run(111).unwrap();
        marker.mark_clean_exit().unwrap();
        assert!(!temp.path().join(START_MARKER_FILE).exists());

        // Next boot sees no surviving marker → no abnormal exit.
        assert_eq!(marker.begin_run(222).unwrap(), None);
    }

    #[test]
    fn startup_failure_is_recorded_for_the_banner() {
        let temp = TempDir::new().unwrap();
        let marker = marker_in(&temp);

        marker
            .record_startup_failure("unattended + auto_unlock disabled")
            .unwrap();

        let report = marker.read_last_exit().expect("startup failure persisted");
        assert_eq!(report.kind, DaemonExitKind::StartupFailure);
        assert_eq!(report.pid, std::process::id());
        assert_eq!(
            report.detail.as_deref(),
            Some("unattended + auto_unlock disabled")
        );
    }

    #[test]
    fn clear_last_exit_is_idempotent() {
        let temp = TempDir::new().unwrap();
        let marker = marker_in(&temp);

        marker.record_startup_failure("boom").unwrap();
        marker.clear_last_exit().unwrap();
        assert!(marker.read_last_exit().is_none());
        // Clearing again on a missing file is fine.
        marker.clear_last_exit().unwrap();
    }

    #[test]
    fn corrupt_marker_is_treated_as_absent() {
        let temp = TempDir::new().unwrap();
        let marker = marker_in(&temp);

        fs::write(temp.path().join(START_MARKER_FILE), b"{not json").unwrap();
        // begin_run must not error on a torn start marker; it just won't report.
        assert_eq!(marker.begin_run(222).unwrap(), None);

        fs::write(temp.path().join(LAST_EXIT_FILE), b"garbage").unwrap();
        assert!(marker.read_last_exit().is_none());
    }

    #[test]
    fn atomic_write_leaves_no_temp_behind() {
        let temp = TempDir::new().unwrap();
        let marker = marker_in(&temp);

        marker.begin_run(111).unwrap();
        assert!(!temp
            .path()
            .join(format!("{START_MARKER_FILE}.tmp"))
            .exists());
    }
}
