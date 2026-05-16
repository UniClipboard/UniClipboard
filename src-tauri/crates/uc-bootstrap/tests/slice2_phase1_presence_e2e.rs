//! Slice 2 Phase 1 · T11 — presence lifecycle end-to-end.
//!
//! Runs two fully-wired stacks against a real iroh loopback transport and
//! a wiremock rendezvous (same harness shape as `slice1_handshake_e2e`),
//! pair them via A1 → B1 → B2, then assert the two core presence verdicts
//! from plan §1.1:
//!
//! 1. 配对成功后,两端 `MemberRosterFacade::list_with_presence` 报告对端
//!    `Online` + 本机 `is_local=true`(经 `facade.refresh_presence()` 显
//!    式 probe 一轮后)。
//! 2. 关闭 B 端 iroh 节点后,A 端在 **≤ 10s** 内通过 `list_with_presence`
//!    看见 B = `Offline`(`IrohPresenceAdapter` 的 `Connection::closed()`
//!    watchdog + 重 probe 组合路径)。
//!
//! ## 刻意未覆盖:"B 重启后再次 online" (plan §1.1 第三条)
//!
//! 要在 loopback-only 下可靠模拟"B 重新上线",joiner 必须用同一 iroh
//! 密钥再绑一次——但 pair 时写进 sponsor `peer_addr_repo` 的 blob 含老
//! socket(随机端口),新 bind 用新端口,直连拿不回去。生产依赖 relay
//! 透过 `NodeAddr` 刷新;测试里 `disable_relays: true` 关掉了这条路径,
//! 强行模拟只会验出假阳性。§1.1 第三条靠手动 / single-machine-e2e 覆盖。
//!
//! ## 代码重复
//!
//! 下面的 `InMemory*` fakes / wiremock respond 处理器 / `build_side` 与
//! `slice1_handshake_e2e.rs` 同构。整合进 `tests/common/mod.rs` 会同时
//! 改 slice1 测试导入面,对已绿测试有回归风险;这里保留 duplicate,单文
//! 件可独立读可独立调试。未来再出第三个 e2e 时统一抽取。

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use uc_application::facade::roster::{MemberRosterDeps, MemberRosterFacade};
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
    ClockPort, DeviceIdentityPort, LocalIdentityPort, PresencePort, ReachabilityState,
    SecureStorageError, SecureStoragePort, SettingsPort, SetupStatusPort,
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

// ─── in-memory fakes (duplicated from slice1_handshake_e2e.rs) ──────────────

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
    /// T11:`MemberRosterFacade` 是本测试的主要断言面——用它的
    /// `list_with_presence()` 打印/比对 roster。真实 bootstrap
    /// 在 `build_space_setup_assembly` 里同时构造二者;这里为保持与 slice1
    /// 测试同构,手工一起 new。
    roster: Arc<MemberRosterFacade>,
    iroh_node: IrohNode,
    member_repo: Arc<InMemoryMemberRepo>,
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
    let presence: Arc<dyn PresencePort> = builder.install_presence(
        Arc::clone(&peer_addr_repo) as Arc<dyn uc_core::ports::PeerAddressRepositoryPort>,
        Arc::clone(&member_repo) as Arc<dyn MemberRepositoryPort>,
        Arc::new(Sha256IdentityFingerprintFactory),
        Arc::new(SystemClock) as Arc<dyn ClockPort>,
    );
    let iroh_node = builder.spawn();

    let proof_port: Arc<dyn ProofPort> = Arc::new(HmacProofAdapter::new_with_space_access(
        Arc::clone(&space_access),
    ));
    let local_identity: Arc<dyn LocalIdentityPort> = Arc::clone(&identity_store) as _;

    // Clone the presence + local_identity handles before moving into SpaceSetupDeps
    // so MemberRosterDeps can reuse them. All three Arcs (member_repo, local_identity,
    // presence) are shared between the two facades — mirrors production wiring in
    // `build_space_setup_assembly` (`uc-bootstrap/src/space_setup.rs`).
    let presence_for_roster = Arc::clone(&presence);
    let local_identity_for_roster = Arc::clone(&local_identity);

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

    let roster = Arc::new(MemberRosterFacade::new(MemberRosterDeps {
        member_repo: Arc::clone(&member_repo) as Arc<dyn MemberRepositoryPort>,
        peer_addr_repo: Arc::clone(&peer_addr_repo)
            as Arc<dyn uc_core::ports::PeerAddressRepositoryPort>,
        trusted_peer_repo: Arc::clone(&trusted_peer_repo) as Arc<dyn TrustedPeerRepositoryPort>,
        local_identity: local_identity_for_roster,
        presence: presence_for_roster,
        connection_channel: None,
    }));

    Side {
        facade,
        roster,
        iroh_node,
        member_repo,
        device_id,
        _keystore_dir: keystore_dir,
    }
}

