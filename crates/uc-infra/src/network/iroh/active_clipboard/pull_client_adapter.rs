//! Iroh-backed implementation of [`ActiveClipboardPullClientPort`] (requester
//! side).
//!
//! Resolves the target peer's stored address, dials it on
//! [`ACTIVE_CLIPBOARD_PULL_ALPN`], opens one bi-stream, writes the content-hash
//! request (per [`pull_wire`]), and awaits the holder's
//! response. The whole exchange is bounded by [`PULL_TIMEOUT`] (issue #1017
//! D6, 10s); a peer that is unreachable or slow surfaces as
//! [`ActiveClipboardPullClientError::Unreachable`].
//!
//! Failure mapping:
//! * Missing / undecodable stored address, dial failure, or deadline →
//!   [`ActiveClipboardPullClientError::Unreachable`].
//! * Holder responds `NotAvailable` / `Locked` / `Internal` →
//!   [`ActiveClipboardPullClientError::NotAvailable`] (the requester cannot
//!   distinguish these and treats them identically: this peer cannot serve).
//! * Stream write / read / codec failure →
//!   [`ActiveClipboardPullClientError::Io`].
//!
//! The returned bytes are the opaque transfer envelope the holder produced;
//! decoding + storing them is the application layer's job.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use iroh::{Endpoint, EndpointAddr};
use tracing::{debug, instrument, warn};

use uc_core::ids::DeviceId;
use uc_core::ports::clipboard::{ActiveClipboardPullClientError, ActiveClipboardPullClientPort};
use uc_core::ports::PeerAddressRepositoryPort;

use super::super::connect::connect_with_staggered_retry;
use super::pull_serve_adapter::ACTIVE_CLIPBOARD_PULL_ALPN;
use super::pull_wire::{self, PullResponse};

/// Hard deadline on one pull exchange (dial + request + response). Issue #1017
/// D6 fixes this at 10s: pull-fail (timeout, offline, holder locked) does not
/// advance the register or re-broadcast, and there is no retry loop, so this
/// is the only bound that matters for liveness on the requesting side.
const PULL_TIMEOUT: Duration = Duration::from_secs(10);

/// Requests one active-clipboard pull from a single peer over the pull ALPN.
/// Reuses the shared endpoint + `peer_addr_repo` so a pull rides the same
/// NAT/relay mapping presence already established.
pub struct IrohActiveClipboardPullClientAdapter {
    endpoint: Arc<Endpoint>,
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
}

impl IrohActiveClipboardPullClientAdapter {
    pub fn new(
        endpoint: Arc<Endpoint>,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    ) -> Self {
        Self {
            endpoint,
            peer_addr_repo,
        }
    }

    /// Resolve the peer's current [`EndpointAddr`]. `None` (no record,
    /// undecodable blob, or repo error) maps to `Unreachable`. Mirrors the
    /// dispatch adapter's `resolve_addr`.
    async fn resolve_addr(&self, target: &DeviceId) -> Option<EndpointAddr> {
        match self.peer_addr_repo.get(target).await {
            Ok(Some(record)) => match postcard::from_bytes::<EndpointAddr>(&record.addr_blob) {
                Ok(addr) => Some(addr),
                Err(err) => {
                    warn!(
                        device = %target.as_str(),
                        error = %err,
                        "active-clipboard pull: peer_addr_repo blob did not postcard-decode; \
                         treating peer as unreachable"
                    );
                    None
                }
            },
            Ok(None) => None,
            Err(err) => {
                warn!(
                    device = %target.as_str(),
                    error = %err,
                    "active-clipboard pull: peer_addr_repo.get failed; treating peer as unreachable"
                );
                None
            }
        }
    }
}

#[async_trait]
impl ActiveClipboardPullClientPort for IrohActiveClipboardPullClientAdapter {
    #[instrument(skip_all, fields(device = %peer.as_str(), snapshot_hash = %snapshot_hash))]
    async fn pull(
        &self,
        peer: &DeviceId,
        snapshot_hash: &str,
    ) -> Result<Vec<u8>, ActiveClipboardPullClientError> {
        // The whole dial + request + response exchange is bounded by a single
        // deadline (D6, 10s). A timeout maps to `Unreachable` — pull-fail does
        // not advance the register, so the caller just logs and drops.
        match tokio::time::timeout(PULL_TIMEOUT, self.exchange(peer, snapshot_hash)).await {
            Ok(result) => result,
            Err(_) => {
                debug!("active-clipboard pull: exceeded deadline; treating as Unreachable");
                Err(ActiveClipboardPullClientError::Unreachable)
            }
        }
    }
}

