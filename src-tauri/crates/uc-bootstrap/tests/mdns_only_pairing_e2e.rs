//! mDNS-only end-to-end pairing test.
//!
//! Sibling of [`slice1_handshake_e2e`] but with the rendezvous mock
//! wired to **always return 5xx**. That forces:
//!
//! * sponsor's `RendezvousPairingInvitationAdapter::issue_invitation` to
//!   take the "cloud unreachable" branch (`is_cloud_recoverable` →
//!   `true` for `ServiceUnavailable`), mint the code locally, and start
//!   only the `MdnsPairingPublisher`;
//! * joiner's `IrohPairingSessionAdapter::resolve_invitation` race to
//!   resolve through the cloud channel and fail, leaving the LAN
//!   (`swarm-discovery`) channel as the only viable resolution path.
//!
//! The remainder of the handshake (`PAIRING_ALPN` over iroh loopback)
//! is unchanged — this test pins the new LAN-only discovery path
//! end-to-end without depending on any external network.
//!
//! ## Why `#[ignore]` by default
//!
//! `swarm-discovery` binds real multicast sockets on every up
//! interface. CI sandboxes (esp. GitHub Actions runners) often deny
//! `IP_ADD_MEMBERSHIP`, so this test is flaky on CI. Run locally with
//!
//! ```text
//! cargo test -p uc-bootstrap --test mdns_only_pairing_e2e -- --ignored --nocapture
//! ```
//!
//! macOS / Linux desktops with Wi-Fi or Ethernet up should pass
//! reliably; if you see "no match within window" it likely means the
//! host has no multicast-capable interface available right now.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use uc_application::facade::space_setup::{
    InitializeSpaceInput, RedeemPairingInvitationInput, SpaceSetupDeps, SpaceSetupFacade,
};
use uc_application::proof::HmacProofAdapter;
use uc_bootstrap::IrohNodeConfig;
use uc_core::ids::DeviceId;
use uc_core::membership::{MemberRepositoryPort, MembershipError, SpaceMember};
use uc_core::ports::pairing::PairingSessionPort;
use uc_core::ports::space::{ProofPort, SpaceAccessPort};
use uc_core::ports::{
    ClockPort, DeviceIdentityPort, LocalIdentityPort, SecureStorageError, SecureStoragePort,
    SettingsPort, SetupStatusPort,
};
use uc_core::settings::model::Settings;
use uc_core::setup::SetupStatus;
use uc_core::trusted_peer::{TrustedPeer, TrustedPeerError, TrustedPeerRepositoryPort};

use uc_infra::fs::key_slot_store::JsonKeySlotStore;
use uc_infra::network::iroh::{IrohIdentityStore, IrohNode, IrohNodeBuilder, PairingHandlers};
use uc_infra::security::{
    DefaultCurrentProfile, DefaultSpaceAccessAdapter, InMemorySession, KeyMaterialStore,
    Sha256IdentityFingerprintFactory,
};

// ─── in-memory fakes ────────────────────────────────────────────────────────
//
// Direct copies of the slice1 fixtures. Kept inline (not extracted to
// `common/`) so this file is self-contained and a maintainer reading
// the test can verify it doesn't share unexpected state with sibling
// e2e suites.

#[derive(Default)]
struct InMemorySecureStorage {
    map: StdMutex<HashMap<String, Vec<u8>>>,
}
impl SecureStoragePort for InMemorySecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        Ok(self.map.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
        self.map
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_vec());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
        self.map.lock().unwrap().remove(key);
        Ok(())
    }
}