/// Drive the full Slice 1 A1 → B1 → B2 pairing flow. Returns once both
/// sides have persisted the peer (sponsor's inbound orchestrator writes
/// asynchronously, so we poll sponsor's member_repo before returning).
async fn pair_sponsor_and_joiner(sponsor: &Side, joiner: &Side, passphrase: &str) {
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

    let invitation = sponsor
        .facade
        .issue_pairing_invitation()
        .await
        .expect("sponsor B1");

    joiner
        .facade
        .redeem_pairing_invitation(RedeemPairingInvitationInput {
            code: invitation.code.as_str().to_string(),
            passphrase: passphrase.to_string(),
        })
        .await
        .expect("joiner B2");

    wait_for(Duration::from_secs(3), || async {
        sponsor
            .member_repo
            .get(&joiner.device_id)
            .await
            .unwrap()
            .is_some()
    })
    .await;
}

// ─── the actual E2E tests ───────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pair_then_refresh_reports_both_sides_online() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_test_writer()
        .try_init();

    let server = MockServer::start().await;
    let vault: TicketVault = Arc::new(StdMutex::new(None));
    const CODE: &str = "E2EP-R001";
    const EXPIRES_AT_MS: i64 = 1_900_000_000_000;

    mount_rendezvous(&server, &vault, CODE, EXPIRES_AT_MS).await;

    let sponsor = build_side("sponsor", server.uri()).await;
    let joiner = build_side("joiner", server.uri()).await;

    // Give the sponsor-side inbound orchestrator a tick to `.subscribe()`
    // before the joiner dials. See slice1_handshake_e2e.rs for the full
    // rationale; in production a human types the code between startup and
    // first dial so this race does not appear.
    tokio::time::sleep(Duration::from_millis(100)).await;

    pair_sponsor_and_joiner(&sponsor, &joiner, "hunter22hunter22").await;

    // Verdict 1a: sponsor refreshes + lists → sees joiner Online + self [local].
    let sponsor_report = sponsor
        .facade
        .refresh_presence()
        .await
        .expect("sponsor refresh_presence");
    assert_eq!(
        sponsor_report.total, 1,
        "sponsor peer_addr_repo should contain exactly joiner after pairing"
    );
    assert_eq!(
        sponsor_report.online, 1,
        "sponsor should probe joiner Online: report = {sponsor_report:?}"
    );
    assert!(sponsor_report.errors.is_empty(), "no probe errors expected");

    let sponsor_roster = sponsor.roster.list_with_presence().await.expect("list");
    assert_eq!(sponsor_roster.len(), 2, "sponsor roster has both members");
    let sponsor_self = sponsor_roster
        .iter()
        .find(|e| e.device_id == sponsor.device_id)
        .expect("sponsor's own entry");
    assert!(sponsor_self.is_local, "sponsor's own entry must be local");
    let sponsor_view_of_joiner = sponsor_roster
        .iter()
        .find(|e| e.device_id == joiner.device_id)
        .expect("joiner entry in sponsor's roster");
    assert!(!sponsor_view_of_joiner.is_local);
    assert_eq!(
        sponsor_view_of_joiner.state,
        ReachabilityState::Online,
        "sponsor should see joiner Online after refresh_presence"
    );

    // Verdict 1b: joiner refreshes + lists → sees sponsor Online.
    // (The joiner's B2 auto_start_network already fired ensure_reachable_all
    //  under the hood; refresh_presence is idempotent and makes the assertion
    //  deterministic rather than relying on that background hook's timing.)
    //
    // 已知 gap(Slice 1 行为,不在 T11 scope):joiner 的 `RedeemPairingInvitationUseCase`
    // 只 admit 了 sponsor,没把 joiner 自己 save 进 member_repo,所以 joiner
    // 视角下 roster 里只有 1 条(sponsor)。对应的 UX 修复(让 `members`
    // 命令在 joiner 侧也能看到自己)属于 Slice 1/2 清洁化 follow-up——
    // 这里的 assertion 严格反映当前行为,别人改修 gap 时测试会失败,
    // 作为契约变更的信号。
    let joiner_report = joiner
        .facade
        .refresh_presence()
        .await
        .expect("joiner refresh_presence");
    assert_eq!(joiner_report.total, 1);
    assert_eq!(joiner_report.online, 1);

    let joiner_roster = joiner.roster.list_with_presence().await.expect("list");
    assert_eq!(
        joiner_roster.len(),
        1,
        "joiner roster currently contains only sponsor (B2 does not save \
         self as SpaceMember — pre-existing Slice 1 gap, see comment above)"
    );
    let joiner_view_of_sponsor = &joiner_roster[0];
    assert_eq!(joiner_view_of_sponsor.device_id, sponsor.device_id);
    assert!(
        !joiner_view_of_sponsor.is_local,
        "sponsor entry in joiner's roster must not be is_local"
    );
    assert_eq!(joiner_view_of_sponsor.state, ReachabilityState::Online);

    sponsor.shutdown().await;
    joiner.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn joiner_shutdown_flips_sponsor_roster_to_offline_within_10s() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_test_writer()
        .try_init();

    let server = MockServer::start().await;
    let vault: TicketVault = Arc::new(StdMutex::new(None));
    const CODE: &str = "E2EP-OFF1";
    const EXPIRES_AT_MS: i64 = 1_900_000_000_000;

    mount_rendezvous(&server, &vault, CODE, EXPIRES_AT_MS).await;

    let sponsor = build_side("sponsor", server.uri()).await;
    let joiner = build_side("joiner", server.uri()).await;

    tokio::time::sleep(Duration::from_millis(100)).await;
    pair_sponsor_and_joiner(&sponsor, &joiner, "hunter22hunter22").await;

    // Prime sponsor's presence cache + establish Connection so the
    // `IrohPresenceAdapter` watchdog is installed on this Connection.
    // Without this probe, sponsor's cache starts at `Unknown` and no
    // watchdog exists — shutting joiner down would have nothing to
    // observe.
    let pre_report = sponsor
        .facade
        .refresh_presence()
        .await
        .expect("sponsor initial refresh");
    assert_eq!(
        pre_report.online, 1,
        "sanity: sponsor must see joiner Online before teardown"
    );

    let joiner_device_id = joiner.device_id.clone();

    // Remember sponsor's references before moving joiner into shutdown.
    let sponsor_roster = Arc::clone(&sponsor.roster);
    let sponsor_facade = Arc::clone(&sponsor.facade);

    // B goes down — fully tear down joiner's iroh node so `Connection::closed()`
    // fires on sponsor's side.
    joiner.shutdown().await;

    // Plan §1.1 验收:≤ 10s 内 sponsor.list_with_presence 反映 joiner Offline。
    // 实际 `IrohPresenceAdapter` 的 watchdog 在 peer 关连接时 ~100ms 内写缓
    // 存,这里给 10s ceiling 以覆盖 CI 抖动 + 调度延迟。
    //
    // 同时并跑一轮主动 `refresh_presence` 帮助推进——watchdog 是惰性的,
    // 如果 sponsor 一直没有新事件来,cache 里仍是 Online;主动重 probe 对
    // 已死对端会 dial 失败 → 显式写 Offline。两条路任一先到算通过。
    wait_for(Duration::from_secs(10), || {
        let roster = Arc::clone(&sponsor_roster);
        let facade = Arc::clone(&sponsor_facade);
        let joiner_device_id = joiner_device_id.clone();
        async move {
            // Kick a refresh without failing on error — dial to dead peer
            // may surface as a per-peer error in the report; we don't care
            // which mechanism flips the state.
            let _ = facade.refresh_presence().await;
            let entries = match roster.list_with_presence().await {
                Ok(v) => v,
                Err(_) => return false,
            };
            entries
                .iter()
                .find(|e| e.device_id == joiner_device_id)
                .map(|e| e.state != ReachabilityState::Online)
                .unwrap_or(false)
        }
    })
    .await;

    // Final assertion: snapshot must read Offline (or Unknown — both mean
    // "not Online", which is the user-visible contract per plan §1.1).
    let final_entries = sponsor_roster
        .list_with_presence()
        .await
        .expect("final list");
    let final_view = final_entries
        .iter()
        .find(|e| e.device_id == joiner_device_id)
        .expect("joiner still listed (membership persists)");
    assert_ne!(
        final_view.state,
        ReachabilityState::Online,
        "sponsor should no longer view joiner as Online"
    );

    sponsor.shutdown().await;
}

// ─── test helpers ───────────────────────────────────────────────────────────

async fn mount_rendezvous(
    server: &MockServer,
    vault: &TicketVault,
    code: &'static str,
    expires_at_ms: i64,
) {
    Mock::given(method("POST"))
        .and(path("/v1/pairings"))
        .respond_with(PostPairings {
            vault: Arc::clone(vault),
            code,
            expires_at_ms,
        })
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/pairings/resolve"))
        .respond_with(GetPairing {
            vault: Arc::clone(vault),
            expires_at_ms,
        })
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/pairings/consume"))
        .respond_with(ResponseTemplate::new(204))
        .mount(server)
        .await;
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
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
