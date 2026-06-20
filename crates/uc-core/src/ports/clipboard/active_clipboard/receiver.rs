//! Inbound active-clipboard register port.
//!
//! Complement to [`AdvanceActiveClipboardPort`](super::active_clipboard_register::AdvanceActiveClipboardPort):
//! that port advances the local register, this one exposes the active-clipboard
//! state observations that arrive from peers as a broadcast event stream.

use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::ids::DeviceId;

/// One observation of a peer's active-clipboard register value.
///
/// The cross-device identity of the content is
/// [`snapshot_hash`](Self::snapshot_hash); two peers holding identical
/// clipboard content report the same value. The pair
/// `(activated_at_ms, activated_by)` is the last-writer-wins order used to
/// decide whether this observation supersedes the locally stored value.
///
/// [`sender_entry_id`](Self::sender_entry_id) is the sending peer's own
/// per-device handle for the content. It is carried verbatim for
/// traceability but is **not** a cross-device identity: a consumer must
/// resolve the content locally by `snapshot_hash`, never by this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundActiveClipboardState {
    /// The peer that reported this observation. Resolved from the inbound
    /// connection's identity before the observation reaches this stream.
    pub peer_device_id: DeviceId,
    /// Stable, cross-device content identity string (`"blake3v1:<hex>"`).
    pub snapshot_hash: String,
    /// The sending peer's local entry handle for the content. Per-device
    /// only; never compared across devices or used to resolve local content.
    pub sender_entry_id: String,
    /// Wall-clock milliseconds of the activation event on the originating
    /// device. Primary last-writer-wins key.
    pub activated_at_ms: i64,
    /// The device that performed the activation. Last-writer-wins tiebreaker
    /// and attribution only.
    pub activated_by: DeviceId,
}

/// Multi-consumer subscription to the inbound active-clipboard state stream.
///
/// Lagging receivers drop messages per the `broadcast` contract. That is
/// acceptable: the register is a convergent last-writer-wins value, so a
/// dropped observation is recovered by the next observation a peer reports.
#[async_trait]
pub trait ActiveClipboardReceiverPort: Send + Sync {
    fn subscribe(&self) -> broadcast::Receiver<InboundActiveClipboardState>;
}
