//! Iroh-backed implementation of [`ActiveClipboardDispatchPort`].
//!
//! Each call resolves the target's stored address, opens a fresh iroh
//! bi-stream on [`ACTIVE_CLIPBOARD_ALPN`], writes one
//! [`ActiveClipboardWireMessage`] frame (per [`wire`]), and
//! closes. There is no ack: active-clipboard state is a fire-and-forget
//! last-writer-wins observation, matching the receiver side's no-ack accept
//! handler. Concurrent fan-out to multiple peers is assembled by the
//! application layer; this adapter stays single-target.
//!
//! Failure mapping mirrors the bulk
//! [`IrohClipboardDispatchAdapter`](super::clipboard_dispatch_adapter::IrohClipboardDispatchAdapter):
//!
//! * Missing / undecodable stored address, or dial failure →
//!   [`ActiveClipboardDispatchError::Offline`]. The record is stale or the
//!   peer is genuinely unreachable; both self-heal on the next pairing
//!   refresh or inbound dispatch.
//! * Stream write I/O failure → [`ActiveClipboardDispatchError::Io`].

use std::sync::Arc;

use async_trait::async_trait;
use iroh::{Endpoint, EndpointAddr};
use tracing::{debug, instrument, warn};

use uc_core::clipboard::ActiveClipboardState;
use uc_core::ids::DeviceId;
use uc_core::ports::{
    ActiveClipboardDispatchError, ActiveClipboardDispatchPort, PeerAddressRepositoryPort,
};

use super::super::connect::connect_with_staggered_retry;
use super::receiver_adapter::ACTIVE_CLIPBOARD_ALPN;
use super::wire::{self, ActiveClipboardWireMessage};

/// Sends one active-clipboard state observation to a single peer over the
/// active-clipboard ALPN. Reuses the shared endpoint + `peer_addr_repo` so a
/// state send rides the same NAT/relay mapping presence already established.
pub struct IrohActiveClipboardDispatchAdapter {
    endpoint: Arc<Endpoint>,
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
}

impl IrohActiveClipboardDispatchAdapter {
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
    /// `None` (no record, undecodable blob, or repo error) maps to
    /// `Offline`. Mirrors the bulk dispatch adapter's `resolve_addr`.
    async fn resolve_addr(&self, target: &DeviceId) -> Option<EndpointAddr> {
        match self.peer_addr_repo.get(target).await {
            Ok(Some(record)) => match postcard::from_bytes::<EndpointAddr>(&record.addr_blob) {
                Ok(addr) => Some(addr),
                Err(err) => {
                    warn!(
                        device = %target.as_str(),
                        error = %err,
                        "active-clipboard dispatch: peer_addr_repo blob did not postcard-decode; \
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
                    "active-clipboard dispatch: peer_addr_repo.get failed; treating peer as offline"
                );
                None
            }
        }
    }
}

#[async_trait]
impl ActiveClipboardDispatchPort for IrohActiveClipboardDispatchAdapter {
    #[instrument(skip_all, fields(device = %target.as_str(), snapshot_hash = %state.snapshot_hash))]
    async fn dispatch(
        &self,
        target: &DeviceId,
        state: &ActiveClipboardState,
    ) -> Result<(), ActiveClipboardDispatchError> {
        // 1. Resolve address; missing / bad record = offline.
        let addr = match self.resolve_addr(target).await {
            Some(a) => a,
            None => return Err(ActiveClipboardDispatchError::Offline),
        };

        // 2. Dial. Dial failure = offline (no typed iroh error leaks up).
        //    The active-clipboard send is fire-and-forget and the register
        //    is convergent, so a missed send is recovered the next time the
        //    register advances or a peer-online resync runs; the bulk
        //    dispatch path's single-flight storm collapse is unnecessary
        //    here (no per-keystroke fan-out of state frames).
        let connection = match connect_with_staggered_retry(
            Arc::clone(&self.endpoint),
            addr,
            ACTIVE_CLIPBOARD_ALPN,
            "active-clipboard",
        )
        .await
        {
            Ok(connection) => connection,
            Err(err) => {
                debug!(
                    error = %err,
                    "active-clipboard dispatch: dial failed, treating as Offline"
                );
                return Err(ActiveClipboardDispatchError::Offline);
            }
        };

        // 3. Open one bi-stream and write the single state frame. The
        //    receiver reads one frame and returns; we never read a reply.
        let (mut send, _recv) = connection
            .open_bi()
            .await
            .map_err(|err| ActiveClipboardDispatchError::Io(format!("open_bi: {err}")))?;

        let msg = ActiveClipboardWireMessage {
            snapshot_hash: state.snapshot_hash.clone(),
            entry_id: state.entry_id.as_ref().to_string(),
            activated_at_ms: state.activated_at_ms,
            activated_by: state.activated_by.as_str().to_string(),
        };
        wire::write_frame(&mut send, &msg)
            .await
            .map_err(|err| ActiveClipboardDispatchError::Io(format!("frame write: {err}")))?;
        send.finish()
            .map_err(|err| ActiveClipboardDispatchError::Io(format!("send.finish: {err}")))?;

        // 4. Wait for the peer to drain the stream and close before we drop
        //    the connection. The receiver reads exactly one frame then returns
        //    `Ok(())`, dropping its side — `closed()` resolves at that point.
        //    Without this barrier, dropping the QUIC connection immediately
        //    after `finish()` can tear it down before the peer's accept task
        //    drains the stream, silently losing the frame. The timeout guards
        //    against a peer that accepts but never closes: the frame was
        //    already written, so we still return `Ok`.
        let _ = tokio::time::timeout(CLOSE_BARRIER_TIMEOUT, connection.closed()).await;
        drop(send);
        drop(connection);

        Ok(())
    }
}

/// Upper bound on waiting for the peer to drain + close after we finish the
/// send half. A cooperative receiver closes within a couple of round trips;
/// this only caps the pathological "accepts but never closes" peer so a
/// single bad peer can't stall a fan-out pass.
const CLOSE_BARRIER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use chrono::Utc;
    use iroh::{RelayMode, SecretKey};
    use tokio::sync::Mutex;

