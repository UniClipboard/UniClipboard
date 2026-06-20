//! Iroh-backed implementation of [`ActiveClipboardReceiverPort`].
//!
//! The adapter publishes an [`InboundActiveClipboardState`] broadcast stream
//! that the application layer subscribes to. Inbound connections are handled
//! by [`IrohActiveClipboardReceiverHandler`] — the same `ProtocolHandler`
//! split pattern used for the bulk clipboard / presence / pairing transports
//! (see `uc-infra/AGENTS.md` §4.3): the adapter owns the broadcast `Sender`
//! and the domain dependencies, and the handler is a cheap `Clone` that
//! iroh's `Router` registers under [`ACTIVE_CLIPBOARD_ALPN`].
//!
//! ## Identity resolution
//!
//! Each inbound connection's `Connection::remote_id()` is the peer's iroh
//! `EndpointId` (a newtype over the 32-byte Ed25519 public key). The receiver
//! feeds those bytes into the same [`IdentityFingerprintFactoryPort`] used
//! when persisting the local fingerprint, recovers the remote's
//! `IdentityFingerprint`, and looks it up in [`MemberRepositoryPort`].
//! Unknown peers (fingerprint not in `member_repo`) are dropped without
//! reaching the broadcast stream.
//!
//! ## Why no ack
//!
//! Active-clipboard state is a fire-and-forget last-writer-wins broadcast:
//! the sender does not wait for a reply, so the handler reads one frame,
//! publishes it, and returns. There is no rejection ack (contrast the bulk
//! codec, where the sender blocks on a per-frame ack). Convergence is the
//! responsibility of the LWW register, not of per-frame acknowledgement.

use std::sync::Arc;

use async_trait::async_trait;
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use tokio::sync::broadcast;
use tracing::{debug, warn};

use uc_core::ids::DeviceId;
use uc_core::membership::MemberRepositoryPort;
use uc_core::ports::security::IdentityFingerprintFactoryPort;
use uc_core::ports::{ActiveClipboardReceiverPort, InboundActiveClipboardState};
use uc_core::security::IdentityFingerprint;

use super::wire;

/// ALPN identifier for the active-clipboard state protocol. An independent
/// sibling of the bulk clipboard / presence / pairing ALPNs so the Router can
/// multiplex every transport on the same endpoint.
pub const ACTIVE_CLIPBOARD_ALPN: &[u8] = b"uniclipboard/active-clipboard/0";

/// Capacity of the `InboundActiveClipboardState` broadcast channel. Matches
/// the bulk clipboard receiver so both inbound streams share the same burst
/// tolerance. Lagging subscribers drop frames per broadcast semantics — the
/// register is convergent, so a dropped observation is recovered by the next
/// one a peer reports.
const INBOUND_CHANNEL_CAPACITY: usize = 64;

/// Receiver-side adapter. Holds the broadcast sender and the domain
/// dependencies needed to resolve `endpoint_id → DeviceId`. See
/// [`IrohActiveClipboardReceiverAdapter::handler`] for the paired
/// `ProtocolHandler`.
pub struct IrohActiveClipboardReceiverAdapter {
    event_tx: broadcast::Sender<InboundActiveClipboardState>,
    handler_state: Arc<HandlerState>,
}

struct HandlerState {
    member_repo: Arc<dyn MemberRepositoryPort>,
    fingerprint_factory: Arc<dyn IdentityFingerprintFactoryPort>,
    event_tx: broadcast::Sender<InboundActiveClipboardState>,
}

impl IrohActiveClipboardReceiverAdapter {
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

    /// Cheap clone-able handle registered with iroh's `RouterBuilder`. Each
    /// inbound connection runs [`IrohActiveClipboardReceiverHandler::accept`],
    /// which shares the adapter's broadcast `Sender` via `Arc`.
    pub fn handler(&self) -> IrohActiveClipboardReceiverHandler {
        IrohActiveClipboardReceiverHandler {
            state: Arc::clone(&self.handler_state),
        }
    }
}

#[async_trait]
impl ActiveClipboardReceiverPort for IrohActiveClipboardReceiverAdapter {
    fn subscribe(&self) -> broadcast::Receiver<InboundActiveClipboardState> {
        self.event_tx.subscribe()
    }
}

