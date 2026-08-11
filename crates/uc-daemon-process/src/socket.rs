//! Shared daemon connection-file (`daemon.conn`) resolution (ADR-011).
//!
//! The daemon binds an ephemeral loopback port and publishes the actual
//! address + bearer token + process identity in `<app_data_root>/daemon.conn`;
//! every local client discovers the daemon through this file. The legacy
//! `UC_PROFILE`-hash fixed-port resolution was retired with the single-track
//! switch (ADR-011 P2).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use uc_app_paths::app_data_root;

/// Loopback host the daemon listens on and advertises in `daemon.conn`.
pub const DEFAULT_HTTP_HOST: &str = "127.0.0.1";

// ── daemon.conn connection file (ADR-011) ───────────────────────────────────

/// Format version of the `daemon.conn` connection-info file.
///
/// Readers must reject an unknown format rather than guess: a shape change is
/// a client/daemon contract change gated by `DAEMON_API_REVISION` (ADR-011
/// §4.5). Clients that cannot read the file treat the daemon as
/// absent/incompatible and let the existing version handshake decide.
pub const DAEMON_CONN_FORMAT: u32 = 1;

/// Leaf filename of the daemon connection-info file
/// (`<app_data_root>/daemon.conn`).
const DAEMON_CONN_FILE_NAME: &str = "daemon.conn";

/// Connection-info payload the daemon publishes for local clients (ADR-011).
///
/// The single source of truth for the loopback HTTP address, the bearer token
/// and the owning process identity. Written atomically (temp + rename, `0o600`)
/// AFTER the loopback listener is bound, so a client that reads the file can
/// connect immediately without racing the bind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonConnFile {
    pub format: u32,
    pub host: String,
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub started_at_ms: u64,
}

impl DaemonConnFile {
    /// Build a connection record for the daemon process itself.
    ///
    /// `started_at_ms` is the wall-clock timestamp at publication time; it is
    /// diagnostic only — staleness is detected via PID identity
    /// (`verify_pid_identity`), never by age.
    pub fn new(host: &str, port: u16, token: &str, pid: u32) -> Self {
        let started_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or(0);
        Self {
            format: DAEMON_CONN_FORMAT,
            host: host.to_string(),
            port,
            token: token.to_string(),
            pid,
            started_at_ms,
        }
    }
}

/// Resolve the daemon connection-info file path.
///
/// Returns the path at which the daemon publishes its connection info as a
/// `PathBuf`. Reproduces `AppPaths` (`<app_data_root>/daemon.conn`) via the
/// directory-layout authority without touching the app stack.
///
/// # Examples
///
/// ```
/// use uc_daemon_process::socket::resolve_daemon_conn_path;
///
/// let path = resolve_daemon_conn_path().unwrap();
/// assert_eq!(path.file_name().unwrap(), "daemon.conn");
/// ```
pub fn resolve_daemon_conn_path() -> Result<PathBuf> {
    let root = app_data_root().context("the system data-local directory is unavailable")?;
    Ok(root.join(DAEMON_CONN_FILE_NAME))
}

/// Read the daemon connection-info file, returning `Ok(None)` when it does not
/// exist yet (daemon not running).
///
/// A malformed payload or an unsupported `format` is an error — callers decide
/// whether to surface it or treat the daemon as absent (ADR-011 §4.5).
///
/// # Examples
///
/// ```
/// use uc_daemon_process::socket::{DaemonConnFile, read_daemon_conn_file_at, write_daemon_conn_file_at};
///
/// let tmp = tempfile::tempdir().unwrap();
/// let path = tmp.path().join("daemon.conn");
/// assert!(read_daemon_conn_file_at(&path).unwrap().is_none());
/// write_daemon_conn_file_at(&path, &DaemonConnFile::new("127.0.0.1", 43127, "tok", 42)).unwrap();
/// let conn = read_daemon_conn_file_at(&path).unwrap().unwrap();
/// assert_eq!(conn.port, 43127);
/// ```
pub fn read_daemon_conn_file() -> Result<Option<DaemonConnFile>> {
    read_daemon_conn_file_at(&resolve_daemon_conn_path()?)
}