    use uc_core::ids::EntryId;
    use uc_core::ports::{PeerAddressError, PeerAddressRecord};

    use super::super::receiver_adapter::{
        IrohActiveClipboardReceiverAdapter, ACTIVE_CLIPBOARD_ALPN,
    };
    use crate::security::Sha256IdentityFingerprintFactory;

    use iroh::protocol::Router;
    use std::collections::HashMap;
    use tokio::sync::broadcast;
    use uc_core::membership::{MembershipError, SpaceMember};
    use uc_core::ports::security::IdentityFingerprintFactoryPort;
    use uc_core::ports::ActiveClipboardReceiverPort;
    use uc_core::{MemberRepositoryPort, MemberSyncPreferences};

    // ----- in-memory peer_addr_repo -----------------------------------------

    #[derive(Default)]
    struct MemRepo {
        inner: Mutex<HashMap<String, PeerAddressRecord>>,
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

    // ----- in-memory member_repo (receiver-side identity admission) ----------

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

    fn sample_state() -> ActiveClipboardState {
        ActiveClipboardState::new(
            format!("blake3v1:{}", "7".repeat(64)),
            EntryId::from("01941b00-0000-7000-8000-000000000099"),
            1_700_000_000_123,
            DeviceId::new("sender-dispatch"),
        )
    }

    /// Verdict 1 — happy path. The dispatched state observation reaches a
    /// live receiver's broadcast stream field-for-field, attributed to the
    /// sender, and `dispatch` returns `Ok(())` (fire-and-forget).
    #[tokio::test]
    async fn dispatch_delivers_state_to_live_receiver() {
        let sender_seed = [0x21u8; 32];
        let receiver_seed = [0x22u8; 32];

        // Receiver admits the sender by fingerprint.
        let member_repo: Arc<dyn MemberRepositoryPort> = Arc::new(MemMemberRepo::default());
        member_repo
            .save(&make_member(sender_seed, "sender-dispatch"))
            .await
            .unwrap();

        let receiver_endpoint = bind_endpoint_with(receiver_seed).await;
        wait_for_direct_addrs(&receiver_endpoint).await;
        let adapter = IrohActiveClipboardReceiverAdapter::new(
            member_repo,
            Arc::new(Sha256IdentityFingerprintFactory),
        );
        let mut inbound_rx: broadcast::Receiver<_> = adapter.subscribe();
        let receiver_router = Router::builder((*receiver_endpoint).clone())
            .accept(ACTIVE_CLIPBOARD_ALPN, adapter.handler())
            .spawn();
        let receiver_addr = receiver_endpoint.addr();

        // Sender dials the receiver's stored address.
        let sender_endpoint = bind_endpoint_with(sender_seed).await;
        wait_for_direct_addrs(&sender_endpoint).await;
        let repo = Arc::new(MemRepo::default());
        let target = DeviceId::new("receiver-x");
        seed_addr(&repo, &target, &receiver_addr).await;

        let dispatcher = IrohActiveClipboardDispatchAdapter::new(sender_endpoint, repo);
        let state = sample_state();
        dispatcher
            .dispatch(&target, &state)
            .await
            .expect("dispatch succeeds");

        let inbound = tokio::time::timeout(Duration::from_secs(3), inbound_rx.recv())
            .await
            .expect("broadcast arrives within timeout")
            .expect("subscriber sees the observation");
        assert_eq!(inbound.peer_device_id.as_str(), "sender-dispatch");
        assert_eq!(inbound.snapshot_hash, state.snapshot_hash);
        assert_eq!(inbound.sender_entry_id, state.entry_id.as_ref());
        assert_eq!(inbound.activated_at_ms, state.activated_at_ms);
        assert_eq!(inbound.activated_by.as_str(), "sender-dispatch");

        receiver_router.shutdown().await.ok();
    }

    /// Verdict 2 — missing peer_addr entry. Dispatch returns `Offline`
    /// without touching the network.
    #[tokio::test]
    async fn dispatch_returns_offline_when_peer_addr_missing() {
        let sender_endpoint = bind_endpoint_with([0x31u8; 32]).await;
        let repo = Arc::new(MemRepo::default());
        let dispatcher = IrohActiveClipboardDispatchAdapter::new(sender_endpoint, repo);

        let result = dispatcher
            .dispatch(&DeviceId::new("never-paired"), &sample_state())
            .await;
        match result {
            Err(ActiveClipboardDispatchError::Offline) => {}
            other => panic!("expected Offline, got {other:?}"),
        }
    }
}
