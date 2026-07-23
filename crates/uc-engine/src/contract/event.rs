use std::fmt;

use serde::{Deserialize, Serialize};

use super::{EngineError, EngineState, LifecycleAction, OperationTerminal};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshReason {
    ConsumerLagged,
    StateInvalidated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    StateChanged {
        state: EngineState,
    },
    IncomingEntry(IncomingEntryEvent),
    InboundNotice(InboundNoticeEvent),
    IncomingPending(IncomingPendingEvent),
    ReceiveAttemptStateChanged(ReceiveAttemptStateChanged),
    TransferProgress(TransferProgress),
    TransferStatusChanged(TransferStatusChanged),
    DeliveryStatusChanged(DeliveryStatusChanged),
    PeerPresenceChanged(PeerPresenceChanged),
    ActiveClipboardChanged(ActiveClipboardChanged),
    MobileLanSettingsChanged(MobileLanSettingsChanged),
    RefreshRequired {
        reason: RefreshReason,
    },
    OperationFinished {
        operation_id: String,
        terminal: OperationTerminal,
    },
    LifecycleFailed {
        action: LifecycleAction,
        error: EngineError,
    },
    Fatal {
        error: EngineError,
    },
}

impl EngineEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::StateChanged { .. } => "state_changed",
            Self::IncomingEntry(_) => "incoming_entry",
            Self::InboundNotice(_) => "inbound_notice",
            Self::IncomingPending(_) => "incoming_pending",
            Self::ReceiveAttemptStateChanged(_) => "receive_attempt_state_changed",
            Self::TransferProgress(_) => "transfer_progress",
            Self::TransferStatusChanged(_) => "transfer_status_changed",
            Self::DeliveryStatusChanged(_) => "delivery_status_changed",
            Self::PeerPresenceChanged(_) => "peer_presence_changed",
            Self::ActiveClipboardChanged(_) => "active_clipboard_changed",
            Self::MobileLanSettingsChanged(_) => "mobile_lan_settings_changed",
            Self::RefreshRequired { .. } => "refresh_required",
            Self::OperationFinished { .. } => "operation_finished",
            Self::LifecycleFailed { .. } => "lifecycle_failed",
            Self::Fatal { .. } => "fatal",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardOriginSummary {
    Local,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboundNoticeActionSummary {
    NewEntry,
    DuplicateIgnored,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboundNoticeEvent {
    pub from_device: String,
    pub snapshot_hash: String,
    pub plaintext: Vec<u8>,
    pub text_preview: Option<String>,
    pub representations: Vec<InboundRepresentationSummary>,
    pub action: InboundNoticeActionSummary,
    pub at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboundRepresentationSummary {
    pub mime_type: Option<String>,
    pub size_bytes: i64,
}

impl fmt::Debug for InboundNoticeEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InboundNoticeEvent")
            .field("has_from_device", &!self.from_device.is_empty())
            .field("has_snapshot_hash", &!self.snapshot_hash.is_empty())
            .field("plaintext_len", &self.plaintext.len())
            .field("has_text_preview", &self.text_preview.is_some())
            .field("representation_count", &self.representations.len())
            .field("action", &self.action)
            .field("at_ms", &self.at_ms)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingEntryEvent {
    pub entry_id: String,
    pub attempt_id: Option<String>,
    pub preview: String,
    pub origin: ClipboardOriginSummary,
}

impl fmt::Debug for IncomingEntryEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IncomingEntryEvent")
            .field("has_entry_id", &!self.entry_id.is_empty())
            .field("has_attempt_id", &self.attempt_id.is_some())
            .field("preview_len", &self.preview.len())
            .field("origin", &self.origin)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingPendingEvent {
    pub entry_id: String,
    pub attempt_id: Option<String>,
    pub from_device: String,
    pub total_bytes: Option<u64>,
    pub filenames: Vec<String>,
}

impl fmt::Debug for IncomingPendingEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IncomingPendingEvent")
            .field("has_entry_id", &!self.entry_id.is_empty())
            .field("has_attempt_id", &self.attempt_id.is_some())
            .field("has_from_device", &!self.from_device.is_empty())
            .field("total_bytes", &self.total_bytes)
            .field("file_count", &self.filenames.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiveAttemptStateChanged {
    pub entry_id: String,
    pub attempt_id: String,
    pub state: String,
}

impl fmt::Debug for ReceiveAttemptStateChanged {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReceiveAttemptStateChanged")
            .field("has_entry_id", &!self.entry_id.is_empty())
            .field("has_attempt_id", &!self.attempt_id.is_empty())
            .field("state", &self.state)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferDirectionSummary {
    Sending,
    Receiving,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferProgress {
    pub transfer_id: String,
    pub entry_id: Option<String>,
    pub attempt_id: Option<String>,
    pub peer_id: String,
    pub direction: TransferDirectionSummary,
    pub completed_bytes: u64,
    pub total_bytes: Option<u64>,
}

impl fmt::Debug for TransferProgress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransferProgress")
            .field("has_transfer_id", &!self.transfer_id.is_empty())
            .field("has_entry_id", &self.entry_id.is_some())
            .field("has_attempt_id", &self.attempt_id.is_some())
            .field("has_peer_id", &!self.peer_id.is_empty())
            .field("direction", &self.direction)
            .field("completed_bytes", &self.completed_bytes)
            .field("total_bytes", &self.total_bytes)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferStatusChanged {
    pub transfer_id: String,
    pub entry_id: String,
    pub attempt_id: Option<String>,
    pub status: String,
    pub reason: Option<String>,
}

impl fmt::Debug for TransferStatusChanged {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransferStatusChanged")
            .field("has_transfer_id", &!self.transfer_id.is_empty())
            .field("has_entry_id", &!self.entry_id.is_empty())
            .field("has_attempt_id", &self.attempt_id.is_some())
            .field("status", &self.status)
            .field("has_reason", &self.reason.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryStatusChanged {
    pub entry_id: String,
    pub target_device_id: String,
}

impl fmt::Debug for DeliveryStatusChanged {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeliveryStatusChanged")
            .field("has_entry_id", &!self.entry_id.is_empty())
            .field("has_target_device_id", &!self.target_device_id.is_empty())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerPresenceChanged {
    pub device_id: String,
    pub state: String,
    pub at_ms: i64,
}

impl fmt::Debug for PeerPresenceChanged {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PeerPresenceChanged")
            .field("has_device_id", &!self.device_id.is_empty())
            .field("state", &self.state)
            .field("at_ms", &self.at_ms)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveClipboardChanged {
    pub snapshot_hash: String,
    pub entry_id: String,
    pub activated_at_ms: i64,
    pub activated_by: String,
}

impl fmt::Debug for ActiveClipboardChanged {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ActiveClipboardChanged")
            .field("has_snapshot_hash", &!self.snapshot_hash.is_empty())
            .field("has_entry_id", &!self.entry_id.is_empty())
            .field("activated_at_ms", &self.activated_at_ms)
            .field("has_activated_by", &!self.activated_by.is_empty())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobileLanSettingsChanged {
    pub enabled: bool,
    pub lan_listen_enabled: bool,
    pub lan_port: Option<u16>,
}