/// Read the daemon connection-info file at an explicit path.
pub fn read_daemon_conn_file_at(path: &Path) -> Result<Option<DaemonConnFile>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path).with_context(|| {
        format!(
            "failed to read daemon connection file at {}",
            path.display()
        )
    })?;
    let conn: DaemonConnFile = serde_json::from_str(&content).with_context(|| {
        format!(
            "failed to parse daemon connection file at {}",
            path.display()
        )
    })?;
    if conn.format != DAEMON_CONN_FORMAT {
        anyhow::bail!(
            "unsupported daemon connection file format {} at {} (expected {})",
            conn.format,
            path.display(),
            DAEMON_CONN_FORMAT
        );
    }
    Ok(Some(conn))
}

/// Atomically publish the daemon connection-info file (temp + rename, `0o600`).
///
/// The temp file is created in the same directory as the target so the rename
/// is a same-filesystem atomic publish; readers either see the old file or the
/// new file, never a partial write.
///
/// # Examples
///
/// ```
/// use uc_daemon_process::socket::{DaemonConnFile, write_daemon_conn_file_at};
///
/// let tmp = tempfile::tempdir().unwrap();
/// let path = tmp.path().join("daemon.conn");
/// write_daemon_conn_file_at(&path, &DaemonConnFile::new("127.0.0.1", 43127, "tok", 42)).unwrap();
/// assert!(path.exists());
/// ```
pub fn write_daemon_conn_file(conn: &DaemonConnFile) -> Result<()> {
    write_daemon_conn_file_at(&resolve_daemon_conn_path()?, conn)
}

/// Remove the daemon connection-info file.
///
/// Called on graceful daemon shutdown so a clean exit leaves no stale file
/// behind. Crash leftovers are still handled: clients verify the recorded PID
/// identity and treat a dead PID as absent (ADR-011 §4.2).
///
/// # Examples
///
/// ```
/// use uc_daemon_process::socket::{DaemonConnFile, remove_daemon_conn_file_at, write_daemon_conn_file_at};
///
/// let tmp = tempfile::tempdir().unwrap();
/// let path = tmp.path().join("daemon.conn");
/// write_daemon_conn_file_at(&path, &DaemonConnFile::new("127.0.0.1", 1, "t", 1)).unwrap();
/// remove_daemon_conn_file_at(&path).unwrap();
/// assert!(!path.exists());
/// ```
pub fn remove_daemon_conn_file() -> Result<()> {
    remove_daemon_conn_file_at(&resolve_daemon_conn_path()?)
}

/// Remove the daemon connection-info file at an explicit path.
pub fn remove_daemon_conn_file_at(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("failed to remove daemon connection file {}", path.display())),
    }
}

