//! Slice 1 end-to-end pairing handshake.
//!
//! Runs two fully-wired Slice 1 stacks (sponsor + joiner) against a real
//! iroh loopback transport and a [`wiremock`]-backed rendezvous server,
//! then drives A1 → B1 → B2 and asserts that both sides persist the new
//! peer as both `SpaceMember` and `TrustedPeer` (mirrored persistence is
//! the Slice 1 contract, F-053).
//!
//! The DI graph is assembled by hand here rather than going through
//! [`uc_bootstrap::wire_dependencies`] because the production path reaches
//! for keychain and a real SQLite pool; we replace those with in-memory
//! fakes and tempdir-backed `JsonKeySlotStore` instances so the test runs
//! hermetically on CI.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

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

// ─── wiremock respond handlers ──────────────────────────────────────────────

/// Shared slot holding the sponsor's `sponsorTicket` JSON between the POST
/// that registers it and the GET that echoes it back to the joiner.
type TicketVault = Arc<StdMutex<Option<String>>>;

struct PostPairings {
    vault: TicketVault,
    code: &'static str,
    expires_at_ms: i64,
}
impl Respond for PostPairings {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let body: serde_json::Value =
            serde_json::from_slice(&req.body).expect("POST /v1/pairings body must be JSON");
        let ticket = body["sponsorTicket"]
            .as_str()
            .expect("sponsorTicket missing")
            .to_string();
        *self.vault.lock().unwrap() = Some(ticket);
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "code": self.code,
            "expiresAtMs": self.expires_at_ms,
        }))
    }
}

struct GetPairing {
    vault: TicketVault,
    expires_at_ms: i64,
}
impl Respond for GetPairing {
    fn respond(&self, _req: &Request) -> ResponseTemplate {
        let ticket = self
            .vault
            .lock()
            .unwrap()
            .clone()
            .expect("joiner GET before sponsor POST: ticket vault empty");
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sponsorTicket": ticket,
            "sponsorEndpointId": "ignored",
            "expiresAtMs": self.expires_at_ms,
        }))
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
    _keystore_dir: TempDir, // kept alive for the JsonKeySlotStore path
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

    // Identity + iroh endpoint: one secret key, loaded by IrohIdentityStore
    // and reused to bind the endpoint so the on-wire identity matches the
    // fingerprint handed to domain code.
    let secure_storage: Arc<dyn SecureStoragePort> = Arc::new(InMemorySecureStorage::default());
    let identity_store = Arc::new(IrohIdentityStore::new(
        Arc::clone(&secure_storage),
        Arc::new(Sha256IdentityFingerprintFactory),
    ));

    // KeyMaterialStore: real JsonKeySlotStore rooted in a tempdir so Argon2
    // derivation produces matching keyslots across sponsor/joiner. Without
    // a real keyslot the proof adapter can't verify the joiner's response.
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

    // Iroh node: loopback only (tests must not depend on public relays).
    let mut builder = IrohNodeBuilder::bind(
        &identity_store,
        IrohNodeConfig {
            rendezvous_base_url: Some(rendezvous_base_url),
            disable_relays: true,
            allow_overlay_network_addrs: false,
        },
    )
    .await
    .expect("bind iroh node");
    let PairingHandlers {
        session: pairing_session,
        events: pairing_events,
        invitation: pairing_invitation,
    } = builder.install_pairing(
        Arc::clone(&device_identity),
        Arc::clone(&settings) as Arc<dyn SettingsPort>,
    );
    // Slice 2 Phase 1 · T8:presence handler 在同一 iroh 节点上 install,
    // 这个 e2e 不断言 presence 行为,但 F1 hook 现在 unconditionally 跑
    // `ensure_reachable_all.execute()`——需要一个真 presence port 才能
    // 通过 facade 构造。仍用 loopback iroh adapter,与 Slice 1 测试目标
    // 一致。
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
        analytics: Arc::new(uc_observability::analytics::NoopAnalyticsSink),
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

