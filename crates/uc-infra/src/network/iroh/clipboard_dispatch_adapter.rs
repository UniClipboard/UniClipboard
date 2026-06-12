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

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use iroh::endpoint::Connection;
use iroh::{Endpoint, EndpointAddr};
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, instrument, warn};

use uc_core::ids::DeviceId;
use uc_core::ports::{
    ClipboardDispatchError, ClipboardDispatchPort, ClipboardHeader, DispatchAck,
    PeerAddressRepositoryPort, PresencePort, SyncPayload,
};

use super::clipboard_wire::{self, AckCode, WireEncodeError};
use super::connect::connect_with_staggered_retry;

/// ALPN identifier for the Slice 2 clipboard sync protocol. Independent of
/// the presence / pairing ALPNs so the Router can multiplex all three
/// transports on the same endpoint.
pub const CLIPBOARD_ALPN: &[u8] = b"uniclipboard/clipboard/0";

/// Result a single in-flight dial broadcasts to every follower waiting
/// on the same peer's single-flight slot. `Connection: Clone` makes
/// fan-out cheap (each follower gets its own handle on the same QUIC
/// connection); the failure branch carries the joined attempt errors so
/// followers' tracing surfaces the same root cause the leader observed.
type DialResult = Result<Connection, String>;

pub struct IrohClipboardDispatchAdapter {
    endpoint: Arc<Endpoint>,
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    /// PresencePort handle the adapter notifies on dial failure. A dial
    /// that we have to fall back to `Offline` is first-hand evidence the
    /// peer is unreachable; feeding that signal back through
    /// [`PresencePort::mark_offline`] lets every other consumer of presence
    /// (roster view, fan-out skip logic) observe the truth without waiting
    /// for the keepalive worker's next probe cycle.
    presence: Arc<dyn PresencePort>,
    /// Single-flight slot per destination device. Concurrent dispatches to
    /// the same peer collapse to one `connect_with_staggered_retry`
    /// invocation: the first caller becomes the leader (records its
    /// sender in this map, runs the dial, broadcasts the verdict), every
    /// later caller becomes a follower (subscribes and waits). See
    /// [`Self::dial_single_flight`].
    ///
    /// Pre-#886 a storm of N concurrent copies against the same offline
    /// peer kicked off N parallel staggered-retry loops (3·N raw `iroh
    /// connect` attempts) and N `mark_offline` calls; single-flight cuts
    /// both to 1.
    in_flight_dials: Mutex<HashMap<DeviceId, broadcast::Sender<DialResult>>>,
}

impl IrohClipboardDispatchAdapter {
    pub fn new(
        endpoint: Arc<Endpoint>,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
        presence: Arc<dyn PresencePort>,
    ) -> Self {
        Self {
            endpoint,
            peer_addr_repo,
            presence,
            in_flight_dials: Mutex::new(HashMap::new()),
        }
    }

