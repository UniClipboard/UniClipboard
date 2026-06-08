//! Host event port — domain events emitted by the application toward
//! the host environment (GUI shell, daemon API, etc.).
//!
//! These events carry semantic notifications about clipboard, transfer,
//! and delivery state changes. The host decides how to render/forward them.

use crate::file_transfer::FileTransferDirection;

/// Clipboard content origin.
#[derive(Debug, Clone)]
pub enum ClipboardOriginKind {
    Local,
    Remote,
}

/// Clipboard subsystem events for the host.
#[derive(Debug, Clone)]
pub enum ClipboardHostEvent {
    NewContent {
        entry_id: String,
        preview: String,
        origin: ClipboardOriginKind,
    },
    /// An inbound clipboard entry has been confirmed — the V3 envelope is
    /// decoded but blob fetch has not started / is in progress. The host can
    /// render a placeholder card immediately.
    IncomingPending {
        entry_id: String,
        from_device: String,
        /// Total blob bytes declared in the envelope. `None` when no blobs
        /// (pure-text sync).
        total_bytes: Option<u64>,
        /// Filenames collected from `blob_refs[i].filename` in the V3 envelope.
        filenames: Vec<String>,
    },
}

/// File transfer subsystem events for the host.
#[derive(Debug, Clone)]
pub enum TransferHostEvent {
    StatusChanged {
        transfer_id: String,
        entry_id: String,
        status: String,
        reason: Option<String>,
    },
    Progress {
        transfer_id: String,
        entry_id: Option<String>,
        peer_id: String,
        direction: FileTransferDirection,
        bytes_transferred: u64,
        total_bytes: Option<u64>,
    },
}

/// Entry delivery subsystem events for the host.
///
/// Payload intentionally omits `status` — subscribers use the event as a
/// "refetch" hint and query the authoritative view for the actual status.
#[derive(Debug, Clone)]
pub enum DeliveryHostEvent {
    /// Delivery status for an entry toward a specific peer has changed.
    StatusChanged {
        entry_id: String,
        target_device_id: String,
    },
}

/// Unified host event envelope.
#[derive(Debug, Clone)]
pub enum HostEvent {
    Clipboard(ClipboardHostEvent),
    Transfer(TransferHostEvent),
    Delivery(DeliveryHostEvent),
}

/// Error returned when emitting a host event fails.
#[derive(Debug, thiserror::Error)]
pub enum EmitError {
    #[error("emit failed: {0}")]
    Failed(String),
}

/// Port for emitting host events from the application toward the host
/// environment.
pub trait HostEventEmitterPort: Send + Sync {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError>;
}
