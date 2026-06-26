//! Clipboard dispatch port (Slice 2 Phase 2).
//!
//! Replaces the frame-model [`ClipboardOutboundTransportPort`](super::transport)
//! with a business-semantic primitive: "send one clipboard entry's header +
//! ciphertext to one reachable peer, over a fresh stream". Multi-target
//! fan-out is assembled by `DispatchClipboardEntryUseCase` in
//! `uc-application`, not here, so the port stays minimal.
//!
//! The ciphertext is already V3-encoded + AEAD-sealed by the application
//! layer via `TransferCipherPort`; this port does not touch plaintext, nor
//! does it re-encrypt.

use async_trait::async_trait;
use bytes::Bytes;

use crate::ids::DeviceId;
use crate::ports::ConnectionChannel;

/// Wire-neutral clipboard header carried alongside the ciphertext payload.
///
/// `version` is **this port's** own wire format, independent of the pairing
/// `WIRE_VERSION` (Slice 1→2 bumped pairing to v=2 for
/// `transport_address_blob`; clipboard starts at v=1 because it has no
/// predecessor on this ALPN).
///
/// **Wire v2** (current) adds `flow_id` —— a cross-device trace correlation
/// identifier (UUIDv7 string). The sender embeds the id its outbound span
/// uses; the receiver lifts it onto its inbound span so a single business
/// action ("A 同步剪贴板给 B") shows up as one joined `flow.id` group in
/// Sentry. v1 frames decode with `flow_id = None`; the receiver tags those
/// as `flow.synthetic = true` and falls back to generating a local id so
/// downstream spans still carry *some* flow identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardHeader {
    pub version: u8,
    /// Whole-snapshot identity hash as a `"blake3v1:<hex>"` string. Shared
    /// with [`ClipboardEntry`](crate::clipboard::ClipboardEntry) for dedup
    /// (see `IngestInboundClipboardUseCase`).
    pub snapshot_hash: String,
    pub captured_at_ms: i64,
    pub origin_device_id: String,
    /// Plaintext device name. Passively propagated for future A5 rename;
    /// Phase 2 only transits the value.
    pub origin_device_name: String,
    /// Payload codec version — `3` for the existing
    /// `ClipboardBinaryPayload` V3 format. Reserved so a Phase N payload
    /// revision can live alongside V3 without a full ALPN bump.
    pub payload_version: u8,
    /// Cross-device trace correlation id (wire v2+). UUIDv7 string. `None`
    /// when received from a v1 peer; the receiver treats `None` as
    /// "generate a local synthetic id and tag the span accordingly".
    pub flow_id: Option<String>,
}

impl ClipboardHeader {
    /// Current clipboard wire version. Bumped only on incompatible changes.
    ///
    /// History:
    /// - v1: initial Slice 2 Phase 2 format (no `flow_id`)
    /// - v2: adds `flow_id` for cross-device trace correlation
    pub const CURRENT_VERSION: u8 = 2;
}

/// Opaque ciphertext already sealed by the application layer. Phase 2 keeps
/// the payload fully in memory; large payloads / files go through the
/// Slice 3 blob path.
#[derive(Debug, Clone)]
pub struct SyncPayload {
    pub ciphertext: Bytes,
}

/// Outcome of a single dispatch. Adapter-layer ack semantics only —
/// `Accepted` means the bytes reached the peer and its adapter accepted
/// them for ingest; it does **not** promise the application-level ingest
/// succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchAck {
    Accepted,
    DuplicateIgnored,
}

#[derive(Debug, thiserror::Error)]
pub enum ClipboardDispatchError {
    /// No reachable connection could be established (missing address or
    /// dial failure). Application layer treats this as "peer offline".
    #[error("target device offline or unreachable")]
    Offline,
    /// Local-side dispatch policy refused the payload before any wire
    /// activity (e.g. payload exceeds `MAX_PAYLOAD_SIZE` so we early-reject
    /// in the adapter rather than dial). The peer was never contacted; the
    /// caller is expected to route the content through a different channel
    /// (blob ref, file transfer, etc) or surface a user-facing limit.
    #[error("local policy rejected payload before dispatch: {0}")]
    LocalPolicyExceeded(String),
    /// Peer accepted the connection but rejected the payload at the wire
    /// boundary (bad header, unsupported version, etc). Carries the peer's
    /// reason string. This is a real round-trip — distinct from
    /// `LocalPolicyExceeded` which never reaches the peer.
    #[error("peer rejected: {0}")]
    PeerRejected(String),
    /// Stream I/O failure — broken connection, short read, etc.
    #[error("stream io: {0}")]
    Io(String),
    #[error("internal: {0}")]
    Internal(String),
}

/// Outcome of a single dispatch attempt paired with the connection path
/// actually used to reach the peer.
///
/// `transport` is observed when the dispatch settles, so it reflects the
/// path that carried — or attempted to carry — this frame. It is
/// [`ConnectionChannel::Unknown`] when no active path could be resolved:
/// the dial failed before any path established, or the snapshot was taken
/// mid-handshake. `outcome` carries the same wire ack / failure as before;
/// pairing the two lets the caller attribute both success and failure to
/// the path that produced it.
#[derive(Debug)]
pub struct DispatchReport {
    pub transport: ConnectionChannel,
    pub outcome: Result<DispatchAck, ClipboardDispatchError>,
}

/// Single-target, single-stream dispatch primitive.
///
/// Each call opens a fresh bi-stream on the clipboard channel, writes
/// magic + header + payload, reads the peer's one-byte ack, and closes.
/// Concurrent fan-out is the caller's responsibility. The returned
/// [`DispatchReport`] reports both the wire outcome and the connection
/// path that served this attempt.
#[async_trait]
pub trait ClipboardDispatchPort: Send + Sync {
    async fn dispatch(
        &self,
        target: &DeviceId,
        header: &ClipboardHeader,
        payload: SyncPayload,
    ) -> DispatchReport;
}