// ─── the actual E2E test ────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sponsor_joiner_end_to_end_pairing_persists_both_sides() {
    // Best-effort: initialise a test-local tracing subscriber so
    // `RUST_LOG=...` surfaces adapter / orchestrator logs during
    // diagnosis. `try_init` is a no-op if another test in the same
    // process already installed one.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_test_writer()
        .try_init();

    // 1. Rendezvous mock that captures sponsor ticket on POST and echoes
    //    it to the joiner on GET. This faithfully models the real service
    //    without bringing up an actual rendezvous deployment.
    let server = MockServer::start().await;
    let vault: TicketVault = Arc::new(StdMutex::new(None));
    const CODE: &str = "E2E0-A001";
    const EXPIRES_AT_MS: i64 = 1_900_000_000_000;

    Mock::given(method("POST"))
        .and(path("/v1/pairings"))
        .respond_with(PostPairings {
            vault: Arc::clone(&vault),
            code: CODE,
            expires_at_ms: EXPIRES_AT_MS,
        })
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/pairings/resolve"))
        .respond_with(GetPairing {
            vault: Arc::clone(&vault),
            expires_at_ms: EXPIRES_AT_MS,
        })
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/pairings/consume"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    // 2. Build both sides pointing at the same mock rendezvous.
    let sponsor = build_side("sponsor", server.uri()).await;
    let joiner = build_side("joiner", server.uri()).await;

    // 3. The sponsor-side inbound orchestrator spawns in
    //    `SpaceSetupFacade::new` and installs its subscriber via an async
    //    `subscribe().await` in the run loop. A brief yield gives the
    //    runtime enough time to reach that await before the joiner dials,
    //    avoiding the "event dropped: no subscriber installed" path in the
    //    iroh adapter. Production hits this naturally because a human
    //    types an invitation code between facade startup and the first
    //    inbound dial.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 4. A1 · sponsor creates the encrypted space.
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

    // 5. B1 · sponsor issues invitation → rendezvous returns the code.
    let invitation = sponsor
        .facade
        .issue_pairing_invitation()
        .await
        .expect("sponsor B1");
    assert_eq!(invitation.code.as_str(), CODE);

    // 6. B2 · joiner redeems — drives the full handshake over iroh.
    let redeemed = joiner
        .facade
        .redeem_pairing_invitation(RedeemPairingInvitationInput {
            code: invitation.code.as_str().to_string(),
            passphrase: passphrase.to_string(),
        })
        .await
        .expect("joiner B2");
    assert_eq!(redeemed.sponsor_device_id, sponsor.device_id);
    assert_eq!(redeemed.self_device_id, joiner.device_id);
    assert_eq!(redeemed.sponsor_identity_fingerprint, init.fingerprint);
    // Sponsor and joiner end up with *different* `space_id` values by
    // design — the sponsor handshake coordinator generates a fresh probe
    // id (sponsor_handshake.rs:`probe_space_id = SpaceId::new()`) rather
    // than threading the A1 id through. The keyslot is keyed off
    // `profile_id`, so this does not break crypto; both sides just carry
    // different local ids. The _unused_ bindings below make that
    // invariant explicit so a future refactor that tries to unify them
    // shows up as a test change rather than a silent behaviour shift.
    let _sponsor_a1_space_id = &init.space_id;
    let _joiner_handshake_space_id = &redeemed.space_id;

    // 7. Sponsor side also persists asynchronously via the inbound
    //    orchestrator; wait briefly for the final admit+trust to land
    //    before asserting. In production the UI subscribes to those
    //    events; here we poll the repo.
    wait_for(Duration::from_secs(2), || async {
        sponsor
            .member_repo
            .get(&joiner.device_id)
            .await
            .unwrap()
            .is_some()
    })
    .await;

    // 8. Mirrored persistence contract:
    //    ─ sponsor: owner SpaceMember + joiner SpaceMember + joiner TrustedPeer
    //    ─ joiner:  sponsor SpaceMember + sponsor TrustedPeer + setup_status done
    let sponsor_members = sponsor.member_repo.list().await.unwrap();
    assert_eq!(
        sponsor_members.len(),
        2,
        "sponsor should have its own + joiner SpaceMember"
    );
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
            .has_completed,
        "joiner setup should be marked complete after successful B2"
    );

    // 9. Clean teardown — neither side should hang.
    sponsor.shutdown().await;
    joiner.shutdown().await;
}

/// Poll `pred` on a 50 ms cadence until it returns true or the deadline
/// elapses; panics on timeout. Cheaper than plumbing a notification channel
/// through the orchestrator just for a test.
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
