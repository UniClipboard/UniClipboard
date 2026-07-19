use std::str::FromStr;

use async_trait::async_trait;

/// Authoritative lifecycle state of a remote inbound receive attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptState {
    Receiving,
    Committing,
    Cancelling,
    Failing,
    Completed,
    Failed,
    Cancelled,
}

impl AttemptState {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

impl std::fmt::Display for AttemptState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Receiving => "receiving",
            Self::Committing => "committing",
            Self::Cancelling => "cancelling",
            Self::Failing => "failing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        };
        f.write_str(value)
    }
}

impl FromStr for AttemptState {
    type Err = AttemptError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "receiving" => Ok(Self::Receiving),
            "committing" => Ok(Self::Committing),
            "cancelling" => Ok(Self::Cancelling),
            "failing" => Ok(Self::Failing),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(AttemptError::InvalidState(value.to_owned())),
        }
    }
}

/// Current authoritative receive attempt for an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryReceiveAttempt {
    pub entry_id: String,
    pub current_attempt_id: String,
    pub state: AttemptState,
    pub updated_at_ms: i64,
}

/// Failure while reading or advancing receive attempt authority.
#[derive(Debug, thiserror::Error)]
pub enum AttemptError {
    #[error("entry receive attempt store error: {0}")]
    Backend(String),
    #[error("invalid persisted entry receive attempt state: {0}")]
    InvalidState(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeginReceiveOutcome {
    Begun,
    AlreadyReceiving,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestReceiveCancellationOutcome {
    Requested,
    AlreadyCancelling,
    TooLate,
    Terminal,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeginReceiveFailureOutcome {
    Begun,
    CancellationWon,
    Terminal,
    Superseded,
}

/// Query the current receive attempt for an entry.
#[async_trait]
pub trait GetEntryAttemptPort: Send + Sync {
    async fn get_entry_attempt(
        &self,
        entry_id: &str,
    ) -> Result<Option<EntryReceiveAttempt>, AttemptError>;
}

/// Query attempts that have not reached a terminal state.
#[async_trait]
pub trait ListNonTerminalAttemptsPort: Send + Sync {
    async fn list_non_terminal_attempts(&self) -> Result<Vec<EntryReceiveAttempt>, AttemptError>;
}

/// Start the first receive or replace an exact terminal attempt on redelivery.
#[async_trait]
pub trait BeginReceiveAttemptPort: Send + Sync {
    async fn begin_first_receive(
        &self,
        entry_id: &str,
        attempt_id: &str,
        now_ms: i64,
    ) -> Result<BeginReceiveOutcome, AttemptError>;

    async fn begin_redelivery(
        &self,
        entry_id: &str,
        expected_attempt_id: &str,
        new_attempt_id: &str,
        now_ms: i64,
    ) -> Result<BeginReceiveOutcome, AttemptError>;
}

/// Claim the exact receiving attempt for commit settlement.
#[async_trait]
pub trait ClaimReceiveCommitPort: Send + Sync {
    async fn claim_receive_commit(
        &self,
        entry_id: &str,
        attempt_id: &str,
        now_ms: i64,
    ) -> Result<bool, AttemptError>;
}

/// Request cancellation without finalizing settlement.
#[async_trait]
pub trait RequestReceiveCancellationPort: Send + Sync {
    async fn request_receive_cancellation(
        &self,
        entry_id: &str,
        attempt_id: &str,
        now_ms: i64,
    ) -> Result<RequestReceiveCancellationOutcome, AttemptError>;
}

/// Claim a receive for failure settlement.
#[async_trait]
pub trait BeginReceiveFailurePort: Send + Sync {
    async fn begin_receive_failure(
        &self,
        entry_id: &str,
        attempt_id: &str,
        now_ms: i64,
    ) -> Result<BeginReceiveFailureOutcome, AttemptError>;
}

/// Remove settled receive authority and projections after artifact cleanup.
#[async_trait]
pub trait DeleteReceiveStateForEntryPort: Send + Sync {
    async fn delete_receive_state_for_entry(&self, entry_id: &str) -> Result<bool, AttemptError>;
}

/// Purge old terminal attempts that never produced an entry and have no pending artifacts.
#[async_trait]
pub trait PurgeTerminalOrphanAttemptsPort: Send + Sync {
    async fn purge_terminal_orphan_attempts(&self, older_than_ms: i64)
        -> Result<u32, AttemptError>;
}