// ============================================================================
// ProtocolHandler (accept side)
// ============================================================================

#[derive(Clone)]
pub struct IrohActiveClipboardReceiverHandler {
    state: Arc<HandlerState>,
}

impl std::fmt::Debug for IrohActiveClipboardReceiverHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IrohActiveClipboardReceiverHandler")
            .finish_non_exhaustive()
    }
}

impl ProtocolHandler for IrohActiveClipboardReceiverHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let remote = connection.remote_id();
        let remote_bytes: [u8; 32] = *remote.as_bytes();

        // 1. Resolve the remote endpoint's public key back to a known
        //    SpaceMember. Unknown peers never reach the broadcast stream.
        let Some(peer_device_id) = self.state.resolve_device(&remote_bytes).await else {
            warn!(
                remote = %remote,
                "active-clipboard receiver: unknown peer fingerprint; dropping connection"
            );
            // Fire-and-forget protocol: no ack to send, so simply let the
            // connection drop.
            return Ok(());
        };

        // 2. Accept the inbound stream. The sender opens a bi-stream and
        //    finishes its send half after the frame; we only read.
        let (_send, mut recv) = match connection.accept_bi().await {
            Ok(pair) => pair,
            Err(err) => {
                warn!(
                    error = %err,
                    peer = %peer_device_id.as_str(),
                    "active-clipboard receiver: accept_bi failed; dropping connection"
                );
                return Ok(());
            }
        };

        // 3. Read the single state frame. Any codec-level failure drops the
        //    connection — fire-and-forget means there is nothing to ack.
        let msg = match wire::read_frame(&mut recv).await {
            Ok(m) => m,
            Err(err) => {
                warn!(
                    error = %err,
                    peer = %peer_device_id.as_str(),
                    "active-clipboard receiver: frame decode failed; dropping connection"
                );
                return Ok(());
            }
        };

        // 4. Decode the activator device id off the wire. `try_new` rejects
        //    an over-long value instead of panicking the accept task — the
        //    field is untrusted peer input, so a malformed id drops the
        //    frame like any other codec failure.
        let Some(activated_by) = DeviceId::try_new(&msg.activated_by) else {
            warn!(
                peer = %peer_device_id.as_str(),
                "active-clipboard receiver: activated_by exceeds device id bound; dropping frame"
            );
            return Ok(());
        };

        // 5. Publish to subscribers. A `SendError` means no subscriber is
        //    attached; dropping the observation is acceptable — the register
        //    converges on the next observation a peer reports.
        let inbound = InboundActiveClipboardState {
            peer_device_id,
            snapshot_hash: msg.snapshot_hash,
            sender_entry_id: msg.entry_id,
            activated_at_ms: msg.activated_at_ms,
            activated_by,
        };
        if self.state.event_tx.send(inbound).is_err() {
            debug!(
                peer = %peer_device_id.as_str(),
                "active-clipboard receiver: no subscribers attached; observation dropped"
            );
        }

        Ok(())
    }
}

