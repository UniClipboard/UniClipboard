//! Cross-process controlled-restart handover store (ADR-008 P5-L L7, R8-F1).
//!
//! A controlled restart needs the target run mode to survive the gap between
//! the OLD daemon exiting and a NEW daemon starting — the two processes never
//! share memory, so the intent is persisted as a small record in the
//! per-profile lock directory (beside `.uniclipd.lock` / `.daemon-pid`).
//!
//! ## Lock-holding semantics, not file existence
//!
//! The record's authority flows through the **instance-lock transfer**, not
//! from mere file presence:
//!
//! 1. **write** (L8): the controlled-restart requester writes the record while
//!    holding the OLD daemon's instance lock.
//! 2. **read** (spawner, [`crate::spawn::spawn_detached_daemon`]): the spawner
//!    reads it as a *hint* for which `RUN_MODE_ENV` to launch — nothing more.
//! 3. **clear** (NEW daemon): once the new daemon acquires the instance lock it
//!    consumes the handover by clearing the file (claim under lock — R8-F1).
//!
//! ## Production-neutral
//!
//! L7 ships the STORE primitive plus its READ/CLEAR lifecycle, but **no caller
//! writes a record yet** (the WRITE caller is L8). So in production [`read`]
//! always returns `None` (the spawn is unchanged) and [`clear`] is always a
//! no-op (startup is unchanged). The change is revert-safe.
//!
//! ## Best-effort, never blocks
//!
//! Everything here is best-effort and infallible at the read/clear boundary: a
//! missing or corrupt handover must NEVER block or fail a spawn or a daemon
//! startup. [`read`] returns `Option` (corrupt → `warn!` + `None`); [`clear`]
//! returns unit (missing → success, other errors → `warn!`). Only [`write`]
//! (L8, not called in production yet) surfaces an `io::Result`.

use std::path::{Path, PathBuf};

/// File name of the handover record inside the per-profile lock directory.
///
/// Sits beside `.uniclipd.lock` in the app data root (the lock dir).
pub const HANDOVER_FILE_NAME: &str = ".uniclipd-handover.json";

/// A controlled-restart handover record.
///
/// Persisted by the restart requester (L8) while it holds the OLD daemon's
/// instance lock, read by the spawner as a hint, and cleared by the NEW daemon
/// after it acquires the instance lock.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HandoverRecord {
    /// The run mode the new daemon should launch in. Stores a
    /// [`crate::spawn_contract`] `RUN_MODE_*` env value (e.g. `"server"` /
    /// `"oneshot"`) so the spawner can pass it straight to
    /// [`crate::spawn_contract::RUN_MODE_ENV`].
    pub target_mode: String,
    /// Monotonic counter L8 uses for restart-conflict arbitration.
    pub generation: u64,
}

/// Resolve the handover-record path inside `app_data_root` (the lock dir).
fn handover_path(app_data_root: &Path) -> PathBuf {
    app_data_root.join(HANDOVER_FILE_NAME)
}

/// Read the pending handover record, if any.
///
/// Best-effort: a missing file yields `None` (no log — that is the normal
/// production state); a present-but-unparseable file is logged via
/// `tracing::warn!` and yields `None` (a corrupt record must NOT block spawn).
/// Never returns `Err`.
pub fn read(app_data_root: &Path) -> Option<HandoverRecord> {
    let path = handover_path(app_data_root);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "failed to read handover record; ignoring",
            );
            return None;
        }
    };

    match serde_json::from_slice::<HandoverRecord>(&bytes) {
        Ok(record) => Some(record),
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "corrupt handover record; ignoring (will not block spawn)",
            );
            None
        }
    }
}

/// Write a handover record, serialized to JSON.
///
/// Called by the controlled-restart requester (L8) while holding the OLD
/// daemon's instance lock. Defined here in L7 so the store primitive is
/// complete and testable; L7 itself only calls this from tests.
///
/// ADR-008 P5-L L8a: the write is **atomic** — the JSON is written to a
/// temp file in the SAME directory (so `rename` stays on one filesystem) and
/// then `rename`d into place. A spawner's [`read`] therefore never observes a
/// half-written record. On any error the temp file is best-effort removed and
/// the `Err` is returned.
///
/// This is **visibility**-atomicity (a reader never sees a torn file), NOT
/// crash-durability: there is no `fsync`, so a crash mid-write may lose the
/// record. That is acceptable for a best-effort hint — a missing record reads
/// back as `None` (see [`read`]) and the spawn falls back to the default mode.
pub fn write(app_data_root: &Path, record: &HandoverRecord) -> std::io::Result<()> {
    let path = handover_path(app_data_root);
    let json = serde_json::to_vec(record).map_err(std::io::Error::other)?;

    // Temp file beside the final path (same dir → rename is atomic on one fs).
    // Include the pid so concurrent writers cannot stomp each other's temp file.
    let temp_path = app_data_root.join(format!("{HANDOVER_FILE_NAME}.{}.tmp", std::process::id()));

    if let Err(error) = std::fs::write(&temp_path, &json) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error);
    }

    if let Err(error) = std::fs::rename(&temp_path, &path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error);
    }

    Ok(())
}

/// Clear (consume) the pending handover record.
///
/// Called by the NEW daemon after it acquires the instance lock to consume the
/// handover (ADR-008 R8-F1). Best-effort: a `NotFound` error is treated as
/// success (no log — nothing to clear); any other error is logged via
/// `tracing::warn!`. Never returns `Err`, never blocks startup.
pub fn clear(app_data_root: &Path) {
    let path = handover_path(app_data_root);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "failed to clear handover record; ignoring",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_on_empty_dir_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path()), None);
    }

    #[test]
    fn write_then_read_round_trips_exact_record() {
        let dir = tempfile::tempdir().unwrap();
        let record = HandoverRecord {
            target_mode: "server".to_string(),
            generation: 42,
        };
        write(dir.path(), &record).unwrap();
        assert_eq!(read(dir.path()), Some(record));
    }

    #[test]
    fn atomic_write_leaves_no_temp_file_behind() {
        // ADR-008 P5-L L8a: the atomic write (temp + rename) must not leave the
        // intermediate temp file in the lock dir once it succeeds.
        let dir = tempfile::tempdir().unwrap();
        let record = HandoverRecord {
            target_mode: "server".to_string(),
            generation: 1,
        };
        write(dir.path(), &record).unwrap();

        assert!(handover_path(dir.path()).exists());
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "stray temp files: {leftovers:?}");

        // The record still round-trips via `read`.
        assert_eq!(read(dir.path()), Some(record));
    }

    #[test]
    fn clear_after_write_removes_file_and_subsequent_read_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let record = HandoverRecord {
            target_mode: "oneshot".to_string(),
            generation: 7,
        };
        write(dir.path(), &record).unwrap();
        assert!(handover_path(dir.path()).exists());

        clear(dir.path());
        assert!(!handover_path(dir.path()).exists());
        assert_eq!(read(dir.path()), None);
    }

    #[test]
    fn corrupt_record_reads_as_none_without_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(handover_path(dir.path()), b"{ not valid json").unwrap();
        // Must not panic or error — best-effort read returns None.
        assert_eq!(read(dir.path()), None);
    }

    #[test]
    fn clear_on_dir_with_no_record_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        // No record present — clear must not panic.
        clear(dir.path());
        assert!(!handover_path(dir.path()).exists());
    }
}