    /// Dial `target` with at most one staggered-retry batch in flight per
    /// device. The first caller becomes the leader: it inserts a
    /// `broadcast::Sender` into [`Self::in_flight_dials`], drives the
    /// actual `connect_with_staggered_retry`, and broadcasts the verdict.
    /// Concurrent callers for the same device become followers and await
    /// the broadcast.
    ///
    /// On dial failure only the leader calls
    /// [`PresencePort::mark_offline`]; followers inherit the verdict
    /// through the broadcast. This is what shrinks the storm metric in
    /// #886's acceptance table: N concurrent copies against an offline
    /// peer collapse from N·3 raw `iroh connect` attempts and N
    /// `mark_offline` calls to 3 and 1 respectively.
    async fn dial_single_flight(&self, target: &DeviceId, addr: EndpointAddr) -> DialResult {
        enum Role {
            Leader(broadcast::Sender<DialResult>),
            Follower(broadcast::Receiver<DialResult>),
        }

        // Capacity 8 is well above the expected fan-out (per-peer roster
        // size ≤ 10 in Slice 2; concurrent dispatches per peer realistically
        // ≤ 3) — broadcast slow-receiver lag is not in the failure modes
        // this slot needs to defend against.
        let role = {
            let mut map = self.in_flight_dials.lock().await;
            match map.get(target) {
                Some(tx) => Role::Follower(tx.subscribe()),
                None => {
                    let (tx, _) = broadcast::channel::<DialResult>(8);
                    map.insert(target.clone(), tx.clone());
                    Role::Leader(tx)
                }
            }
        };

        match role {
            Role::Leader(tx) => {
                let result = connect_with_staggered_retry(
                    Arc::clone(&self.endpoint),
                    addr,
                    CLIPBOARD_ALPN,
                    "clipboard",
                )
                .await;

                // First-hand dial verdict — fold mark_offline into the
                // leader's tail so concurrent followers piling on the
                // same dead peer collapse to a single side-effect rather
                // than each calling `presence.mark_offline` on their own
                // failure return.
                if let Err(ref err) = result {
                    debug!(
                        error = %err,
                        "clipboard dispatch: single-flight dial failed; marking offline"
                    );
                    self.presence.mark_offline(target).await;
                }

                // Remove the slot before broadcasting so any caller that
                // races back in after we send sees an empty map and
                // starts a fresh dial cycle.
                {
                    let mut map = self.in_flight_dials.lock().await;
                    map.remove(target);
                }
                let _ = tx.send(result.clone());
                result
            }
            Role::Follower(mut rx) => match rx.recv().await {
                Ok(result) => result,
                Err(err) => Err(format!(
                    "single-flight follower lost leader broadcast: {err}"
                )),
            },
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

        // 3. Dial via the per-peer single-flight slot so a concurrent
        //    dispatch storm against the same offline peer collapses to one
        //    staggered-retry batch + one `mark_offline`. Dial failure =
        //    offline (no typed iroh error leaks up); the leader has
        //    already fed the verdict to PresencePort so this branch only
        //    has to surface the public error.
        let connection = match self.dial_single_flight(target, addr).await {
            Ok(connection) => connection,
            Err(err) => {
                debug!(
                    error = %err,
                    "clipboard dispatch: dial failed (single-flight), treating as Offline"
                );
                return Err(ClipboardDispatchError::Offline);
            }
        };

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
    use tokio::sync::broadcast;

    use uc_core::ports::{
        PeerAddressError, PeerAddressRecord, PresenceError, PresenceEvent, ReachabilityState,
    };

    // PresencePort mock for the dispatch tests. None of the four tests in
    // this module reach the dial-failure path (happy ack, duplicate ack,
    // missing peer_addr short-circuit, oversized local reject), so the
    // adapter never invokes `mark_offline`. The mock therefore needs no
    // expectations — any accidental call surfaces as a mockall panic, which
    // is exactly the regression guard we want for "dispatch tests shouldn't
    // be touching presence state."
    //
    // `mark_offline` is omitted intentionally: it has a default impl on the
    // trait (noop), so leaving it off the mock keeps that default in play
    // without forcing every test to wire an empty expectation.
    mockall::mock! {
        Presence {}

        #[async_trait]
        impl PresencePort for Presence {
            async fn ensure_reachable(
                &self,
                device: &DeviceId,
            ) -> Result<ReachabilityState, PresenceError>;
            async fn current_state(&self, device: &DeviceId) -> ReachabilityState;
            fn subscribe(&self) -> broadcast::Receiver<PresenceEvent>;
        }
    }

    fn presence_mock() -> Arc<dyn PresencePort> {
        Arc::new(MockPresence::new())
    }

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
            flow_id: None,
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

        let adapter = IrohClipboardDispatchAdapter::new(sender_endpoint, repo, presence_mock());
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

        let adapter = IrohClipboardDispatchAdapter::new(sender_endpoint, repo, presence_mock());
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
        let adapter = IrohClipboardDispatchAdapter::new(sender_endpoint, repo, presence_mock());

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

    /// Presence fake that counts `mark_offline` calls instead of asserting
    /// they never happen. Reserved for the single-flight tests below
    /// (verdicts 5 and 6) — the happy-path tests above keep using
    /// `presence_mock()` so any accidental call still panics.
    struct CountingPresence {
        mark_offline_calls: Arc<std::sync::atomic::AtomicUsize>,
        events: broadcast::Sender<PresenceEvent>,
    }

    impl CountingPresence {
        fn new() -> Self {
            let (events, _) = broadcast::channel(8);
            Self {
                mark_offline_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                events,
            }
        }
    }

    #[async_trait]
    impl PresencePort for CountingPresence {
        async fn ensure_reachable(
            &self,
            _device: &DeviceId,
        ) -> Result<ReachabilityState, PresenceError> {
            Ok(ReachabilityState::Unknown)
        }
        async fn current_state(&self, _device: &DeviceId) -> ReachabilityState {
            ReachabilityState::Unknown
        }
        async fn mark_offline(&self, _device: &DeviceId) {
            self.mark_offline_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        fn subscribe(&self) -> broadcast::Receiver<PresenceEvent> {
            self.events.subscribe()
        }
    }

    /// Verdict 5 — single-flight collapse. Two dispatches racing against
    /// the same offline peer must produce exactly one `mark_offline` call
    /// (the leader's) — followers inherit the verdict through the
    /// broadcast and surface their own `Offline` without re-stamping
    /// presence. This is the storm-axis half of #886's acceptance table.
    #[tokio::test]
    async fn concurrent_dispatch_to_offline_peer_calls_mark_offline_once() {
        let sender_endpoint = bind_endpoint().await;
        wait_for_direct_addrs(&sender_endpoint).await;

        // Seed an unroutable addr blob: a fresh random EndpointId with
        // no transport addrs and relay disabled on the sender, so the
        // staggered retry exhausts its three attempts without ever
        // dialling a live peer.
        let dead_id = iroh::SecretKey::generate().public();
        let dead_addr = EndpointAddr::new(dead_id);

        let target = DeviceId::new("offline-storm-target");
        let repo = Arc::new(MemRepo::default());
        seed_addr(&repo, &target, &dead_addr).await;

        let presence = Arc::new(CountingPresence::new());
        let mark_offline_calls = Arc::clone(&presence.mark_offline_calls);

        let adapter = Arc::new(IrohClipboardDispatchAdapter::new(
            sender_endpoint.clone(),
            repo,
            presence,
        ));

        let header = sample_header();
        let payload_a = SyncPayload {
            ciphertext: Bytes::from_static(b"first"),
        };
        let payload_b = SyncPayload {
            ciphertext: Bytes::from_static(b"second"),
        };

        let adapter_a = Arc::clone(&adapter);
        let adapter_b = Arc::clone(&adapter);
        let target_a = target.clone();
        let target_b = target.clone();
        let header_a = header.clone();
        let header_b = header.clone();

        let (result_a, result_b) = tokio::join!(
            async move { adapter_a.dispatch(&target_a, &header_a, payload_a).await },
            async move { adapter_b.dispatch(&target_b, &header_b, payload_b).await },
        );

        match (result_a, result_b) {
            (Err(ClipboardDispatchError::Offline), Err(ClipboardDispatchError::Offline)) => {}
            other => panic!("both dispatches should report Offline; got {other:?}"),
        }

        let calls = mark_offline_calls.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            calls, 1,
            "single-flight must collapse mark_offline to one call; got {calls}",
        );

        // Slot must be empty after the leader finishes so a later
        // dispatch starts a fresh dial cycle (no stale in-flight entry).
        assert!(
            adapter.in_flight_dials.lock().await.is_empty(),
            "in_flight_dials slot must be released after the leader broadcasts",
        );
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
        let adapter = IrohClipboardDispatchAdapter::new(sender_endpoint, repo, presence_mock());

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