/// Atomically publish the daemon connection-info file at an explicit path.
pub fn write_daemon_conn_file_at(path: &Path, conn: &DaemonConnFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create daemon connection file directory {}",
                parent.display()
            )
        })?;
    }

    let content =
        serde_json::to_string_pretty(conn).context("failed to serialize daemon connection file")?;

    // Publish through a uniquely-named temp file in the same directory, then
    // rename over the target. `create` + `truncate` on the temp file is safe:
    // the name embeds our PID, so two daemons racing here cannot share a temp.
    let tmp_path = path.with_extension(format!("conn.tmp-{}", std::process::id()));

    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);

    #[cfg(unix)]
    options.mode(0o600);

    let mut tmp = options.open(&tmp_path).with_context(|| {
        format!(
            "failed to open temporary daemon connection file {}",
            tmp_path.display()
        )
    })?;
    tmp.write_all(content.as_bytes()).with_context(|| {
        format!(
            "failed to write temporary daemon connection file {}",
            tmp_path.display()
        )
    })?;
    tmp.flush().with_context(|| {
        format!(
            "failed to flush temporary daemon connection file {}",
            tmp_path.display()
        )
    })?;
    drop(tmp);

    // `rename` cannot replace an existing file on Windows; a short non-atomic
    // window here is acceptable (readers of a missing file treat the daemon as
    // absent and retry — the connection handshake is retry-tolerant).
    #[cfg(windows)]
    let _ = std::fs::remove_file(path);

    std::fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "failed to atomically publish daemon connection file {}",
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod daemon_conn_file_tests {
    use super::*;

    #[test]
    fn conn_file_write_read_roundtrip_preserves_all_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.conn");
        let conn = DaemonConnFile::new("127.0.0.1", 43127, "deadbeef", 4242);
        write_daemon_conn_file_at(&path, &conn).unwrap();

        let loaded = read_daemon_conn_file_at(&path).unwrap().unwrap();
        assert_eq!(loaded, conn);
        assert_eq!(loaded.format, DAEMON_CONN_FORMAT);
        assert_eq!(loaded.host, "127.0.0.1");
        assert_eq!(loaded.port, 43127);
        assert_eq!(loaded.token, "deadbeef");
        assert_eq!(loaded.pid, 4242);
    }

    #[test]
    fn conn_file_serializes_with_camel_case_wire_names() {
        // ADR-011: the on-disk JSON uses the camelCase field names the contract
        // types use across the wire; a snake_case regression would silently
        // break every reader.
        let conn = DaemonConnFile::new("127.0.0.1", 43127, "tok", 7);
        let json = serde_json::to_string(&conn).unwrap();
        assert!(json.contains("\"startedAtMs\""), "got {json}");
        assert!(!json.contains("started_at_ms"), "got {json}");
        assert!(json.contains("\"format\":1"), "got {json}");
    }

    #[test]
    fn conn_file_missing_reads_as_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.conn");
        assert_eq!(read_daemon_conn_file_at(&path).unwrap(), None);
    }

    #[test]
    fn conn_file_corrupt_payload_is_an_error_not_silent_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.conn");
        std::fs::write(&path, "not-json").unwrap();
        let err = read_daemon_conn_file_at(&path).unwrap_err();
        assert!(err.to_string().contains("parse"), "got {err}");
    }

    #[test]
    fn conn_file_unknown_format_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.conn");
        std::fs::write(
            &path,
            r#"{"format":99,"host":"127.0.0.1","port":1,"token":"t","pid":1,"startedAtMs":1}"#,
        )
        .unwrap();
        let err = read_daemon_conn_file_at(&path).unwrap_err();
        assert!(err.to_string().contains("format"), "got {err}");
    }

    #[test]
    fn conn_file_write_creates_missing_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested").join("deeper").join("daemon.conn");
        write_daemon_conn_file_at(&path, &DaemonConnFile::new("127.0.0.1", 1, "t", 1)).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn conn_file_overwrite_replaces_previous_payload() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.conn");
        write_daemon_conn_file_at(&path, &DaemonConnFile::new("127.0.0.1", 1000, "old", 1))
            .unwrap();
        write_daemon_conn_file_at(&path, &DaemonConnFile::new("127.0.0.1", 2000, "new", 2))
            .unwrap();
        let loaded = read_daemon_conn_file_at(&path).unwrap().unwrap();
        assert_eq!(loaded.port, 2000);
        assert_eq!(loaded.token, "new");
    }

    #[cfg(unix)]
    #[test]
    fn conn_file_is_written_as_0600() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.conn");
        write_daemon_conn_file_at(&path, &DaemonConnFile::new("127.0.0.1", 1, "t", 1)).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "freshly published conn file must be 0600");
    }

    #[test]
    fn conn_new_stamps_format_and_epoch_ms() {
        let conn = DaemonConnFile::new("127.0.0.1", 1, "t", 1);
        assert_eq!(conn.format, DAEMON_CONN_FORMAT);
        assert!(
            conn.started_at_ms > 1_600_000_000_000,
            "startedAtMs must be a plausible epoch-ms timestamp, got {}",
            conn.started_at_ms
        );
    }
}
