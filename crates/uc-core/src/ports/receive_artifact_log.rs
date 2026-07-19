use std::path::PathBuf;

use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveArtifactPhase {
    Preparing,
    Publishing,
    RollingBack,
    Settled,
}

impl ReceiveArtifactPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Publishing => "publishing",
            Self::RollingBack => "rolling_back",
            Self::Settled => "settled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ReceiveArtifactLogError> {
        match value {
            "preparing" => Ok(Self::Preparing),
            "publishing" => Ok(Self::Publishing),
            "rolling_back" => Ok(Self::RollingBack),
            "settled" => Ok(Self::Settled),
            other => Err(ReceiveArtifactLogError::InvalidValue(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveArtifactResolution {
    Pending,
    Landed,
    /// Cleanup is complete for an attempt that did not commit. This removes
    /// managed staging but never implies deletion of user-destination finals.
    RolledBack,
}

impl ReceiveArtifactResolution {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Landed => "landed",
            Self::RolledBack => "rolled_back",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ReceiveArtifactLogError> {
        match value {
            "pending" => Ok(Self::Pending),
            "landed" => Ok(Self::Landed),
            "rolled_back" => Ok(Self::RolledBack),
            other => Err(ReceiveArtifactLogError::InvalidValue(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveArtifactOwnership {
    ManagedStaging,
    UserDestination,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiveArtifact {
    pub item_id: String,
    pub staged_path: PathBuf,
    pub final_path: PathBuf,
    pub ownership: ReceiveArtifactOwnership,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiveArtifactRecord {
    pub entry_id: String,
    pub attempt_id: String,
    pub phase: ReceiveArtifactPhase,
    pub resolution: ReceiveArtifactResolution,
    pub artifacts: Vec<ReceiveArtifact>,
    pub updated_at_ms: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum ReceiveArtifactLogError {
    #[error("receive artifact log store error: {0}")]
    Backend(String),
    #[error("receive artifact encryption unavailable: {0}")]
    EncryptionUnavailable(String),
    #[error("invalid receive artifact log value: {0}")]
    InvalidValue(String),
}

#[async_trait]
pub trait RecordReceiveArtifactsPort: Send + Sync {
    async fn record_receive_artifacts(
        &self,
        record: &ReceiveArtifactRecord,
    ) -> Result<(), ReceiveArtifactLogError>;
}

#[async_trait]
pub trait GetReceiveArtifactRecordPort: Send + Sync {
    async fn get_receive_artifact_record(
        &self,
        entry_id: &str,
        attempt_id: &str,
    ) -> Result<Option<ReceiveArtifactRecord>, ReceiveArtifactLogError>;
}

#[async_trait]
pub trait ListUnsettledReceiveArtifactsPort: Send + Sync {
    async fn list_unsettled_receive_artifacts(
        &self,
    ) -> Result<Vec<ReceiveArtifactRecord>, ReceiveArtifactLogError>;
}

#[async_trait]
pub trait CleanupReceiveArtifactsPort: Send + Sync {
    /// Remove attempt-owned temporary artifacts. Implementations must never
    /// remove a `UserDestination` final path; when staging and final paths are
    /// identical, the path is entirely user-owned and must be left untouched.
    async fn cleanup_receive_artifacts(
        &self,
        artifacts: &[ReceiveArtifact],
    ) -> Result<(), ReceiveArtifactLogError>;
}
