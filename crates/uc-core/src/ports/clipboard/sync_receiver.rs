//! Clipboard receiver port (Slice 2 Phase 2).
//!
//! Complement to [`ClipboardDispatchPort`](super::sync_dispatch) — exposes
//! inbound payloads from peers on the clipboard ALPN as a broadcast event
//! stream. The application's `IngestInboundClipboardUseCase` subscribes
//! once at F1 `auto_start_network` completion and drives a background loop
//! that decrypts, dedupes and persists each arrival.
//!
//! `peer_device_id` is resolved by the adapter from the iroh connection's
//! remote endpoint id; unresolvable peers are rejected at the ALPN boundary
//! before reaching this stream.

use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::broadcast;

use super::sync_dispatch::ClipboardHeader;
use crate::ids::DeviceId;

/// One inbound clipboard delivery. Ciphertext is still sealed — decryption
/// and content-hash dedup happen in the application layer.
#[derive(Debug, Clone)]
pub struct InboundClipboard {
    pub peer_device_id: DeviceId,
    pub header: ClipboardHeader,
    pub ciphertext: Bytes,
}

/// Multi-consumer subscription to the inbound clipboard event stream.
///
/// Lagging receivers drop messages per `broadcast` contract. That is
/// acceptable: the next content-hash comparison in the ingest use case
/// will still surface missed entries the next time the peer dispatches
/// them.
#[async_trait]
pub trait ClipboardReceiverPort: Send + Sync {
    fn subscribe(&self) -> broadcast::Receiver<InboundClipboard>;
}
