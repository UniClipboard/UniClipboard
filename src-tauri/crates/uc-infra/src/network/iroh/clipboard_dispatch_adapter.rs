//! Iroh-backed implementation of [`ClipboardDispatchPort`] (Slice 2 Phase 2).
//!
//! Each call opens a fresh iroh bi-stream on [`CLIPBOARD_ALPN`], writes the
//! framed header + ciphertext per [`crate::network::iroh::clipboard_wire`],
//! reads the peer's single-byte ack, and closes. Concurrent fan-out to
//! multiple peers is assembled by the application-layer dispatch use case;
//! this adapter stays single-target.
//!
//! Failure mapping is deliberately narrow:
//!
//! * `peer_addr_repo.get()` returning `None` (no stored transport address
//!   for this peer) → [`ClipboardDispatchError::Offline`]. From the caller's
//!   perspective this is indistinguishable from a dial-time failure, and
//!   collapsing them avoids a second error variant with no unique recovery
//!   path.
//! * Stored blob decode failure or dial failure → also `Offline`. The
//!   address record is either stale (peer's addr rotated) or the peer is
//!   genuinely unreachable; both cases correct themselves the next time
//!   the peer dispatches to us or pairing refreshes the record.
//! * Stream write / read I/O failure →
//!   [`ClipboardDispatchError::Io`].
//! * Peer sent a single-byte ack other than Accepted / DuplicateIgnored →
//!   [`ClipboardDispatchError::PeerRejected`] with the code embedded.
//! * Local boundary check (oversized payload, etc) refuses the payload
//!   before any wire activity →
//!   [`ClipboardDispatchError::LocalPolicyExceeded`]. This is **not** a
//!   peer-side rejection — peer was never contacted; caller is expected
//!   to route via blob ref / file transfer instead of retrying this peer.

use std::sync::Arc;

use async_trait::async_trait;
use iroh::{Endpoint, EndpointAddr};
use tracing::{debug, instrument, warn};

use uc_core::ids::DeviceId;
use uc_core::ports::{
    ClipboardDispatchError, ClipboardDispatchPort, ClipboardHeader, DispatchAck,
    PeerAddressRepositoryPort, SyncPayload,
};

use super::clipboard_wire::{self, AckCode, WireEncodeError};
use super::connect::connect_with_staggered_retry;

/// ALPN identifier for the Slice 2 clipboard sync protocol. Independent of
/// the presence / pairing ALPNs so the Router can multiplex all three
/// transports on the same endpoint.
pub const CLIPBOARD_ALPN: &[u8] = b"uniclipboard/clipboard/0";

pub struct IrohClipboardDispatchAdapter {
    endpoint: Arc<Endpoint>,
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
}

impl IrohClipboardDispatchAdapter {
    pub fn new(
        endpoint: Arc<Endpoint>,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    ) -> Self {
        Self {
            endpoint,
            peer_addr_repo,
        }
    }

    /// Resolve the peer's current [`EndpointAddr`] from the repository.
    /// Returns `Ok(None)` when no record exists (maps to `Offline`) so the
    /// dispatch path stays branchless; any other failure (postcard / repo
    /// infra) surfaces as `Offline` after logging — a corrupt blob is a
    /// data-integrity issue the next pairing sync will self-heal.
    async fn resolve_addr(&self, target: &DeviceId) -> Option<EndpointAddr> {
        match self.peer_addr_repo.get(target).await {
            // Stored blobs are guaranteed to be persistable form (NodeId
            // + Relay hint, no `Ip(...)` ephemera) — see
            // `persistable_addr::to_persistable_addr`. Iroh's pkarr
            // discovery resolves the peer's current direct addrs at
            // connect time, so we hand the decoded addr through as-is.
            Ok(Some(record)) => match postcard::from_bytes::<EndpointAddr>(&record.addr_blob) {
                Ok(addr) => Some(addr),
                Err(err) => {
                    warn!(
                        device = %target.as_str(),
                        error = %err,
                        "clipboard dispatch: peer_addr_repo blob did not postcard-decode; \
                         treating peer as offline"
                    );
                    None
                }
            },
            Ok(None) => None,
            Err(err) => {
                warn!(
                    device = %target.as_str(),
                    error = %err,
                    "clipboard dispatch: peer_addr_repo.get failed; treating peer as offline"
                );
                None
            }
        }
    }
}