impl IrohActiveClipboardPullClientAdapter {
    /// The unbounded inner exchange — wrapped in the pull deadline by `pull`.
    async fn exchange(
        &self,
        peer: &DeviceId,
        snapshot_hash: &str,
    ) -> Result<Vec<u8>, ActiveClipboardPullClientError> {
        // 1. Resolve address; missing / bad record = unreachable.
        let addr = match self.resolve_addr(peer).await {
            Some(a) => a,
            None => return Err(ActiveClipboardPullClientError::Unreachable),
        };

        // 2. Dial. Dial failure = unreachable (no typed iroh error leaks up).
        let connection = match connect_with_staggered_retry(
            Arc::clone(&self.endpoint),
            addr,
            ACTIVE_CLIPBOARD_PULL_ALPN,
            "active-clipboard-pull",
        )
        .await
        {
            Ok(connection) => connection,
            Err(err) => {
                debug!(
                    error = %err,
                    "active-clipboard pull: dial failed, treating as Unreachable"
                );
                return Err(ActiveClipboardPullClientError::Unreachable);
            }
        };

        // 3. Open one bi-stream, write the request, close the send half.
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|err| ActiveClipboardPullClientError::Io(format!("open_bi: {err}")))?;

        pull_wire::write_request(&mut send, snapshot_hash)
            .await
            .map_err(|err| ActiveClipboardPullClientError::Io(format!("request write: {err}")))?;
        send.finish()
            .map_err(|err| ActiveClipboardPullClientError::Io(format!("send.finish: {err}")))?;

        // 4. Read the response frame.
        let response = pull_wire::read_response(&mut recv)
            .await
            .map_err(|err| ActiveClipboardPullClientError::Io(format!("response read: {err}")))?;

        // 5. Actively close now that the full response frame is read. The
        //    serve side waits on `connection.closed()` before tearing down (so
        //    its teardown can't race our read); initiating the close from this
        //    side resolves that barrier immediately. Previously this side *also*
        //    waited on `connection.closed()`, so both ends sat idle until the
        //    serve's 3s close-barrier timeout fired — a fixed ~3s tail on every
        //    pull, which was the dominant cost of the #1017 restore lag (the
        //    dial itself completes in ~10ms; the response is one small frame).
        //    `read_response` has already consumed the whole framed body, so
        //    closing here cannot truncate anything. Mirrors the asymmetric
        //    teardown the bulk dispatch path already uses (responder waits,
        //    requester closes).
        connection.close(0u32.into(), b"pull-complete");

