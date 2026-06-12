//! Iroh-backed implementation of [`ClipboardReceiverPort`] (Slice 2 Phase 2).
//!
//! The adapter publishes an [`InboundClipboard`] broadcast stream that
//! ingest use cases subscribe to. Actual inbound connections are handled by
//! [`IrohClipboardReceiverHandler`] — the same `ProtocolHandler` split
//! pattern we use for pairing / presence (see `uc-infra/AGENTS.md` §4.3):
//! adapter owns the broadcast `Sender` and the domain dependencies, the
//! handler is a cheap `Clone` that iroh's `Router` registers under
//! [`CLIPBOARD_ALPN`](super::clipboard_dispatch_adapter::CLIPBOARD_ALPN).
//!
//! ## Identity resolution
//!
//! Each inbound connection's `Connection::remote_id()` is the peer's iroh
//! `EndpointId`, which is an `iroh_base::PublicKey` newtype over the
//! 32-byte Ed25519 public key. The receiver feeds those bytes into the
//! same [`IdentityFingerprintFactoryPort`] that `IrohIdentityStore` uses
//! when persisting the local fingerprint, recovers the remote's
//! `IdentityFingerprint`, and looks it up in [`MemberRepositoryPort`].
//! This invariant was established by the T2 probe
//! (`tests/iroh_clipboard_identity_probe.rs`) — no new port method is
//! needed and no `EndpointId` type leaks above the adapter.
//!
//! Unknown peers (fingerprint not in `member_repo`) receive
//! [`AckCode::Rejected`] and the connection is closed. They never make it
//! to the broadcast stream, so `IngestInboundClipboardUseCase` does not
//! need a second rejection path.
//!
//! ## Failure semantics
//!
//! The handler is best-effort per connection. Every failure path is logged
//! and the connection closes; the port-level contract is "lagging
//! subscribers recover via the next dispatch", so dropping an occasional
//! inbound frame is never fatal. That keeps accept-loop code free of
//! retry logic.

use std::sync::Arc;

use async_trait::async_trait;
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use tokio::sync::broadcast;
use tracing::{debug, instrument, warn};

use uc_core::ids::DeviceId;
use uc_core::membership::MemberRepositoryPort;
use uc_core::ports::security::IdentityFingerprintFactoryPort;
use uc_core::ports::{ClipboardReceiverPort, InboundClipboard};
use uc_core::security::IdentityFingerprint;

use super::clipboard_wire::{self, AckCode};

/// Capacity of the `InboundClipboard` broadcast channel. Matches the
/// presence adapter (`PRESENCE_EVENT_CHANNEL_CAPACITY`) so both streams
/// share the same burst tolerance. Lagging subscribers drop frames per
/// broadcast semantics — they recover on the peer's next dispatch.
const INBOUND_CHANNEL_CAPACITY: usize = 64;

/// Receiver-side adapter. Holds the broadcast sender and domain
/// dependencies needed to resolve `endpoint_id → DeviceId`. See
/// [`IrohClipboardReceiverAdapter::handler`] for the paired
/// `ProtocolHandler`.
pub struct IrohClipboardReceiverAdapter {
    event_tx: broadcast::Sender<InboundClipboard>,
    handler_state: Arc<HandlerState>,
}

struct HandlerState {
    member_repo: Arc<dyn MemberRepositoryPort>,
    fingerprint_factory: Arc<dyn IdentityFingerprintFactoryPort>,
    event_tx: broadcast::Sender<InboundClipboard>,
}

impl IrohClipboardReceiverAdapter {
    pub fn new(
        member_repo: Arc<dyn MemberRepositoryPort>,
        fingerprint_factory: Arc<dyn IdentityFingerprintFactoryPort>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(INBOUND_CHANNEL_CAPACITY);
        let handler_state = Arc::new(HandlerState {
            member_repo,
            fingerprint_factory,
            event_tx: event_tx.clone(),
        });
        Self {
            event_tx,
            handler_state,
        }
    }