#[async_trait]
impl ClipboardDispatchPort for IrohClipboardDispatchAdapter {
    #[instrument(skip_all, fields(device = %target.as_str(), payload_len = payload.ciphertext.len()))]
    async fn dispatch(
        &self,
        target: &DeviceId,
        header: &ClipboardHeader,
        payload: SyncPayload,
    ) -> Result<DispatchAck, ClipboardDispatchError> {
        // 1. Early-reject oversized payloads at the adapter boundary so
        //    the caller gets a clean error without us opening a stream
        //    just to tear it back down. This is a *local* policy check —
        //    no wire activity yet, peer never contacted. Surface as
        //    `LocalPolicyExceeded` so callers don't misread it as a peer
        //    decision (the caller's correct response is to re-dispatch
        //    via blob ref / file transfer, not to retry this peer).
        if payload.ciphertext.len() > clipboard_wire::MAX_PAYLOAD_SIZE as usize {
            return Err(ClipboardDispatchError::LocalPolicyExceeded(format!(
                "ciphertext {} bytes exceeds wire MAX_PAYLOAD_SIZE {}",
                payload.ciphertext.len(),
                clipboard_wire::MAX_PAYLOAD_SIZE
            )));
        }

        // 2. Resolve address; missing / bad record = offline.
        let addr = match self.resolve_addr(target).await {
            Some(a) => a,
            None => return Err(ClipboardDispatchError::Offline),
        };

        // 3. Dial. Dial failure = offline (no typed iroh error leaks up).
        let connection = connect_with_staggered_retry(
            Arc::clone(&self.endpoint),
            addr,
            CLIPBOARD_ALPN,
            "clipboard",
        )
        .await
        .map_err(|err| {
            debug!(error = %err, "clipboard dispatch: dial failed, treating as Offline");
            ClipboardDispatchError::Offline
        })?;

        // 4. Open one bi-stream for this message.
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|err| ClipboardDispatchError::Io(format!("open_bi: {err}")))?;

        // 5. Write the frame + close the send half so the peer's read_exact
        //    on the payload length / body reaches a terminal state.
        clipboard_wire::write_frame(&mut send, header, &payload.ciphertext)
            .await
            .map_err(|err| map_encode_err(err))?;
        send.finish()
            .map_err(|err| ClipboardDispatchError::Io(format!("send.finish: {err}")))?;

        // 6. Read the one-byte ack.
        let mut ack_buf = [0u8; 1];
        recv.read_exact(&mut ack_buf)
            .await
            .map_err(|err| ClipboardDispatchError::Io(format!("ack read: {err}")))?;

        // 7. Close the connection. Adapter owns the connection lifecycle —
        //    dropping here signals `Connection::closed` on the peer. The
        //    Q4 per-dispatch-stream decision (see plan §3.1) means we do
        //    not cache connections.
        drop(recv);
        drop(send);
        drop(connection);

        // 8. Interpret the ack byte. Any unknown code is adapter-level
        //    rejection rather than an ignored success.
        match AckCode::try_from(ack_buf[0]) {
            Ok(AckCode::Accepted) => Ok(DispatchAck::Accepted),
            Ok(AckCode::DuplicateIgnored) => Ok(DispatchAck::DuplicateIgnored),
            Ok(AckCode::Rejected) => Err(ClipboardDispatchError::PeerRejected(
                "peer returned Rejected ack".to_string(),
            )),
            Err(err) => Err(ClipboardDispatchError::PeerRejected(format!(
                "peer returned unknown ack byte: {err}"
            ))),
        }
    }
}