#[derive(Default)]
struct InMemoryMemberRepo {
    rows: StdMutex<Vec<SpaceMember>>,
}
#[async_trait]
impl MemberRepositoryPort for InMemoryMemberRepo {
    async fn get(&self, device_id: &DeviceId) -> Result<Option<SpaceMember>, MembershipError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .find(|m| &m.device_id == device_id)
            .cloned())
    }
    async fn list(&self) -> Result<Vec<SpaceMember>, MembershipError> {
        Ok(self.rows.lock().unwrap().clone())
    }
    async fn save(&self, member: &SpaceMember) -> Result<(), MembershipError> {
        let mut rows = self.rows.lock().unwrap();
        if let Some(existing) = rows.iter_mut().find(|m| m.device_id == member.device_id) {
            *existing = member.clone();
        } else {
            rows.push(member.clone());
        }
        Ok(())
    }
    async fn remove(&self, device_id: &DeviceId) -> Result<bool, MembershipError> {
        let mut rows = self.rows.lock().unwrap();
        let before = rows.len();
        rows.retain(|m| &m.device_id != device_id);
        Ok(rows.len() != before)
    }
}

#[derive(Default)]
struct InMemoryPeerAddrRepo {
    rows: StdMutex<Vec<uc_core::ports::PeerAddressRecord>>,
}
#[async_trait]
impl uc_core::ports::PeerAddressRepositoryPort for InMemoryPeerAddrRepo {
    async fn get(
        &self,
        device_id: &DeviceId,
    ) -> Result<Option<uc_core::ports::PeerAddressRecord>, uc_core::ports::PeerAddressError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .find(|r| &r.device_id == device_id)
            .cloned())
    }
    async fn upsert(
        &self,
        record: &uc_core::ports::PeerAddressRecord,
    ) -> Result<(), uc_core::ports::PeerAddressError> {
        let mut rows = self.rows.lock().unwrap();
        if let Some(existing) = rows.iter_mut().find(|r| r.device_id == record.device_id) {
            *existing = record.clone();
        } else {
            rows.push(record.clone());
        }
        Ok(())
    }
    async fn list(
        &self,
    ) -> Result<Vec<uc_core::ports::PeerAddressRecord>, uc_core::ports::PeerAddressError> {
        Ok(self.rows.lock().unwrap().clone())
    }
    async fn remove(&self, device_id: &DeviceId) -> Result<(), uc_core::ports::PeerAddressError> {
        self.rows
            .lock()
            .unwrap()
            .retain(|r| &r.device_id != device_id);
        Ok(())
    }
}

#[derive(Default)]
struct InMemoryTrustedPeerRepo {
    rows: StdMutex<Vec<TrustedPeer>>,
}
#[async_trait]
impl TrustedPeerRepositoryPort for InMemoryTrustedPeerRepo {
    async fn get(&self, device_id: &DeviceId) -> Result<Option<TrustedPeer>, TrustedPeerError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .find(|p| &p.peer_device_id == device_id)
            .cloned())
    }
    async fn list(&self) -> Result<Vec<TrustedPeer>, TrustedPeerError> {
        Ok(self.rows.lock().unwrap().clone())
    }
    async fn save(&self, peer: &TrustedPeer) -> Result<(), TrustedPeerError> {
        let mut rows = self.rows.lock().unwrap();
        if let Some(existing) = rows
            .iter_mut()
            .find(|p| p.peer_device_id == peer.peer_device_id)
        {
            *existing = peer.clone();
        } else {
            rows.push(peer.clone());
        }
        Ok(())
    }
    async fn remove(&self, device_id: &DeviceId) -> Result<bool, TrustedPeerError> {
        let mut rows = self.rows.lock().unwrap();
        let before = rows.len();
        rows.retain(|p| &p.peer_device_id != device_id);
        Ok(rows.len() != before)
    }
}

#[derive(Default)]
struct InMemorySetupStatus {
    status: StdMutex<SetupStatus>,
}
#[async_trait]
impl SetupStatusPort for InMemorySetupStatus {
    async fn get_status(&self) -> anyhow::Result<SetupStatus> {
        Ok(self.status.lock().unwrap().clone())
    }
    async fn set_status(&self, status: &SetupStatus) -> anyhow::Result<()> {
        *self.status.lock().unwrap() = status.clone();
        Ok(())
    }
}