    /// Cheap clone-able handle registered with iroh's `RouterBuilder`.
    /// Each inbound connection runs [`IrohClipboardReceiverHandler::accept`],
    /// which shares the adapter's broadcast `Sender` via `Arc`.
    pub fn handler(&self) -> IrohClipboardReceiverHandler {
        IrohClipboardReceiverHandler {
            state: Arc::clone(&self.handler_state),
        }
    }
}

#[async_trait]
impl ClipboardReceiverPort for IrohClipboardReceiverAdapter {
    fn subscribe(&self) -> broadcast::Receiver<InboundClipboard> {
        self.event_tx.subscribe()
    }
}

// ============================================================================
// ProtocolHandler (accept side)
// ============================================================================

#[derive(Clone)]
pub struct IrohClipboardReceiverHandler {
    state: Arc<HandlerState>,
}

impl std::fmt::Debug for IrohClipboardReceiverHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IrohClipboardReceiverHandler")
            .finish_non_exhaustive()
    }
}

impl ProtocolHandler for IrohClipboardReceiverHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let remote = connection.remote_id();
        let remote_bytes: [u8; 32] = *remote.as_bytes();

        // 1. Resolve the remote endpoint's public key back to a known
        //    SpaceMember.
        let resolved = self.state.resolve_device(&remote_bytes).await;

        // 2. Open the bi-stream even for rejected peers — we still want to
        //    send a `Rejected` ack so the sender does not stall on its
        //    `recv.read_exact` waiting for a byte that never comes.
        let (mut send, mut recv) = match connection.accept_bi().await {
            Ok(pair) => pair,
            Err(err) => {
                warn!(error = %err, "clipboard receiver: accept_bi failed; dropping connection");
                return Ok(());
            }
        };

        let Some(peer_device_id) = resolved else {
            warn!(
                remote = %remote,
                "clipboard receiver: unknown peer fingerprint; sending Rejected ack"
            );
            emit_ack(&mut send, AckCode::Rejected).await;
            // Keep the connection alive until the peer closes it so the
            // ack byte has time to flush before the QUIC connection gets
            // torn down (otherwise the sender sees
            // `ConnectionLost(ApplicationClosed)` instead of the ack).
            let _ = connection.closed().await;
            return Ok(());
        };

        // 3. Read the frame. Any codec-level failure ends the connection
        //    with a `Rejected` ack so the sender gets a typed
        //    `PeerRejected` error rather than an unexplained `Io`.
        let frame = match clipboard_wire::read_frame(&mut recv).await {
            Ok(f) => f,
            Err(err) => {
                warn!(
                    error = %err,
                    peer = %peer_device_id.as_str(),
                    "clipboard receiver: frame decode failed; sending Rejected ack"
                );
                emit_ack(&mut send, AckCode::Rejected).await;
                let _ = connection.closed().await;
                return Ok(());
            }
        };

        // 4. Broadcast to subscribers. A `SendError` here means no
        //    subscriber is attached; dropping the frame is acceptable per
        //    the port contract (section header above). We still send
        //    `Accepted` because the sender side did its job; application
        //    consumer responsibility is to subscribe before F1 completes.
        let inbound = InboundClipboard {
            peer_device_id: peer_device_id.clone(),
            header: frame.header,
            ciphertext: frame.ciphertext,
        };
        if self.state.event_tx.send(inbound).is_err() {
            debug!(
                peer = %peer_device_id.as_str(),
                "clipboard receiver: no subscribers attached; inbound frame dropped"
            );
        }

        // 5. Ack accepted; hold the connection open until the peer closes
        //    it so the ack byte has time to flush. The sender side drops
        //    the connection after reading the ack, which resolves
        //    `Connection::closed()` here and lets the handler return.
        emit_ack(&mut send, AckCode::Accepted).await;
        let _ = connection.closed().await;
        Ok(())
    }
}

/// Write a one-byte ack + finish the send half. Failures are logged but
/// swallowed: the connection is about to close either way, and the port
/// contract documents that adapter-level frame failures are best-effort.
#[instrument(skip(send))]
async fn emit_ack(send: &mut iroh::endpoint::SendStream, ack: AckCode) {
    if let Err(err) = send.write_all(&[ack.as_byte()]).await {
        debug!(error = %err, "clipboard receiver: ack write failed");
        return;
    }
    if let Err(err) = send.finish() {
        debug!(error = %err, "clipboard receiver: send.finish failed");
    }
}