        match response {
            PullResponse::Envelope(bytes) => Ok(bytes),
            PullResponse::NotAvailable | PullResponse::Locked | PullResponse::Internal => {
                debug!(
                    ?response,
                    "active-clipboard pull: holder cannot serve the content"
                );
                Err(ActiveClipboardPullClientError::NotAvailable)
            }
        }
    }
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
    use iroh::{RelayMode, SecretKey};
    use tokio::sync::Mutex;

    use uc_core::membership::{MembershipError, SpaceMember};
    use uc_core::ports::clipboard::{ActiveClipboardPullServeError, ActiveClipboardPullServePort};
    use uc_core::ports::security::IdentityFingerprintFactoryPort;
    use uc_core::ports::{PeerAddressError, PeerAddressRecord};
    use uc_core::{MemberRepositoryPort, MemberSyncPreferences};

    use super::super::pull_serve_adapter::{
        IrohActiveClipboardPullServeAdapter, ACTIVE_CLIPBOARD_PULL_ALPN,
    };
    use crate::security::Sha256IdentityFingerprintFactory;

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

    // ----- in-memory member_repo (serve-side admission) ----------------------

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

    /// Serve port returning a canned result.
    struct StubServe(Mutex<Option<Result<Vec<u8>, ActiveClipboardPullServeError>>>);
    impl StubServe {
        fn new(result: Result<Vec<u8>, ActiveClipboardPullServeError>) -> Arc<Self> {
            Arc::new(Self(Mutex::new(Some(result))))
        }
    }
    #[async_trait]
    impl ActiveClipboardPullServePort for StubServe {
        async fn serve(
            &self,
            _snapshot_hash: &str,
        ) -> Result<Vec<u8>, ActiveClipboardPullServeError> {
            self.0
                .lock()
                .await
                .take()
                .expect("serve called more than once")
        }
    }

    async fn bind_endpoint_with(seed: [u8; 32]) -> Arc<Endpoint> {
        Arc::new(
            Endpoint::builder(iroh::endpoint::presets::N0)
                .secret_key(SecretKey::from_bytes(&seed))
                .alpns(vec![ACTIVE_CLIPBOARD_PULL_ALPN.to_vec()])
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

    /// Spawn a serve router for a member, returning its addr + router handle.
    async fn spawn_serve_for(
        serve_seed: [u8; 32],
        requester_seed: [u8; 32],
        requester_device: &str,
        serve_result: Result<Vec<u8>, ActiveClipboardPullServeError>,
    ) -> (EndpointAddr, Router) {
        let member_repo: Arc<dyn MemberRepositoryPort> = Arc::new(MemMemberRepo::default());
        member_repo
            .save(&make_member(requester_seed, requester_device))
            .await
            .unwrap();
        let endpoint = bind_endpoint_with(serve_seed).await;
        wait_for_direct_addrs(&endpoint).await;
        let adapter = IrohActiveClipboardPullServeAdapter::new(
            member_repo,
            Arc::new(Sha256IdentityFingerprintFactory),
            StubServe::new(serve_result),
        );
        let router = Router::builder((*endpoint).clone())
            .accept(ACTIVE_CLIPBOARD_PULL_ALPN, adapter.handler())
            .spawn();
        (endpoint.addr(), router)
    }

    // ----- verdicts ----------------------------------------------------------

    /// Verdict 1 — end-to-end happy path: the client dials a live serve
    /// handler and returns the served envelope bytes verbatim.
    #[tokio::test]
    async fn pull_returns_served_envelope() {
        let requester_seed = [0x21u8; 32];
        let serve_seed = [0x22u8; 32];

        let envelope = vec![0x55, 0x43, 0x33, 0x00, 0xAB, 0xCD];
        let (serve_addr, router) = spawn_serve_for(
            serve_seed,
            requester_seed,
            "requester-x",
            Ok(envelope.clone()),
        )
        .await;

        let requester_endpoint = bind_endpoint_with(requester_seed).await;
        wait_for_direct_addrs(&requester_endpoint).await;
        let repo = Arc::new(MemRepo::default());
        let target = DeviceId::new("holder");
        seed_addr(&repo, &target, &serve_addr).await;

        let client = IrohActiveClipboardPullClientAdapter::new(requester_endpoint, repo);
        let hash = format!("blake3v1:{}", "7".repeat(64));
        let got = client.pull(&target, &hash).await.expect("pull succeeds");
        assert_eq!(got, envelope);

        router.shutdown().await.ok();
    }

    /// Verdict 1b — the pull returns promptly, not after the serve side's 3s
    /// close-barrier timeout. Both ends used to wait on `connection.closed()`,
    /// so a successful pull against the real serve handler always burned the
    /// full `CLOSE_BARRIER_TIMEOUT` (3s) before returning — the dominant cost of
    /// the #1017 restore lag. The client now actively closes once the response
    /// is read, resolving the serve barrier immediately. A 2s bound clears the
    /// real exchange (low ms on loopback) by a wide margin while still failing
    /// hard if the 3s standoff is reintroduced.
    #[tokio::test]
    async fn pull_returns_before_close_barrier_timeout() {
        use std::time::Instant;

        let requester_seed = [0x23u8; 32];
        let serve_seed = [0x24u8; 32];

        let envelope = vec![0x55, 0x43, 0x33, 0x00, 0x99];
        let (serve_addr, router) = spawn_serve_for(
            serve_seed,
            requester_seed,
            "requester-fast",
            Ok(envelope.clone()),
        )
        .await;

        let requester_endpoint = bind_endpoint_with(requester_seed).await;
        wait_for_direct_addrs(&requester_endpoint).await;
        let repo = Arc::new(MemRepo::default());
        let target = DeviceId::new("holder");
        seed_addr(&repo, &target, &serve_addr).await;

        let client = IrohActiveClipboardPullClientAdapter::new(requester_endpoint, repo);
        let hash = format!("blake3v1:{}", "5".repeat(64));

        let started = Instant::now();
        let got = client.pull(&target, &hash).await.expect("pull succeeds");
        let elapsed = started.elapsed();

        assert_eq!(got, envelope);
        assert!(
            elapsed < Duration::from_secs(2),
            "pull took {elapsed:?}; the close-barrier standoff (~3s) appears to be back",
        );

        router.shutdown().await.ok();
    }

    /// Verdict 2 — the holder responds `Locked`; the client maps it to
    /// `NotAvailable` (a locked holder cannot serve).
    #[tokio::test]
    async fn pull_maps_locked_holder_to_not_available() {
        let requester_seed = [0x31u8; 32];
        let serve_seed = [0x32u8; 32];

        let (serve_addr, router) = spawn_serve_for(
            serve_seed,
            requester_seed,
            "requester-x",
            Err(ActiveClipboardPullServeError::NotUnlocked),
        )
        .await;

        let requester_endpoint = bind_endpoint_with(requester_seed).await;
        wait_for_direct_addrs(&requester_endpoint).await;
        let repo = Arc::new(MemRepo::default());
        let target = DeviceId::new("holder");
        seed_addr(&repo, &target, &serve_addr).await;

        let client = IrohActiveClipboardPullClientAdapter::new(requester_endpoint, repo);
        let hash = format!("blake3v1:{}", "7".repeat(64));
        let err = client
            .pull(&target, &hash)
            .await
            .expect_err("locked holder must surface as error");
        assert!(matches!(err, ActiveClipboardPullClientError::NotAvailable));

        router.shutdown().await.ok();
    }

    /// Verdict 3 — no stored address for the peer → `Unreachable` without
    /// touching the network.
    #[tokio::test]
    async fn pull_returns_unreachable_when_addr_missing() {
        let requester_endpoint = bind_endpoint_with([0x41u8; 32]).await;
        let repo = Arc::new(MemRepo::default());
        let client = IrohActiveClipboardPullClientAdapter::new(requester_endpoint, repo);

        let err = client
            .pull(
                &DeviceId::new("never-seen"),
                &format!("blake3v1:{}", "7".repeat(64)),
            )
            .await
            .expect_err("missing addr must surface as error");
        assert!(matches!(err, ActiveClipboardPullClientError::Unreachable));
    }
}