struct InMemorySettings(StdMutex<Settings>);
impl InMemorySettings {
    fn with_device_name(name: &str) -> Arc<Self> {
        let mut s = Settings::default();
        s.general.device_name = Some(name.into());
        Arc::new(Self(StdMutex::new(s)))
    }
}
#[async_trait]
impl SettingsPort for InMemorySettings {
    async fn load(&self) -> anyhow::Result<Settings> {
        Ok(self.0.lock().unwrap().clone())
    }
    async fn save(&self, s: &Settings) -> anyhow::Result<()> {
        *self.0.lock().unwrap() = s.clone();
        Ok(())
    }
}

struct FixedDeviceIdentity(DeviceId);
impl DeviceIdentityPort for FixedDeviceIdentity {
    fn current_device_id(&self) -> DeviceId {
        self.0.clone()
    }
}

struct SystemClock;
impl ClockPort for SystemClock {
    fn now_ms(&self) -> i64 {
        chrono::Utc::now().timestamp_millis()
    }
}

// ─── per-side assembly ──────────────────────────────────────────────────────

struct Side {
    facade: Arc<SpaceSetupFacade>,
    iroh_node: IrohNode,
    member_repo: Arc<InMemoryMemberRepo>,
    trusted_peer_repo: Arc<InMemoryTrustedPeerRepo>,
    setup_status: Arc<InMemorySetupStatus>,
    device_id: DeviceId,
    _keystore_dir: TempDir,
}

impl Side {
    async fn shutdown(self) {
        self.facade.on_shutdown().await;
        self.iroh_node.shutdown().await;
    }
}

async fn build_side(name: &'static str, rendezvous_base_url: String) -> Side {
    let device_id = DeviceId::new(format!("{name}-device"));
    let settings = InMemorySettings::with_device_name(name);
    let setup_status = Arc::new(InMemorySetupStatus::default());
    let member_repo = Arc::new(InMemoryMemberRepo::default());
    let trusted_peer_repo = Arc::new(InMemoryTrustedPeerRepo::default());
    let peer_addr_repo = Arc::new(InMemoryPeerAddrRepo::default());

    let secure_storage: Arc<dyn SecureStoragePort> = Arc::new(InMemorySecureStorage::default());
    let identity_store = Arc::new(IrohIdentityStore::new(
        Arc::clone(&secure_storage),
        Arc::new(Sha256IdentityFingerprintFactory),
    ));

    let keystore_dir = TempDir::new().expect("keystore tempdir");
    let keyslot_store = Arc::new(JsonKeySlotStore::new(keystore_dir.path().to_path_buf()));
    let key_material = Arc::new(KeyMaterialStore::new(
        Arc::clone(&secure_storage),
        keyslot_store,
    ));
    let current_profile = Arc::new(DefaultCurrentProfile::new());
    let session = Arc::new(InMemorySession::new());
    let space_access: Arc<dyn SpaceAccessPort> = Arc::new(DefaultSpaceAccessAdapter::new(
        key_material,
        current_profile,
        Arc::clone(&session) as Arc<InMemorySession>,
    ));

    let device_identity: Arc<dyn DeviceIdentityPort> =
        Arc::new(FixedDeviceIdentity(device_id.clone()));

    let mut builder = IrohNodeBuilder::bind(
        &identity_store,
        IrohNodeConfig {
            rendezvous_base_url: Some(rendezvous_base_url),
            disable_relays: true,
            allow_overlay_network_addrs: false,
            custom_relay_urls: Vec::new(),
            ..Default::default()
        },
    )
    .await
    .expect("bind iroh node");
    let PairingHandlers {
        session: pairing_session,
        events: pairing_events,
        invitation: pairing_invitation,
        invitation_addresses: pairing_invitation_addresses,
        invitation_by_address: pairing_invitation_by_address,
    } = builder.install_pairing(
        Arc::clone(&device_identity),
        Arc::clone(&settings) as Arc<dyn SettingsPort>,
    );
    let presence: Arc<dyn uc_core::ports::PresencePort> = builder.install_presence(
        Arc::clone(&peer_addr_repo) as Arc<dyn uc_core::ports::PeerAddressRepositoryPort>,
        Arc::clone(&member_repo) as Arc<dyn MemberRepositoryPort>,
        Arc::new(Sha256IdentityFingerprintFactory),
        Arc::new(SystemClock) as Arc<dyn uc_core::ports::ClockPort>,
    );
    let iroh_node = builder.spawn();

    let proof_port: Arc<dyn ProofPort> = Arc::new(HmacProofAdapter::new_with_space_access(
        Arc::clone(&space_access),
    ));
    let local_identity: Arc<dyn LocalIdentityPort> = Arc::clone(&identity_store) as _;

    let (migration_state, key_migration, blob_migration_repo, blob_cipher) =
        common::migration_noop_deps();
    let facade = Arc::new(SpaceSetupFacade::new(SpaceSetupDeps {
        space_access,
        local_identity,
        device_identity,
        member_repo: Arc::clone(&member_repo) as Arc<dyn MemberRepositoryPort>,
        setup_status: Arc::clone(&setup_status) as Arc<dyn SetupStatusPort>,
        settings: Arc::clone(&settings) as Arc<dyn SettingsPort>,
        clock: Arc::new(SystemClock),
        pairing_invitation,
        pairing_invitation_addresses,
        pairing_invitation_by_address,
        pairing_session: Arc::clone(&pairing_session) as Arc<dyn PairingSessionPort>,
        pairing_events,
        proof_port,
        trusted_peer_repo: Arc::clone(&trusted_peer_repo) as Arc<dyn TrustedPeerRepositoryPort>,
        peer_addr_repo: Arc::clone(&peer_addr_repo)
            as Arc<dyn uc_core::ports::PeerAddressRepositoryPort>,
        presence,
        migration_state,
        key_migration,
        blob_migration_repo,
        blob_cipher,
        analytics: Arc::new(uc_observability::analytics::NoopAnalyticsFacade),
    }));

    Side {
        facade,
        iroh_node,
        member_repo,
        trusted_peer_repo,
        setup_status,
        device_id,
        _keystore_dir: keystore_dir,
    }
}