impl HandlerState {
    /// Look up a `SpaceMember` whose `identity_fingerprint` equals the one
    /// derived from `remote_pubkey_bytes`. Returns `None` when the peer is
    /// unknown or when repository errors (logged).
    ///
    /// `member_repo.list()` is used because the port does not expose
    /// lookup-by-fingerprint and the roster size is bounded (Slice 2
    /// assumption N ≤ 10). Adding a dedicated index is a Phase 3 concern.
    async fn resolve_device(&self, remote_pubkey_bytes: &[u8; 32]) -> Option<DeviceId> {
        let derived = match self
            .fingerprint_factory
            .from_public_key(remote_pubkey_bytes)
        {
            Ok(fp) => fp,
            Err(err) => {
                warn!(
                    error = %err,
                    "clipboard receiver: fingerprint derivation failed — cannot resolve peer"
                );
                return None;
            }
        };

        let members = match self.member_repo.list().await {
            Ok(ms) => ms,
            Err(err) => {
                warn!(
                    error = %err,
                    "clipboard receiver: member_repo.list failed; treating peer as unknown"
                );
                return None;
            }
        };

        members
            .into_iter()
            .find(|m| fingerprints_equal(&m.identity_fingerprint, &derived))
            .map(|m| m.device_id)
    }
}

