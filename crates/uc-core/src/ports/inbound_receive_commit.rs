use async_trait::async_trait;

use crate::clipboard::{
    ClipboardEntry, ClipboardEntryContentCategory, ClipboardEvent, ClipboardSelectionDecision,
    EntryFileSet, PersistedClipboardRepresentation,
};
use crate::ids::EntryId;

#[derive(Clone)]
pub enum InboundReceiveRecord {
    Create {
        entry: ClipboardEntry,
        event: ClipboardEvent,
        representations: Vec<PersistedClipboardRepresentation>,
        selection: ClipboardSelectionDecision,
    },
    Replace {
        entry_id: EntryId,
        new_event: ClipboardEvent,
        new_representations: Vec<PersistedClipboardRepresentation>,
        new_selection: ClipboardSelectionDecision,
        new_total_size: i64,
        new_content_category: ClipboardEntryContentCategory,
    },
}

impl InboundReceiveRecord {
    pub fn entry_id(&self) -> &EntryId {
        match self {
            Self::Create { entry, .. } => &entry.entry_id,
            Self::Replace { entry_id, .. } => entry_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletedReceiveArtifacts {
    None,
    Landed,
    DirectoryPublished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialReceiveArtifacts {
    None,
    Landed,
    RolledBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoEntryReceiveArtifacts {
    None,
    /// The attempt no longer owns any cleanup work. Managed staging has been
    /// removed, while user-destination final paths were deliberately preserved
    /// and relinquished according to their ownership contract.
    RolledBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialReceiveTerminal {
    Cancelled,
    Failed,
}

pub enum InboundReceiveSettlement {
    Complete {
        record: InboundReceiveRecord,
        attempt_id: String,
        file_set: Option<EntryFileSet>,
        artifacts: CompletedReceiveArtifacts,
        now_ms: i64,
    },
    Partial {
        record: InboundReceiveRecord,
        attempt_id: String,
        terminal: PartialReceiveTerminal,
        file_set: Option<EntryFileSet>,
        artifacts: PartialReceiveArtifacts,
        now_ms: i64,
    },
    NoEntry {
        entry_id: EntryId,
        attempt_id: String,
        terminal: PartialReceiveTerminal,
        artifacts: NoEntryReceiveArtifacts,
        now_ms: i64,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum InboundReceiveCommitError {
    #[error("receive attempt is not in the required settlement state")]
    AttemptStateMismatch,
    #[error("required receive artifact record is missing")]
    ArtifactRecordMissing,
    #[error("required directory publication record is missing")]
    DirectoryPublishRecordMissing,
    #[error("inbound receive commit store error: {0}")]
    Backend(String),
}

#[async_trait]
pub trait CommitInboundReceivePort: Send + Sync {
    async fn commit_inbound_receive(
        &self,
        settlement: &InboundReceiveSettlement,
    ) -> Result<(), InboundReceiveCommitError>;
}
