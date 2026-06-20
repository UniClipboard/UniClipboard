//! Iroh-backed accept handler for the active-clipboard pull protocol (holder
//! side).
//!
//! Registered under [`ACTIVE_CLIPBOARD_PULL_ALPN`] as an independent sibling
//! of the bulk clipboard / presence / pairing / active-clipboard-state ALPNs
//! (see `uc-infra/AGENTS.md` §4.3). Each inbound connection runs one
//! request → response exchange: the requester sends a content hash, the
//! handler resolves it through the application-layer
//! [`ActiveClipboardPullServePort`], and writes back the transfer envelope (or
//! a typed no-content status).
//!
//! ## Admission
//!
//! Admission is **member-fingerprint only** (issue #1017 D2): the inbound
//! connection's `remote_id()` is resolved to a `SpaceMember` via the shared
//! [`IdentityFingerprintFactoryPort`] + [`MemberRepositoryPort`], and unknown
//! peers are dropped. There is **no** send-preference gate here — a member
//! whose `send_enabled` is off can still pull. This is the accepted asymmetry
//! with the active push path: the served content is still the
//! transfer-encrypted envelope and still requires the holder unlocked, so a
//! muted member gains nothing it could not already learn from the state
//! broadcast.
//!
//! ## Why the close barrier
//!
//! A pull is request → response on a single bi-stream. After writing the
//! response the handler waits for the requester to drain + close before
//! dropping the connection, so the QUIC teardown does not race the requester's
//! final read and silently lose the response.

use std::sync::Arc;
use std::time::Duration;

use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use tracing::{debug, warn};

use uc_core::ids::DeviceId;
use uc_core::membership::MemberRepositoryPort;
use uc_core::ports::clipboard::{ActiveClipboardPullServeError, ActiveClipboardPullServePort};
use uc_core::ports::security::IdentityFingerprintFactoryPort;
use uc_core::security::IdentityFingerprint;

use super::pull_wire::{self, PullResponse};

/// ALPN identifier for the active-clipboard pull protocol. An independent
/// sibling of the bulk clipboard / active-clipboard-state ALPNs so the Router
/// can multiplex every transport on the same endpoint.
pub const ACTIVE_CLIPBOARD_PULL_ALPN: &[u8] = b"uniclipboard/active-clipboard-pull/0";

/// Upper bound on waiting for the requester to drain + close after we write
/// the response. A cooperative requester closes within a couple of round
/// trips; this only caps the pathological "reads response but never closes"
/// peer so one bad peer can't pin an accept task.
const CLOSE_BARRIER_TIMEOUT: Duration = Duration::from_secs(3);

/// Serve-side adapter. Resolves `endpoint_id → DeviceId` for admission and
/// delegates content production to the application-layer serve port.
pub struct IrohActiveClipboardPullServeAdapter {
    state: Arc<HandlerState>,
}

struct HandlerState {
    member_repo: Arc<dyn MemberRepositoryPort>,
    fingerprint_factory: Arc<dyn IdentityFingerprintFactoryPort>,
    serve: Arc<dyn ActiveClipboardPullServePort>,
}

impl IrohActiveClipboardPullServeAdapter {
    pub fn new(
        member_repo: Arc<dyn MemberRepositoryPort>,
        fingerprint_factory: Arc<dyn IdentityFingerprintFactoryPort>,
        serve: Arc<dyn ActiveClipboardPullServePort>,
    ) -> Self {
        Self {
            state: Arc::new(HandlerState {
                member_repo,
                fingerprint_factory,
                serve,
            }),
        }
    }

    /// Cheap clone-able handle registered with iroh's `RouterBuilder`.
    pub fn handler(&self) -> IrohActiveClipboardPullServeHandler {
        IrohActiveClipboardPullServeHandler {
            state: Arc::clone(&self.state),
        }
    }
}

// ============================================================================
// ProtocolHandler (accept side)
// ============================================================================

#[derive(Clone)]
pub struct IrohActiveClipboardPullServeHandler {
    state: Arc<HandlerState>,
}

