use std::path::PathBuf;
use std::str::FromStr;

use async_trait::async_trait;

/// Durable phase of a directory publication attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishPhase {
    Staging,
    Publishing,
    Landed,
}

impl std::fmt::Display for PublishPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Staging => "staging",
            Self::Publishing => "publishing",
            Self::Landed => "landed",
        })
    }
}

impl FromStr for PublishPhase {
    type Err = PublishLogError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "staging" => Ok(Self::Staging),
            "publishing" => Ok(Self::Publishing),
            "landed" => Ok(Self::Landed),
            _ => Err(PublishLogError::InvalidPhase(value.to_owned())),
        }
    }
}

/// Durable publication metadata for one directory receive attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryPublishRecord {
    pub entry_id: String,
    pub attempt_id: String,
    pub phase: PublishPhase,
    pub root_map: Vec<(PathBuf, PathBuf)>,
    pub partial_visible_roots: Option<u32>,
    pub landed: bool,
}

/// Failure while reading or recording directory publication metadata.
#[derive(Debug, thiserror::Error)]
pub enum PublishLogError {
    #[error("directory publish log store error: {0}")]
    Backend(String),
    #[error("invalid persisted directory publish phase: {0}")]
    InvalidPhase(String),
    #[error("directory publish log encryption unavailable: {0}")]
    EncryptionUnavailable(String),
    #[error("directory publish log ciphertext is invalid")]
    InvalidCiphertext,
}

/// Failure while removing transient directory receive content.
#[derive(Debug, thiserror::Error)]
pub enum DirectoryStagingCleanupError {
    #[error("directory staging cleanup failed: {0}")]
    Backend(String),
    #[error("directory staging path is outside a receive staging area")]
    InvalidPath,
}

/// Remove transient roots belonging to a directory receive attempt.
#[async_trait]
pub trait CleanupDirectoryStagingPort: Send + Sync {
    /// Remove the staging areas containing `staged_roots`.
    ///
    /// Implementations must reject paths that do not identify dedicated
    /// receive staging areas and must be idempotent when an area is absent.
    async fn cleanup_staging_roots(
        &self,
        staged_roots: &[PathBuf],
    ) -> Result<(), DirectoryStagingCleanupError>;
}

/// Command directory publication phase and recovery metadata.
#[async_trait]
pub trait RecordDirectoryPublishPort: Send + Sync {
    /// Store the phase and complete staged-to-final root mapping atomically.
    async fn record_phase(
        &self,
        entry_id: &str,
        attempt_id: &str,
        phase: PublishPhase,
        root_map: &[(PathBuf, PathBuf)],
        now_ms: i64,
    ) -> Result<(), PublishLogError>;

    /// Record how many roots remain visible after an incomplete rollback.
    async fn record_partial(
        &self,
        entry_id: &str,
        attempt_id: &str,
        visible_roots: u32,
        now_ms: i64,
    ) -> Result<(), PublishLogError>;
}

/// Query directory publication metadata for recovery.
#[async_trait]
pub trait GetDirectoryPublishRecordPort: Send + Sync {
    async fn get_publish_record(
        &self,
        entry_id: &str,
        attempt_id: &str,
    ) -> Result<Option<DirectoryPublishRecord>, PublishLogError>;
}