/// Map wire-encoding failures into the public error type without leaking
/// postcard / io internals upward. `HeaderTooLarge` is an adapter-side bug
/// (the header we built is oversized), so we surface it as `Internal` even
/// though the wire type name suggests otherwise.
fn map_encode_err(err: WireEncodeError) -> ClipboardDispatchError {
    match err {
        WireEncodeError::Io(ioerr) => ClipboardDispatchError::Io(format!("frame write: {ioerr}")),
        WireEncodeError::PayloadTooLarge { size, max } => {
            // wire codec also enforces MAX_PAYLOAD_SIZE; if we somehow get
            // here (the upstream early-reject in `dispatch` should have
            // caught it first), surface the same `LocalPolicyExceeded`
            // semantics — peer was never reached.
            ClipboardDispatchError::LocalPolicyExceeded(format!(
                "wire codec payload {size} bytes exceeds local maximum {max}"
            ))
        }
        WireEncodeError::HeaderTooLarge { size, max } => ClipboardDispatchError::Internal(format!(
            "self-built header {size} bytes exceeds {max}"
        )),
        WireEncodeError::Postcard(err) => {
            ClipboardDispatchError::Internal(format!("header encode: {err}"))
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Duration;

    use async_trait::async_trait;
    use bytes::Bytes;
    use chrono::Utc;
    use iroh::protocol::{AcceptError, ProtocolHandler, Router};
    use iroh::{Endpoint, RelayMode};

    use uc_core::ports::{PeerAddressError, PeerAddressRecord};

    /// In-memory peer_addr_repo the tests use to inject an address blob
    /// for a target device. Mirrors the surface the adapter needs without
    /// pulling in the Diesel implementation.
    #[derive(Default)]
    struct MemRepo {
        inner: tokio::sync::Mutex<std::collections::HashMap<String, PeerAddressRecord>>,
    }

    #[async_trait]
    impl PeerAddressRepositoryPort for MemRepo {
        async fn get(
            &self,
            device: &DeviceId,
        ) -> Result<Option<PeerAddressRecord>, PeerAddressError> {
            Ok(self.inner.lock().await.get(device.as_str()).cloned())
        }
        async fn upsert(&self, record: &PeerAddressRecord) -> Result<(), PeerAddressError> {
            self.inner
                .lock()
                .await
                .insert(record.device_id.as_str().to_string(), record.clone());
            Ok(())
        }
        async fn list(&self) -> Result<Vec<PeerAddressRecord>, PeerAddressError> {
            Ok(self.inner.lock().await.values().cloned().collect())
        }
        async fn remove(&self, device: &DeviceId) -> Result<(), PeerAddressError> {
            self.inner.lock().await.remove(device.as_str());
            Ok(())
        }
    }

    async fn bind_endpoint() -> Arc<Endpoint> {
        Arc::new(
            Endpoint::builder(iroh::endpoint::presets::N0)
                .alpns(vec![CLIPBOARD_ALPN.to_vec()])
                .relay_mode(RelayMode::Disabled)
                .bind()
                .await
                .expect("bind endpoint"),
        )
    }

    async fn wait_for_direct_addrs(endpoint: &Endpoint) {
        for _ in 0..100 {
            if !endpoint.addr().addrs.is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("endpoint never published direct addresses");
    }

    /// Stash an iroh `EndpointAddr` into the mem repo under the given
    /// device id, postcard-encoded the same way the pairing wire does.
    async fn seed_addr(repo: &MemRepo, device: &DeviceId, addr: &EndpointAddr) {
        let blob = postcard::to_stdvec(addr).expect("postcard encode EndpointAddr");
        repo.upsert(&PeerAddressRecord {
            device_id: device.clone(),
            addr_blob: blob,
            observed_at: Utc::now(),
        })
        .await
        .expect("upsert");
    }

    fn sample_header() -> ClipboardHeader {
        ClipboardHeader {
            version: ClipboardHeader::CURRENT_VERSION,
            content_hash: "c".repeat(64),
            captured_at_ms: 1_700_000_000_000,
            origin_device_id: "sender-001".to_string(),
            origin_device_name: "Sender".to_string(),
            payload_version: 3,
        }
    }

    /// Test-only handler that echoes a fixed ack byte after draining the
    /// frame. Stored as `Arc<[u8; 1]>` so each spawned task pulls the same
    /// configured ack.
    #[derive(Clone)]
    struct FixedAckHandler {
        ack_byte: u8,
    }

    impl std::fmt::Debug for FixedAckHandler {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("FixedAckHandler")
                .field("ack", &format!("0x{:02X}", self.ack_byte))
                .finish()
        }
    }

    impl ProtocolHandler for FixedAckHandler {
        async fn accept(&self, connection: iroh::endpoint::Connection) -> Result<(), AcceptError> {
            let ack = self.ack_byte;
            let (mut send, mut recv) = connection
                .accept_bi()
                .await
                .map_err(|err| AcceptError::from_err(err))?;
            // Consume one frame; failure here is fine in negative tests
            // that send bad headers.
            let _ = clipboard_wire::read_frame(&mut recv).await;
            // Emit the configured ack byte regardless.
            send.write_all(&[ack])
                .await
                .map_err(|err| AcceptError::from_err(err))?;
            send.finish().map_err(|err| AcceptError::from_err(err))?;
            let _ = connection.closed().await;
            Ok(())
        }
    }

    async fn spawn_ack_endpoint(ack: u8) -> (Arc<Endpoint>, Router) {
        let endpoint = bind_endpoint().await;
        wait_for_direct_addrs(&endpoint).await;
        let router = Router::builder((*endpoint).clone())
            .accept(CLIPBOARD_ALPN, FixedAckHandler { ack_byte: ack })
            .spawn();
        (endpoint, router)
    }

    /// Verdict 1 — happy path. Peer is reachable + accepts the frame.
    /// Dispatch returns `DispatchAck::Accepted`.
    #[tokio::test]
    async fn dispatch_returns_accepted_on_peer_ack_0x01() {
        let (peer_endpoint, peer_router) = spawn_ack_endpoint(AckCode::Accepted.as_byte()).await;
        let peer_addr = peer_endpoint.addr();

        let sender_endpoint = bind_endpoint().await;
        wait_for_direct_addrs(&sender_endpoint).await;
        let repo = Arc::new(MemRepo::default());
        let target = DeviceId::new("target-alpha");
        seed_addr(&repo, &target, &peer_addr).await;

        let adapter = IrohClipboardDispatchAdapter::new(sender_endpoint, repo);
        let payload = SyncPayload {
            ciphertext: Bytes::from(vec![0x11, 0x22, 0x33, 0x44]),
        };

        let ack = adapter
            .dispatch(&target, &sample_header(), payload)
            .await
            .expect("dispatch succeeds");
        assert_eq!(ack, DispatchAck::Accepted);

        peer_router.shutdown().await.expect("router shutdown");
    }

    /// Verdict 2 — peer returns the duplicate-ignored ack. Dispatch still
    /// returns `Ok`, and the specific variant propagates so the use case
    /// can report it distinct from `Accepted`.
    #[tokio::test]
    async fn dispatch_returns_duplicate_on_peer_ack_0x02() {
        let (peer_endpoint, peer_router) =
            spawn_ack_endpoint(AckCode::DuplicateIgnored.as_byte()).await;
        let peer_addr = peer_endpoint.addr();

        let sender_endpoint = bind_endpoint().await;
        wait_for_direct_addrs(&sender_endpoint).await;
        let repo = Arc::new(MemRepo::default());
        let target = DeviceId::new("target-beta");
        seed_addr(&repo, &target, &peer_addr).await;

        let adapter = IrohClipboardDispatchAdapter::new(sender_endpoint, repo);
        let payload = SyncPayload {
            ciphertext: Bytes::from(vec![0xAA; 16]),
        };

        let ack = adapter
            .dispatch(&target, &sample_header(), payload)
            .await
            .expect("dispatch succeeds");
        assert_eq!(ack, DispatchAck::DuplicateIgnored);

        peer_router.shutdown().await.expect("router shutdown");
    }

    /// Verdict 3 — missing peer_addr entry (peer never paired, or entry
    /// removed). Dispatch returns `Offline` without touching the network.
    #[tokio::test]
    async fn dispatch_returns_offline_when_peer_addr_missing() {
        let sender_endpoint = bind_endpoint().await;
        let repo = Arc::new(MemRepo::default());
        let adapter = IrohClipboardDispatchAdapter::new(sender_endpoint, repo);

        let result = adapter
            .dispatch(
                &DeviceId::new("never-paired"),
                &sample_header(),
                SyncPayload {
                    ciphertext: Bytes::from_static(b"irrelevant"),
                },
            )
            .await;
        match result {
            Err(ClipboardDispatchError::Offline) => {}
            other => panic!("expected Offline, got {other:?}"),
        }
    }

    /// Verdict 4 — oversized payload. The adapter short-circuits before
    /// even dialing, returning `LocalPolicyExceeded` with a message that
    /// mentions the wire MAX_PAYLOAD_SIZE. Protects against wasted QUIC
    /// handshake on a payload that would fail the wire boundary anyway,
    /// and uses the dedicated local-policy variant so callers don't
    /// misread this as a peer-side rejection.
    #[tokio::test]
    async fn dispatch_rejects_oversized_payload_locally_without_dialing() {
        // No peer_addr seeded — proves the rejection is local; otherwise
        // we would hit Offline first.
        let sender_endpoint = bind_endpoint().await;
        let repo = Arc::new(MemRepo::default());
        let adapter = IrohClipboardDispatchAdapter::new(sender_endpoint, repo);

        let oversized = vec![0u8; clipboard_wire::MAX_PAYLOAD_SIZE as usize + 1];
        let result = adapter
            .dispatch(
                &DeviceId::new("irrelevant"),
                &sample_header(),
                SyncPayload {
                    ciphertext: Bytes::from(oversized),
                },
            )
            .await;
        match result {
            Err(ClipboardDispatchError::LocalPolicyExceeded(msg)) => {
                assert!(
                    msg.contains("exceeds wire MAX_PAYLOAD_SIZE"),
                    "unexpected reject msg: {msg}"
                );
            }
            other => panic!("expected LocalPolicyExceeded, got {other:?}"),
        }
    }
}
