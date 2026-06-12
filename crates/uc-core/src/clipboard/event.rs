use super::hash::ContentHash;
use crate::{clipboard::system::SnapshotHash, ids::EventId, DeviceId};

/// Event representing a user-initiated action on clipboard content.
#[derive(Debug, Clone, PartialEq)]
pub enum ClipboardContentActionEvent {
    /// User requested an action on clipboard content identified by hash.
    UserRequested {
        content_hash: ContentHash,
        action: ClipboardContentAction,
    },
}

/// Actions that can be performed on clipboard content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardContentAction {
    /// Copy content from history to system clipboard.
    CopyToSystemClipboard,
    /// Delete content from history.
    Delete,
    /// Pin content to prevent automatic deletion.
    Pin,
    /// Unpin previously pinned content.
    Unpin,
}

pub struct ClipboardEvent {
    pub event_id: EventId,
    pub captured_at_ms: i64,
    pub source_device: DeviceId,
    pub snapshot_hash: SnapshotHash,
}

impl ClipboardEvent {
    pub fn new(
        event_id: EventId,
        captured_at_ms: i64,
        source_device: DeviceId,
        snapshot_hash: SnapshotHash,
    ) -> Self {
        Self {
            event_id,
            captured_at_ms,
            source_device,
            snapshot_hash,
        }
    }
}