impl std::fmt::Debug for IrohActiveClipboardPullServeHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IrohActiveClipboardPullServeHandler")
            .finish_non_exhaustive()
    }
}

impl ProtocolHandler for IrohActiveClipboardPullServeHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let remote = connection.remote_id();
        let remote_bytes: [u8; 32] = *remote.as_bytes();

        // 1. Member-fingerprint admission ONLY (D2). Unknown peers never reach
        //    the serve port. No send-preference gate — a muted member can pull.
        let Some(peer_device_id) = self.state.resolve_device(&remote_bytes).await else {
            warn!(
                remote = %remote,
                "active-clipboard pull serve: unknown peer fingerprint; dropping connection"
            );
            return Ok(());
        };

        // 2. Accept the bi-stream. The requester opens it, writes the request,
        //    and reads our response on the same stream.
        let (mut send, mut recv) = match connection.accept_bi().await {
            Ok(pair) => pair,
            Err(err) => {
                warn!(
                    error = %err,
                    peer = %peer_device_id.as_str(),
                    "active-clipboard pull serve: accept_bi failed; dropping connection"
                );
                return Ok(());
            }
        };

        // 3. Read the request frame. A codec failure (bad magic, over-long
        //    hash, non-UTF8) drops the connection — the field is untrusted
        //    peer input and the codec validates lengths before allocating.
        let snapshot_hash = match pull_wire::read_request(&mut recv).await {
            Ok(h) => h,
            Err(err) => {
                warn!(
                    error = %err,
                    peer = %peer_device_id.as_str(),
                    "active-clipboard pull serve: request decode failed; dropping connection"
                );
                return Ok(());
            }
        };

        // 4. Resolve the content through the application-layer serve port.
        //    NotUnlocked / NotAvailable map to typed no-content statuses; a
        //    locked holder never leaks plaintext.
        let response = match self.state.serve.serve(&snapshot_hash).await {
            Ok(envelope) => PullResponse::Envelope(envelope),
            Err(ActiveClipboardPullServeError::NotAvailable) => {
                debug!(
                    peer = %peer_device_id.as_str(),
                    "active-clipboard pull serve: content not held; responding NotAvailable"
                );
                PullResponse::NotAvailable
            }
            Err(ActiveClipboardPullServeError::NotUnlocked) => {
                debug!(
                    peer = %peer_device_id.as_str(),
                    "active-clipboard pull serve: session locked; responding Locked"
                );
                PullResponse::Locked
            }
            Err(ActiveClipboardPullServeError::Internal(reason)) => {
                warn!(
                    peer = %peer_device_id.as_str(),
                    reason,
                    "active-clipboard pull serve: internal failure; responding Internal"
                );
                PullResponse::Internal
            }
        };

        // 5. Write the response frame, then close the send half.
        if let Err(err) = pull_wire::write_response(&mut send, &response).await {
            warn!(
                error = %err,
                peer = %peer_device_id.as_str(),
                "active-clipboard pull serve: response write failed; dropping connection"
            );
            return Ok(());
        }
        if let Err(err) = send.finish() {
            debug!(
                error = %err,
                peer = %peer_device_id.as_str(),
                "active-clipboard pull serve: send.finish failed"
            );
        }

        // 6. Wait for the requester to drain + close before dropping the
        //    connection, so QUIC teardown does not race its final read.
        let _ = tokio::time::timeout(CLOSE_BARRIER_TIMEOUT, connection.closed()).await;
        Ok(())
    }
}