/// `IdentityFingerprint` does not derive `PartialEq` on its raw form in
/// every version of `uc-core`; use the display form which is the stable
/// canonical comparison surface (`ABCD-EFGH-IJKL-MNOP`).
fn fingerprints_equal(a: &IdentityFingerprint, b: &IdentityFingerprint) -> bool {
    a == b
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use bytes::Bytes;
    use chrono::Utc;
    use iroh::protocol::Router;
    use iroh::{Endpoint, RelayMode, SecretKey};
    use tokio::sync::Mutex;

    use uc_core::membership::{MembershipError, SpaceMember};
    use uc_core::ports::{
        ClipboardDispatchPort, ClipboardHeader, PresenceError, PresenceEvent, PresencePort,
        ReachabilityState, SyncPayload,
    };
    use uc_core::MemberSyncPreferences;

    use crate::network::iroh::clipboard_dispatch_adapter::{
        IrohClipboardDispatchAdapter, CLIPBOARD_ALPN,
    };
    use crate::security::Sha256IdentityFingerprintFactory;
    use uc_core::ports::{PeerAddressError, PeerAddressRecord, PeerAddressRepositoryPort};

    // PresencePort mock. Receiver-side tests use the dispatch adapter only
    // as a wire-driver against the receiver under test; the dispatch path
    // never hits dial failure in these scenarios, so `mark_offline` is
    // never invoked. The mock therefore needs no expectations — any
    // unexpected call surfaces as a mockall panic.
    //
    // `mark_offline` is intentionally omitted from the mock so the trait's
    // default noop impl is in play; if a future test does exercise the
    // dial-failure path, mockall will catch any accidental presence-side
    // assumption it makes.
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

    // ----- test doubles ------------------------------------------------------

    #[derive(Default)]
    struct MemMemberRepo {
        inner: Mutex<HashMap<String, SpaceMember>>,
    }
    #[async_trait]
    impl MemberRepositoryPort for MemMemberRepo {
        async fn get(&self, device_id: &DeviceId) -> Result<Option<SpaceMember>, MembershipError> {
            Ok(self.inner.lock().await.get(device_id.as_str()).cloned())
        }
        async fn list(&self) -> Result<Vec<SpaceMember>, MembershipError> {
            Ok(self.inner.lock().await.values().cloned().collect())
        }
        async fn save(&self, member: &SpaceMember) -> Result<(), MembershipError> {
            self.inner
                .lock()
                .await
                .insert(member.device_id.as_str().to_string(), member.clone());
            Ok(())
        }
        async fn remove(&self, device_id: &DeviceId) -> Result<bool, MembershipError> {
            Ok(self.inner.lock().await.remove(device_id.as_str()).is_some())
        }
    }

    #[derive(Default)]
    struct MemPeerAddrRepo {
        inner: Mutex<HashMap<String, PeerAddressRecord>>,
    }
    #[async_trait]
    impl PeerAddressRepositoryPort for MemPeerAddrRepo {
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

    // ----- harness -----------------------------------------------------------

    async fn bind_endpoint_with(seed: [u8; 32]) -> Arc<Endpoint> {
        Arc::new(
            Endpoint::builder(iroh::endpoint::presets::N0)
                .secret_key(SecretKey::from_bytes(&seed))
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

    fn make_member(seed: [u8; 32], device_id: &str) -> SpaceMember {
        let factory = Sha256IdentityFingerprintFactory;
        let sk = SecretKey::from_bytes(&seed);
        let fp = factory
            .from_public_key(sk.public().as_bytes())
            .expect("derive fingerprint for test member");
        SpaceMember {
            device_id: DeviceId::new(device_id),
            device_name: "Test Device".to_string(),
            identity_fingerprint: fp,
            joined_at: Utc::now(),
            sync_preferences: MemberSyncPreferences::default(),
        }
    }

    fn sample_header() -> ClipboardHeader {
        ClipboardHeader {
            version: ClipboardHeader::CURRENT_VERSION,
            content_hash: "9".repeat(64),
            captured_at_ms: 1_700_000_000_000,
            origin_device_id: "sender-001".to_string(),
            origin_device_name: "Alice".to_string(),
            payload_version: 3,
            flow_id: None,
        }
    }

    /// Spin up a receiver router around a fresh adapter and return the
    /// handles the test needs: the router (to shut down), the receiver's
    /// endpoint addr (for the dialer to connect to), and a subscriber to
    /// the inbound stream.
    struct ReceiverHarness {
        receiver_endpoint: Arc<Endpoint>,
        receiver_router: Router,
        inbound_rx: broadcast::Receiver<InboundClipboard>,
    }

    async fn spawn_receiver(
        seed: [u8; 32],
        member_repo: Arc<dyn MemberRepositoryPort>,
    ) -> ReceiverHarness {
        let receiver_endpoint = bind_endpoint_with(seed).await;
        wait_for_direct_addrs(&receiver_endpoint).await;

        let adapter = IrohClipboardReceiverAdapter::new(
            member_repo,
            Arc::new(Sha256IdentityFingerprintFactory),
        );
        let inbound_rx = adapter.subscribe();
        let router = Router::builder((*receiver_endpoint).clone())
            .accept(CLIPBOARD_ALPN, adapter.handler())
            .spawn();

        ReceiverHarness {
            receiver_endpoint,
            receiver_router: router,
            inbound_rx,
        }
    }

    // ----- verdicts ----------------------------------------------------------

    /// Verdict 1 — single frame delivered. Sender is in member_repo, frame
    /// reaches the broadcast stream with ciphertext + header intact, and
    /// the sender sees `Accepted`.
    #[tokio::test]
    async fn accepts_single_inbound_frame_from_known_peer() {
        let sender_seed = [0x11u8; 32];
        let receiver_seed = [0x22u8; 32];

        let member_repo: Arc<dyn MemberRepositoryPort> = Arc::new(MemMemberRepo::default());
        let sender_member = make_member(sender_seed, "sender-a");
        member_repo.save(&sender_member).await.unwrap();

        let harness = spawn_receiver(receiver_seed, Arc::clone(&member_repo)).await;
        let receiver_addr = harness.receiver_endpoint.addr();

        // Build the sender side.
        let sender_endpoint = bind_endpoint_with(sender_seed).await;
        wait_for_direct_addrs(&sender_endpoint).await;
        let peer_addr_repo = Arc::new(MemPeerAddrRepo::default());
        peer_addr_repo
            .upsert(&PeerAddressRecord {
                device_id: DeviceId::new("receiver-b"),
                addr_blob: postcard::to_stdvec(&receiver_addr)
                    .expect("postcard encode EndpointAddr"),
                observed_at: Utc::now(),
            })
            .await
            .unwrap();
        let dispatch =
            IrohClipboardDispatchAdapter::new(sender_endpoint, peer_addr_repo, presence_mock());

        let payload = Bytes::from(vec![0xAB; 128]);
        let ack = dispatch
            .dispatch(
                &DeviceId::new("receiver-b"),
                &sample_header(),
                SyncPayload {
                    ciphertext: payload.clone(),
                },
            )
            .await
            .expect("dispatch succeeds");
        assert_eq!(ack, uc_core::ports::DispatchAck::Accepted);

        let mut inbound_rx = harness.inbound_rx;
        let inbound = tokio::time::timeout(Duration::from_secs(3), inbound_rx.recv())
            .await
            .expect("broadcast arrives within timeout")
            .expect("subscriber sees the frame");

        assert_eq!(inbound.peer_device_id.as_str(), "sender-a");
        assert_eq!(inbound.header, sample_header());
        assert_eq!(inbound.ciphertext, payload);

        harness.receiver_router.shutdown().await.ok();
    }

    /// Verdict 2 — unknown peer. The receiver emits `Rejected` and does
    /// not push anything into the broadcast stream.
    #[tokio::test]
    async fn rejects_unknown_peer_with_ack_rejected() {
        let sender_seed = [0x33u8; 32];
        let receiver_seed = [0x44u8; 32];

        // member_repo is empty — sender's fingerprint is unknown.
        let member_repo: Arc<dyn MemberRepositoryPort> = Arc::new(MemMemberRepo::default());
        let harness = spawn_receiver(receiver_seed, Arc::clone(&member_repo)).await;
        let receiver_addr = harness.receiver_endpoint.addr();

        let sender_endpoint = bind_endpoint_with(sender_seed).await;
        wait_for_direct_addrs(&sender_endpoint).await;
        let peer_addr_repo = Arc::new(MemPeerAddrRepo::default());
        peer_addr_repo
            .upsert(&PeerAddressRecord {
                device_id: DeviceId::new("receiver-c"),
                addr_blob: postcard::to_stdvec(&receiver_addr).unwrap(),
                observed_at: Utc::now(),
            })
            .await
            .unwrap();
        let dispatch =
            IrohClipboardDispatchAdapter::new(sender_endpoint, peer_addr_repo, presence_mock());

        let result = dispatch
            .dispatch(
                &DeviceId::new("receiver-c"),
                &sample_header(),
                SyncPayload {
                    ciphertext: Bytes::from_static(b"irrelevant"),
                },
            )
            .await;

        match result {
            Err(uc_core::ports::ClipboardDispatchError::PeerRejected(msg)) => {
                assert!(
                    msg.contains("Rejected ack"),
                    "unexpected reject message: {msg}"
                );
            }
            other => panic!("expected PeerRejected, got {other:?}"),
        }

        // Subscriber never receives anything.
        let mut inbound_rx = harness.inbound_rx;
        let fast_poll = tokio::time::timeout(Duration::from_millis(200), inbound_rx.recv()).await;
        assert!(fast_poll.is_err(), "unknown peer must not publish");

        harness.receiver_router.shutdown().await.ok();
    }

    /// Verdict 3 — bad magic byte from a malicious / out-of-protocol peer.
    /// The handler rejects the frame with `Rejected` ack and drops the
    /// connection without touching the broadcast stream. Uses a raw
    /// sender that writes garbage bytes to ensure the handler tolerates
    /// misbehaved peers even if they otherwise pass identity resolution.
    #[tokio::test]
    async fn rejects_bad_magic_without_touching_broadcast_stream() {
        let sender_seed = [0x55u8; 32];
        let receiver_seed = [0x66u8; 32];

        let member_repo: Arc<dyn MemberRepositoryPort> = Arc::new(MemMemberRepo::default());
        let sender_member = make_member(sender_seed, "known-bad-actor");
        member_repo.save(&sender_member).await.unwrap();

        let harness = spawn_receiver(receiver_seed, Arc::clone(&member_repo)).await;
        let receiver_addr = harness.receiver_endpoint.addr();

        // Raw-ish sender: bypass the dispatch adapter so we can write
        // wrong bytes on purpose.
        let sender_endpoint = bind_endpoint_with(sender_seed).await;
        wait_for_direct_addrs(&sender_endpoint).await;
        let conn = sender_endpoint
            .connect(receiver_addr, CLIPBOARD_ALPN)
            .await
            .expect("dial receiver");
        let (mut send, mut recv) = conn.open_bi().await.expect("open_bi");
        // Bad magic + garbage; receiver should bail out at the magic check.
        send.write_all(&[0x00, 0x00, 0x00, 0x00, 0x05])
            .await
            .expect("write garbage");
        send.finish().expect("finish");

        // Receiver must respond with Rejected, not hang.
        let mut ack_buf = [0u8; 1];
        recv.read_exact(&mut ack_buf).await.expect("ack read");
        assert_eq!(ack_buf[0], AckCode::Rejected.as_byte());

        let mut inbound_rx = harness.inbound_rx;
        let fast_poll = tokio::time::timeout(Duration::from_millis(200), inbound_rx.recv()).await;
        assert!(
            fast_poll.is_err(),
            "bad magic must not publish to broadcast"
        );

        harness.receiver_router.shutdown().await.ok();
    }

    /// Verdict 4 — multiple concurrent inbound connections are processed
    /// in parallel, and every frame shows up on the broadcast stream.
    /// This is the guard against a handler that accidentally serializes
    /// connections (e.g. by holding a lock across the whole `accept`
    /// body).
    #[tokio::test]
    async fn processes_concurrent_inbound_connections() {
        let receiver_seed = [0x77u8; 32];
        let member_repo: Arc<dyn MemberRepositoryPort> = Arc::new(MemMemberRepo::default());

        // Three senders with distinct identities.
        let sender_seeds: [[u8; 32]; 3] = [[0xA0; 32], [0xA1; 32], [0xA2; 32]];
        for (i, seed) in sender_seeds.iter().enumerate() {
            let member = make_member(*seed, &format!("sender-{i}"));
            member_repo.save(&member).await.unwrap();
        }

        let harness = spawn_receiver(receiver_seed, Arc::clone(&member_repo)).await;
        let receiver_addr = harness.receiver_endpoint.addr();

        let mut tasks = Vec::new();
        for (i, seed) in sender_seeds.iter().enumerate() {
            let addr = receiver_addr.clone();
            let seed = *seed;
            tasks.push(tokio::spawn(async move {
                let sender_endpoint = bind_endpoint_with(seed).await;
                wait_for_direct_addrs(&sender_endpoint).await;
                let peer_addr_repo = Arc::new(MemPeerAddrRepo::default());
                peer_addr_repo
                    .upsert(&PeerAddressRecord {
                        device_id: DeviceId::new("rcv"),
                        addr_blob: postcard::to_stdvec(&addr).unwrap(),
                        observed_at: Utc::now(),
                    })
                    .await
                    .unwrap();
                let dispatch = IrohClipboardDispatchAdapter::new(
                    sender_endpoint,
                    peer_addr_repo,
                    presence_mock(),
                );

                let mut header = sample_header();
                header.origin_device_id = format!("sender-{i}");
                let payload = Bytes::from(vec![i as u8; 32]);
                let ack = dispatch
                    .dispatch(
                        &DeviceId::new("rcv"),
                        &header,
                        SyncPayload {
                            ciphertext: payload.clone(),
                        },
                    )
                    .await
                    .expect("concurrent dispatch ok");
                assert_eq!(ack, uc_core::ports::DispatchAck::Accepted);
            }));
        }

        for task in tasks {
            task.await.expect("sender task");
        }

        // Collect three inbound frames; order is not asserted (concurrency).
        let mut inbound_rx = harness.inbound_rx;
        let mut seen: Vec<String> = Vec::new();
        for _ in 0..3 {
            let inbound = tokio::time::timeout(Duration::from_secs(3), inbound_rx.recv())
                .await
                .expect("broadcast arrives in time")
                .expect("subscriber sees frame");
            seen.push(inbound.peer_device_id.as_str().to_string());
        }
        seen.sort();
        assert_eq!(seen, vec!["sender-0", "sender-1", "sender-2"]);

        harness.receiver_router.shutdown().await.ok();
    }
}
