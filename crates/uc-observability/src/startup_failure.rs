//! Bounded record of a daemon start that failed before its logs were usable.
//!
//! Failures that happen while the daemon is still claiming its profile (path
//! resolution, the instance lock, tracing setup) cannot be written to the log
//! files they would normally appear in. They are recorded here instead: one
//! small file in the log directory that holds the most recent failure only, so
//! it cannot grow, and that the diagnostic export includes next to the logs.

use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Name of the record inside the log directory.
pub const STARTUP_FAILURE_FILE: &str = "daemon-startup-failure.json";

/// Upper bound of the record. The schema is fixed and carries no free text, so
/// a larger file was not written by this module and is refused.
pub const MAX_STARTUP_FAILURE_BYTES: u64 = 4096;

/// Step of the daemon start that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupStep {
    PathResolution,
    InstanceLock,
    Tracing,
    HostPreparation,
    EngineStart,
    ServiceStart,
}

/// Coarse cause of the failure. Never carries error text, which may hold paths
/// or other user content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupFailureClass {
    /// Another daemon held the profile for the whole wait.
    LockTimeout,
    /// A filesystem or operating-system call failed.
    Io,
    /// Any other failure of the step.
    Internal,
}

/// The most recent failed daemon start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupFailure {
    pub schema_version: u8,
    /// Run identity shared with every host log record of the same process.
    pub run_id: String,
    pub pid: u32,
    pub step: StartupStep,
    pub class: StartupFailureClass,
    pub recorded_at: DateTime<Utc>,
}

impl StartupFailure {
    /// A failure of `step` observed now by this process.
    pub fn now(step: StartupStep, class: StartupFailureClass) -> Self {
        Self {
            schema_version: 1,
            run_id: crate::run_id().to_owned(),
            pid: crate::pid(),
            step,
            class,
            recorded_at: Utc::now(),
        }
    }
}

/// Replace the record in `logs_dir`, creating the directory when it is missing.
///
/// The new content is written to a temporary file and renamed over the old
/// record, so a reader sees either the previous failure or this one.
pub fn write_startup_failure(logs_dir: &Path, failure: &StartupFailure) -> io::Result<()> {
    fs::create_dir_all(logs_dir)?;
    let mut temporary = tempfile::NamedTempFile::new_in(logs_dir)?;
    serde_json::to_writer_pretty(&mut temporary, failure).map_err(io::Error::from)?;
    temporary.write_all(b"\n")?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(logs_dir.join(STARTUP_FAILURE_FILE))
        .map_err(|error| error.error)?;
    Ok(())
}

/// Read the record from `logs_dir`; `None` when no start has failed.
pub fn read_startup_failure(logs_dir: &Path) -> io::Result<Option<StartupFailure>> {
    let file = match fs::File::open(logs_dir.join(STARTUP_FAILURE_FILE)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_STARTUP_FAILURE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STARTUP_FAILURE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "startup failure record exceeds its size bound",
        ));
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(io::Error::from)
}