impl HandlerState {
    /// Look up a `SpaceMember` whose `identity_fingerprint` equals the one
    /// derived from `remote_pubkey_bytes`. Returns `None` when the peer is
    /// unknown or the repository errors (logged). Mirrors the bulk receiver's
    /// member-fingerprint admission; the roster is bounded (N ≤ 10).
    async fn resolve_device(&self, remote_pubkey_bytes: &[u8; 32]) -> Option<DeviceId> {
        let derived = match self
            .fingerprint_factory
            .from_public_key(remote_pubkey_bytes)
        {
            Ok(fp) => fp,
            Err(err) => {
                warn!(
                    error = %err,
                    "active-clipboard pull serve: fingerprint derivation failed — cannot resolve peer"
                );
                return None;
            }
        };

        let members = match self.member_repo.list().await {
            Ok(ms) => ms,
            Err(err) => {
                warn!(
                    error = %err,
                    "active-clipboard pull serve: member_repo.list failed; treating peer as unknown"
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

    use super::super::pull_wire::{read_response, write_request, PullResponse};
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

    /// Serve port that records the requested hash and returns a canned result.
    struct StubServe {
        result: Mutex<Option<Result<Vec<u8>, ActiveClipboardPullServeError>>>,
        seen_hash: Mutex<Option<String>>,
    }
    impl StubServe {
        fn new(result: Result<Vec<u8>, ActiveClipboardPullServeError>) -> Arc<Self> {
            Arc::new(Self {
                result: Mutex::new(Some(result)),
                seen_hash: Mutex::new(None),
            })
        }
    }
    #[async_trait]
    impl ActiveClipboardPullServePort for StubServe {
        async fn serve(
            &self,
            snapshot_hash: &str,
        ) -> Result<Vec<u8>, ActiveClipboardPullServeError> {
            *self.seen_hash.lock().await = Some(snapshot_hash.to_string());
            self.result
                .lock()
                .await
                .take()
                .expect("serve called more than once")
        }
    }

    /// Serve port that panics if `serve` is ever reached — proves an unknown
    /// peer never reaches the serve port.
    struct NeverServe;
    #[async_trait]
    impl ActiveClipboardPullServePort for NeverServe {
        async fn serve(
            &self,
            _snapshot_hash: &str,
        ) -> Result<Vec<u8>, ActiveClipboardPullServeError> {
            panic!("serve reached past the admission gate");
        }
    }

    // ----- harness -----------------------------------------------------------

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

    async fn spawn_serve(
        seed: [u8; 32],
        member_repo: Arc<dyn MemberRepositoryPort>,
        serve: Arc<dyn ActiveClipboardPullServePort>,
    ) -> (Arc<Endpoint>, Router) {
        let endpoint = bind_endpoint_with(seed).await;
        wait_for_direct_addrs(&endpoint).await;
        let adapter = IrohActiveClipboardPullServeAdapter::new(
            member_repo,
            Arc::new(Sha256IdentityFingerprintFactory),
            serve,
        );
        let router = Router::builder((*endpoint).clone())
            .accept(ACTIVE_CLIPBOARD_PULL_ALPN, adapter.handler())
            .spawn();
        (endpoint, router)
    }

    /// Open a bi-stream, send a request, return the decoded response.
    async fn pull_request(
        sender_seed: [u8; 32],
        receiver_addr: iroh::EndpointAddr,
        snapshot_hash: &str,
    ) -> Result<PullResponse, super::pull_wire::PullWireError> {
        let sender = bind_endpoint_with(sender_seed).await;
        wait_for_direct_addrs(&sender).await;
        let conn = sender
            .connect(receiver_addr, ACTIVE_CLIPBOARD_PULL_ALPN)
            .await
            .expect("dial serve");
        let (mut send, mut recv) = conn.open_bi().await.expect("open_bi");
        write_request(&mut send, snapshot_hash)
            .await
            .expect("write request");
        send.finish().expect("finish");
        let resp = read_response(&mut recv).await;
        let _ = conn.closed().await;
        resp
    }

    // ----- verdicts ----------------------------------------------------------

    /// Verdict 1 — a known member's pull is served: the requested hash reaches
    /// the serve port and the envelope round-trips back verbatim.
    #[tokio::test]
    async fn member_pull_is_served_with_envelope() {
        let sender_seed = [0x11u8; 32];
        let receiver_seed = [0x22u8; 32];

        let member_repo: Arc<dyn MemberRepositoryPort> = Arc::new(MemMemberRepo::default());
        member_repo
            .save(&make_member(sender_seed, "member-a"))
            .await
            .unwrap();

        let envelope = vec![0x55, 0x43, 0x33, 0x00, 0x01, 0x02, 0x03];
        let serve = StubServe::new(Ok(envelope.clone()));
        let (endpoint, router) = spawn_serve(
            receiver_seed,
            Arc::clone(&member_repo),
            Arc::clone(&serve) as _,
        )
        .await;

        let hash = format!("blake3v1:{}", "9".repeat(64));
        let resp = pull_request(sender_seed, endpoint.addr(), &hash)
            .await
            .expect("response decodes");

        assert_eq!(resp, PullResponse::Envelope(envelope));
        assert_eq!(serve.seen_hash.lock().await.as_deref(), Some(hash.as_str()));

        router.shutdown().await.ok();
    }

    /// Verdict 2 — an unknown peer (fingerprint not in member_repo) is dropped
    /// at the admission gate; the serve port is never reached and the dial
    /// surfaces as a closed connection / io error (no response frame).
    #[tokio::test]
    async fn unknown_peer_is_dropped_before_serve() {
        let sender_seed = [0x33u8; 32];
        let receiver_seed = [0x44u8; 32];

        // member_repo is empty — sender's fingerprint is unknown.
        let member_repo: Arc<dyn MemberRepositoryPort> = Arc::new(MemMemberRepo::default());
        let (endpoint, router) = spawn_serve(
            receiver_seed,
            Arc::clone(&member_repo),
            Arc::new(NeverServe),
        )
        .await;

        let hash = format!("blake3v1:{}", "9".repeat(64));
        let resp = pull_request(sender_seed, endpoint.addr(), &hash).await;
        // The handler returns Ok(()) immediately without writing a frame, so
        // the requester's read hits EOF → Io error.
        assert!(resp.is_err(), "unknown peer must not get a response frame");

        router.shutdown().await.ok();
    }

    /// Verdict 3 — a locked holder responds `Locked` without panicking and
    /// without leaking any envelope bytes.
    #[tokio::test]
    async fn locked_holder_responds_locked() {
        let sender_seed = [0x55u8; 32];
        let receiver_seed = [0x66u8; 32];

        let member_repo: Arc<dyn MemberRepositoryPort> = Arc::new(MemMemberRepo::default());
        member_repo
            .save(&make_member(sender_seed, "member-a"))
            .await
            .unwrap();

        let serve = StubServe::new(Err(ActiveClipboardPullServeError::NotUnlocked));
        let (endpoint, router) = spawn_serve(receiver_seed, Arc::clone(&member_repo), serve).await;

        let hash = format!("blake3v1:{}", "9".repeat(64));
        let resp = pull_request(sender_seed, endpoint.addr(), &hash)
            .await
            .expect("response decodes");
        assert_eq!(resp, PullResponse::Locked);

        router.shutdown().await.ok();
    }

    /// Verdict 4 — a member asking for content the holder does not hold gets
    /// `NotAvailable`.
    #[tokio::test]
    async fn missing_content_responds_not_available() {
        let sender_seed = [0x77u8; 32];
        let receiver_seed = [0x88u8; 32];

        let member_repo: Arc<dyn MemberRepositoryPort> = Arc::new(MemMemberRepo::default());
        member_repo
            .save(&make_member(sender_seed, "member-a"))
            .await
            .unwrap();

        let serve = StubServe::new(Err(ActiveClipboardPullServeError::NotAvailable));
        let (endpoint, router) = spawn_serve(receiver_seed, Arc::clone(&member_repo), serve).await;

        let hash = format!("blake3v1:{}", "9".repeat(64));
        let resp = pull_request(sender_seed, endpoint.addr(), &hash)
            .await
            .expect("response decodes");
        assert_eq!(resp, PullResponse::NotAvailable);

        router.shutdown().await.ok();
    }
}