// ─── the test ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires LAN multicast — CI sandboxes block IP_ADD_MEMBERSHIP"]
async fn mdns_only_first_pair_succeeds_when_cloud_unreachable() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,uc_infra=debug".into()),
        )
        .with_test_writer()
        .try_init();

    // 1. Rendezvous mock that **always returns 503**. The sponsor adapter's
    //    `is_cloud_recoverable` covers `ServiceUnavailable`, so this drives
    //    the local-mint + mDNS-publisher branch. The joiner adapter's race
    //    in `resolve_invitation` sees the cloud branch fail and waits on
    //    the LAN branch — which is what the test is here to exercise.
    //
    //    `/v1/pairings/consume` also fails (consume is best-effort; the
    //    sponsor logs and moves on).
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/pairings"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/pairings/resolve"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/pairings/consume"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    // 2. Build sponsor + joiner against the failing rendezvous.
    let sponsor = build_side("sponsor", server.uri()).await;
    let joiner = build_side("joiner", server.uri()).await;

    // Give the sponsor inbound orchestrator a tick to install its event
    // subscription before the joiner dials. Same rationale as slice1.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 3. A1 · sponsor initialises the encrypted space.
    let passphrase = "hunter22hunter22";
    let init = sponsor
        .facade
        .initialize_space(InitializeSpaceInput {
            passphrase: passphrase.to_string(),
            passphrase_confirm: passphrase.to_string(),
            device_name: None,
        })
        .await
        .expect("sponsor A1");
    assert_eq!(init.self_device_id, sponsor.device_id);

    // 4. B1 · sponsor issues invitation. Rendezvous returns 503 → adapter
    //    must fall back to local mint. Code shape pins the alphabet so a
    //    future refactor that accidentally restores server-mint behaviour
    //    surfaces here as a shape failure rather than a silent regression.
    let invitation = sponsor
        .facade
        .issue_pairing_invitation()
        .await
        .expect("sponsor B1 (cloud-down fallback)");
    assert_eq!(
        invitation.code.as_str().len(),
        9,
        "local-mint code is XXXX-XXXX (8 chars + 1 hyphen), got {:?}",
        invitation.code.as_str()
    );
    let (left, right) = invitation
        .code
        .as_str()
        .split_once('-')
        .expect("code must contain a hyphen");
    assert_eq!(left.len(), 4, "left half must be 4 chars");
    assert_eq!(right.len(), 4, "right half must be 4 chars");
    // Crockford base32 alphabet: 0-9 + A-Z minus I, L, O, U.
    for ch in invitation.code.as_str().chars().filter(|c| *c != '-') {
        assert!(
            matches!(ch, '0'..='9' | 'A'..='H' | 'J' | 'K' | 'M' | 'N' | 'P'..='T' | 'V'..='Z'),
            "code char {ch:?} is not in the Crockford alphabet — server-mint regression?",
        );
    }

    // 5. B2 · joiner redeems. The race driver tries cloud (503 →
    //    ServiceUnavailable) and LAN (mDNS) in parallel; the LAN branch
    //    must win or the test fails with "InvitationNotFound" after both
    //    branches give up. Allow a generous wall-clock budget — mDNS
    //    cadence is ~2s in the publisher.
    let redeemed = tokio::time::timeout(
        Duration::from_secs(20),
        joiner
            .facade
            .redeem_pairing_invitation(RedeemPairingInvitationInput {
                code: invitation.code.as_str().to_string(),
                passphrase: passphrase.to_string(),
            }),
    )
    .await
    .expect("joiner B2 timed out — mDNS branch never matched")
    .expect("joiner B2 (LAN-only path) failed");

    assert_eq!(redeemed.sponsor_device_id, sponsor.device_id);
    assert_eq!(redeemed.self_device_id, joiner.device_id);
    assert_eq!(redeemed.sponsor_identity_fingerprint, init.fingerprint);

    // 6. Wait for the sponsor inbound orchestrator to persist the joiner
    //    side of the mirrored membership contract.
    wait_for(Duration::from_secs(5), || async {
        sponsor
            .member_repo
            .get(&joiner.device_id)
            .await
            .unwrap()
            .is_some()
    })
    .await;

    // 7. Mirrored persistence contract — identical to slice1's assertions
    //    because pairing's domain outcome should be the same regardless of
    //    which discovery channel resolved the ticket.
    let sponsor_members = sponsor.member_repo.list().await.unwrap();
    assert_eq!(sponsor_members.len(), 2);
    assert!(sponsor_members
        .iter()
        .any(|m| m.device_id == sponsor.device_id));
    assert!(sponsor_members
        .iter()
        .any(|m| m.device_id == joiner.device_id));
    assert!(sponsor
        .trusted_peer_repo
        .get(&joiner.device_id)
        .await
        .unwrap()
        .is_some());

    assert!(joiner
        .member_repo
        .get(&sponsor.device_id)
        .await
        .unwrap()
        .is_some());
    assert!(joiner
        .trusted_peer_repo
        .get(&sponsor.device_id)
        .await
        .unwrap()
        .is_some());
    assert!(
        joiner
            .setup_status
            .get_status()
            .await
            .unwrap()
            .has_completed
    );

    // 8. Clean teardown.
    sponsor.shutdown().await;
    joiner.shutdown().await;
}

async fn wait_for<F, Fut>(deadline: Duration, mut pred: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let start = std::time::Instant::now();
    loop {
        if pred().await {
            return;
        }
        if start.elapsed() >= deadline {
            panic!("wait_for timed out after {deadline:?}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