impl HandlerState {
    /// Look up a `SpaceMember` whose `identity_fingerprint` equals the one
    /// derived from `remote_pubkey_bytes`. Returns `None` when the peer is
    /// unknown or the repository errors (logged).
    ///
    /// `member_repo.list()` is used because the port does not expose
    /// lookup-by-fingerprint and the roster size is bounded (N ≤ 10); a
    /// dedicated index is a later concern, mirroring the bulk receiver.
    async fn resolve_device(&self, remote_pubkey_bytes: &[u8; 32]) -> Option<DeviceId> {
        let derived = match self
            .fingerprint_factory
            .from_public_key(remote_pubkey_bytes)
        {
            Ok(fp) => fp,
            Err(err) => {
                warn!(
                    error = %err,
                    "active-clipboard receiver: fingerprint derivation failed — cannot resolve peer"
                );
                return None;
            }
        };

        let members = match self.member_repo.list().await {
            Ok(ms) => ms,
            Err(err) => {
                warn!(
                    error = %err,
                    "active-clipboard receiver: member_repo.list failed; treating peer as unknown"
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
    use chrono::Utc;
    use iroh::protocol::Router;
    use iroh::{Endpoint, RelayMode, SecretKey};
    use tokio::sync::Mutex;

    use uc_core::membership::{MembershipError, SpaceMember};
    use uc_core::MemberSyncPreferences;

    use super::super::wire::{write_frame, ActiveClipboardWireMessage};
    use crate::security::Sha256IdentityFingerprintFactory;

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

    // ----- harness -----------------------------------------------------------

    async fn bind_endpoint_with(seed: [u8; 32]) -> Arc<Endpoint> {
        Arc::new(
            Endpoint::builder(iroh::endpoint::presets::N0)
                .secret_key(SecretKey::from_bytes(&seed))
                .alpns(vec![ACTIVE_CLIPBOARD_ALPN.to_vec()])
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

    fn sample_message() -> ActiveClipboardWireMessage {
        ActiveClipboardWireMessage {
            snapshot_hash: format!("blake3v1:{}", "9".repeat(64)),
            entry_id: "01941b00-0000-7000-8000-000000000001".to_string(),
            activated_at_ms: 1_700_000_000_000,
            activated_by: "sender-001".to_string(),
        }
    }

    struct ReceiverHarness {
        receiver_endpoint: Arc<Endpoint>,
        receiver_router: Router,
        inbound_rx: broadcast::Receiver<InboundActiveClipboardState>,
    }

    async fn spawn_receiver(
        seed: [u8; 32],
        member_repo: Arc<dyn MemberRepositoryPort>,
    ) -> ReceiverHarness {
        let receiver_endpoint = bind_endpoint_with(seed).await;
        wait_for_direct_addrs(&receiver_endpoint).await;

        let adapter = IrohActiveClipboardReceiverAdapter::new(
            member_repo,
            Arc::new(Sha256IdentityFingerprintFactory),
        );
        let inbound_rx = adapter.subscribe();
        let router = Router::builder((*receiver_endpoint).clone())
            .accept(ACTIVE_CLIPBOARD_ALPN, adapter.handler())
            .spawn();

        ReceiverHarness {
            receiver_endpoint,
            receiver_router: router,
            inbound_rx,
        }
    }

    /// Drive one raw frame at the receiver from a known sender endpoint.
    async fn send_one_frame(
        sender_seed: [u8; 32],
        receiver_addr: iroh::EndpointAddr,
        msg: &ActiveClipboardWireMessage,
    ) {
        let sender_endpoint = bind_endpoint_with(sender_seed).await;
        wait_for_direct_addrs(&sender_endpoint).await;
        let conn = sender_endpoint
            .connect(receiver_addr, ACTIVE_CLIPBOARD_ALPN)
            .await
            .expect("dial receiver");
        let (mut send, _recv) = conn.open_bi().await.expect("open_bi");
        write_frame(&mut send, msg).await.expect("write frame");
        send.finish().expect("finish");
        let _ = conn.closed().await;
    }

    // ----- verdicts ----------------------------------------------------------

    /// Verdict 1 — a known peer's state observation reaches the broadcast
    /// stream with every field intact, and the peer is attributed correctly.
    #[tokio::test]
    async fn accepts_state_from_known_peer() {
        let sender_seed = [0x11u8; 32];
        let receiver_seed = [0x22u8; 32];

        let member_repo: Arc<dyn MemberRepositoryPort> = Arc::new(MemMemberRepo::default());
        let sender_member = make_member(sender_seed, "sender-a");
        member_repo.save(&sender_member).await.unwrap();

        let harness = spawn_receiver(receiver_seed, Arc::clone(&member_repo)).await;
        let receiver_addr = harness.receiver_endpoint.addr();

        let msg = sample_message();
        send_one_frame(sender_seed, receiver_addr, &msg).await;

        let mut inbound_rx = harness.inbound_rx;
        let inbound = tokio::time::timeout(Duration::from_secs(3), inbound_rx.recv())
            .await
            .expect("broadcast arrives within timeout")
            .expect("subscriber sees the observation");

        assert_eq!(inbound.peer_device_id.as_str(), "sender-a");
        assert_eq!(inbound.snapshot_hash, msg.snapshot_hash);
        assert_eq!(inbound.sender_entry_id, msg.entry_id);
        assert_eq!(inbound.activated_at_ms, msg.activated_at_ms);
        assert_eq!(inbound.activated_by.as_str(), msg.activated_by);

        harness.receiver_router.shutdown().await.ok();
    }

    /// Verdict 2 — an unknown peer (fingerprint not in member_repo) never
    /// publishes to the broadcast stream.
    #[tokio::test]
    async fn rejects_unknown_peer_without_publishing() {
        let sender_seed = [0x33u8; 32];
        let receiver_seed = [0x44u8; 32];

        // member_repo is empty — sender's fingerprint is unknown.
        let member_repo: Arc<dyn MemberRepositoryPort> = Arc::new(MemMemberRepo::default());
        let harness = spawn_receiver(receiver_seed, Arc::clone(&member_repo)).await;
        let receiver_addr = harness.receiver_endpoint.addr();

        send_one_frame(sender_seed, receiver_addr, &sample_message()).await;

        let mut inbound_rx = harness.inbound_rx;
        let fast_poll = tokio::time::timeout(Duration::from_millis(300), inbound_rx.recv()).await;
        assert!(fast_poll.is_err(), "unknown peer must not publish");

        harness.receiver_router.shutdown().await.ok();
    }

    /// Verdict 3 — a known peer that writes a bad-magic frame is dropped
    /// without touching the broadcast stream.
    #[tokio::test]
    async fn drops_bad_magic_without_publishing() {
        let sender_seed = [0x55u8; 32];
        let receiver_seed = [0x66u8; 32];

        let member_repo: Arc<dyn MemberRepositoryPort> = Arc::new(MemMemberRepo::default());
        let sender_member = make_member(sender_seed, "known-bad-actor");
        member_repo.save(&sender_member).await.unwrap();

        let harness = spawn_receiver(receiver_seed, Arc::clone(&member_repo)).await;
        let receiver_addr = harness.receiver_endpoint.addr();

        let sender_endpoint = bind_endpoint_with(sender_seed).await;
        wait_for_direct_addrs(&sender_endpoint).await;
        let conn = sender_endpoint
            .connect(receiver_addr, ACTIVE_CLIPBOARD_ALPN)
            .await
            .expect("dial receiver");
        let (mut send, _recv) = conn.open_bi().await.expect("open_bi");
        // Wrong magic (0xC1) + garbage; receiver bails at the magic check.
        send.write_all(&[0xC1, 0x00, 0x00, 0x00, 0x05])
            .await
            .expect("write garbage");
        send.finish().expect("finish");
        let _ = conn.closed().await;

        let mut inbound_rx = harness.inbound_rx;
        let fast_poll = tokio::time::timeout(Duration::from_millis(300), inbound_rx.recv()).await;
        assert!(fast_poll.is_err(), "bad magic must not publish");

        harness.receiver_router.shutdown().await.ok();
    }

    /// Verdict 4 — a well-formed frame whose `activated_by` exceeds the
    /// device-id bound is dropped at the trust boundary instead of panicking
    /// the accept task, and never reaches the broadcast stream.
    #[tokio::test]
    async fn drops_overlong_activated_by_without_publishing() {
        use uc_core::ids::device_id::DEVICE_ID_MAX_BYTES;

        let sender_seed = [0x77u8; 32];
        let receiver_seed = [0x88u8; 32];

        let member_repo: Arc<dyn MemberRepositoryPort> = Arc::new(MemMemberRepo::default());
        let sender_member = make_member(sender_seed, "sender-a");
        member_repo.save(&sender_member).await.unwrap();

        let harness = spawn_receiver(receiver_seed, Arc::clone(&member_repo)).await;
        let receiver_addr = harness.receiver_endpoint.addr();

        let mut msg = sample_message();
        msg.activated_by = "x".repeat(DEVICE_ID_MAX_BYTES + 1);
        send_one_frame(sender_seed, receiver_addr, &msg).await;

        let mut inbound_rx = harness.inbound_rx;
        let fast_poll = tokio::time::timeout(Duration::from_millis(300), inbound_rx.recv()).await;
        assert!(
            fast_poll.is_err(),
            "over-long activated_by must not publish"
        );

        harness.receiver_router.shutdown().await.ok();
    }
}
