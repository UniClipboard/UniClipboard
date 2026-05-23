//! Slice 2 Phase 2 · T12 — clipboard sync end-to-end.
//!
//! Pairs two fully-wired stacks over a real iroh loopback transport (with
//! presence + clipboard ALPNs both active), then drives the Phase 2 sync
//! contract from plan §1 / §15 acceptance criteria:
//!
//! 1. A copies text → ≤ 2s B sees the same plaintext + matching
//!    `content_hash` via `ClipboardSyncFacade::subscribe_inbound_notices`.
//! 2. Repeating the same dispatch a second time still Accepts on the wire
//!    (Phase 2's receiver adapter does not dedup; the ingest use case
//!    only re-broadcasts decrypted plaintext) — the `DuplicateIgnored`
//!    code path lives on the wire but no Phase 2 producer emits it.
//!    Phase 3 lands receiver-side dedup on top of `ClipboardEventWriter`.
//!
//! ## 已知 Slice 1 gap (再现自 phase1 e2e)
//!
//! `RedeemPairingInvitationUseCase` 不把 joiner 自己 save 进 member_repo,
//! 所以 A→B dispatch 时 sponsor 的 member_repo 里能看到 joiner 的
//! `SpaceMember`,但 joiner 的 member_repo 只看见 sponsor。这影响 verdict
//! 2 的对称性测试(B→A dispatch),所以本文件只测 A→B 单向 — 完整双向
//! 在 Slice 1 follow-up 之后再加。
//!
//! ## 与 phase1 e2e 的代码重复
//!
//! 同样的 InMemory* fakes + wiremock 处理器 + build_side 体写两遍。原
//! 因详见 `slice2_phase1_presence_e2e.rs` 文件首注:整合 `tests/common/`
//! 会牵动已绿测试导入面;留待 Phase 2 收尾后统一抽取。

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use uc_application::decode_v3_bytes_to_snapshot;
use uc_application::facade::space_setup::{
    InitializeSpaceInput, RedeemPairingInvitationInput, SpaceSetupDeps, SpaceSetupFacade,
};
use uc_application::facade::{
    ClipboardSyncDeps, ClipboardSyncFacade, IngestHandle, MemberRosterDeps, MemberRosterFacade,
};
use uc_application::proof::HmacProofAdapter;
use uc_bootstrap::IrohNodeConfig;
use uc_core::ids::{DeviceId, FormatId, RepresentationId};
use uc_core::membership::{MemberRepositoryPort, MembershipError, SpaceMember};
use uc_core::ports::pairing::PairingSessionPort;
use uc_core::ports::security::TransferCipherPort;
use uc_core::ports::space::{ProofPort, SpaceAccessPort};
use uc_core::ports::{
    ClipboardDispatchPort, ClipboardReceiverPort, ClockPort, DeviceIdentityPort,
    FirstSyncStateError, FirstSyncStatePort, LocalIdentityPort, PresencePort, SecureStorageError,
    SecureStoragePort, SettingsPort, SetupStatusPort,
};
use uc_core::settings::model::Settings;
use uc_core::setup::SetupStatus;
use uc_core::trusted_peer::{TrustedPeer, TrustedPeerError, TrustedPeerRepositoryPort};
use uc_core::{
    ClipboardChangeOrigin, MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot,
};

use uc_infra::clipboard::TransferCipherAdapter;
use uc_infra::fs::key_slot_store::JsonKeySlotStore;
use uc_infra::network::iroh::{
    ClipboardHandlers, IrohIdentityStore, IrohNode, IrohNodeBuilder, PairingHandlers,
};
use uc_infra::security::{
    DefaultCurrentProfile, DefaultSpaceAccessAdapter, InMemorySession, KeyMaterialStore,
    Sha256IdentityFingerprintFactory,
};

/// Slice 8c-2 · e2e 不依赖磁盘 first-sync 状态——所有 `mark_*` 直接返回
/// `Ok(false)`，dispatch use case 不会 fire 任何 `first_*` 事件。
struct NoopFirstSyncState;
#[async_trait::async_trait]
impl FirstSyncStatePort for NoopFirstSyncState {
    async fn mark_first_sync_attempted(&self) -> Result<bool, FirstSyncStateError> {
        Ok(false)
    }
    async fn mark_first_sync_succeeded(&self) -> Result<bool, FirstSyncStateError> {
        Ok(false)
    }
    async fn mark_first_file_sync_succeeded(&self) -> Result<bool, FirstSyncStateError> {
        Ok(false)
    }
}

// ─── in-memory fakes (duplicated from slice2_phase1_presence_e2e.rs) ────────

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

/// e2e 不验证投递持久化,给一组最小 noop adapter 让 facade 装配通过。
#[derive(Default)]
struct NoopEntryDeliveryRepo;
#[async_trait]
impl uc_core::ports::EntryDeliveryRepositoryPort for NoopEntryDeliveryRepo {
    async fn record_attempt(
        &self,
        _record: &uc_core::clipboard::EntryDeliveryRecord,
    ) -> Result<(), uc_core::clipboard::EntryDeliveryError> {
        Ok(())
    }
    async fn list_by_entry(
        &self,
        _entry_id: &uc_core::ids::EntryId,
    ) -> Result<Vec<uc_core::clipboard::EntryDeliveryRecord>, uc_core::clipboard::EntryDeliveryError>
    {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct NoopClipboardEntryRepo;
#[async_trait]
impl uc_core::ports::ClipboardEntryRepositoryPort for NoopClipboardEntryRepo {
    async fn save_entry_and_selection(
        &self,
        _entry: &uc_core::clipboard::ClipboardEntry,
        _selection: &uc_core::ClipboardSelectionDecision,
    ) -> anyhow::Result<()> {
        Ok(())
    }
    async fn get_entry(
        &self,
        _entry_id: &uc_core::ids::EntryId,
    ) -> anyhow::Result<Option<uc_core::clipboard::ClipboardEntry>> {
        Ok(None)
    }
    async fn list_entries(
        &self,
        _limit: usize,
        _offset: usize,
    ) -> anyhow::Result<Vec<uc_core::clipboard::ClipboardEntry>> {
        Ok(Vec::new())
    }
    async fn delete_entry(&self, _entry_id: &uc_core::ids::EntryId) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct NoopClipboardEventRepo;
#[async_trait]
impl uc_core::ports::ClipboardEventRepositoryPort for NoopClipboardEventRepo {
    async fn get_representation(
        &self,
        _id: &uc_core::ids::EventId,
        _representation_id: &str,
    ) -> anyhow::Result<uc_core::ObservedClipboardRepresentation> {
        anyhow::bail!("unused in e2e tests")
    }
    async fn get_source_device(
        &self,
        _event_id: &uc_core::ids::EventId,
    ) -> anyhow::Result<Option<uc_core::ids::DeviceId>> {
        Ok(None)
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
    /// Cloned for `refresh_presence` reuse — phase 2 dispatch needs a
    /// non-`Unknown` cache or it skips the target. Kept as a field
    /// (prefixed `_`) so the parallel structure with phase 1 e2e is
    /// obvious; future verdicts can drop the prefix.
    _roster: Arc<MemberRosterFacade>,
    clipboard_sync: Arc<ClipboardSyncFacade>,
    /// Held to keep the spawned ingest loop alive for the duration of
    /// the test. Drop aborts (Phase 2 · T8 contract).
    _ingest: IngestHandle,
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
        Arc::clone(&session),
    ));

    // Same `InMemorySession` powers both sides of the cipher: pairing copies
    // the master key into both sessions, then the V3 chunked AEAD adapter
    // uses that key to seal/open clipboard payloads on the wire.
    let transfer_cipher: Arc<dyn TransferCipherPort> =
        Arc::new(TransferCipherAdapter::new(Arc::clone(&session)));

    let device_identity: Arc<dyn DeviceIdentityPort> =
        Arc::new(FixedDeviceIdentity(device_id.clone()));

    let mut builder = IrohNodeBuilder::bind(
        &identity_store,
        IrohNodeConfig {
            rendezvous_base_url: Some(rendezvous_base_url),
            disable_relays: true,
            allow_overlay_network_addrs: false,
            custom_relay_urls: Vec::new(),
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
        Arc::new(SystemClock),
    );
    let ClipboardHandlers {
        dispatch: clipboard_dispatch,
        receiver: clipboard_receiver,
    } = builder.install_clipboard(
        Arc::clone(&peer_addr_repo) as Arc<dyn uc_core::ports::PeerAddressRepositoryPort>,
        Arc::clone(&member_repo) as Arc<dyn MemberRepositoryPort>,
        Arc::new(Sha256IdentityFingerprintFactory),
    );
    let clipboard_dispatch: Arc<dyn ClipboardDispatchPort> = clipboard_dispatch;
    let clipboard_receiver: Arc<dyn ClipboardReceiverPort> = clipboard_receiver;
    let iroh_node = builder.spawn();

    let proof_port: Arc<dyn ProofPort> = Arc::new(HmacProofAdapter::new_with_space_access(
        Arc::clone(&space_access),
    ));
    let local_identity: Arc<dyn LocalIdentityPort> = Arc::clone(&identity_store) as _;

    // Clone the presence + local_identity handles before SpaceSetupDeps moves
    // them so MemberRosterFacade + ClipboardSyncFacade can share the same
    // instances. Mirrors production wiring in `build_space_setup_assembly`.
    let presence_for_roster = Arc::clone(&presence);
    let presence_for_clipboard = Arc::clone(&presence);
    let local_identity_for_roster = Arc::clone(&local_identity);
    let local_identity_for_clipboard = Arc::clone(&local_identity);
    let device_identity_for_clipboard = Arc::clone(&device_identity);
    let settings_for_clipboard = Arc::clone(&settings) as Arc<dyn SettingsPort>;

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

    let clipboard_sync = Arc::new(ClipboardSyncFacade::new(ClipboardSyncDeps {
        peer_addr_repo: Arc::clone(&peer_addr_repo)
            as Arc<dyn uc_core::ports::PeerAddressRepositoryPort>,
        member_repo: Arc::clone(&member_repo) as Arc<dyn MemberRepositoryPort>,
        presence: presence_for_clipboard,
        transfer_cipher,
        clipboard_dispatch,
        clipboard_receiver,
        device_identity: device_identity_for_clipboard,
        local_identity: local_identity_for_clipboard,
        settings: settings_for_clipboard,
        clock: Arc::new(SystemClock),
        analytics: Arc::new(uc_observability::analytics::NoopAnalyticsSink),
        // Slice 8c-2 · e2e 不验证 first-sync 漏斗事件——给 noop fake 让
        // mark_* 永远返回 Ok(false)，避免依赖 file-system 路径状态。
        first_sync_state: Arc::new(NoopFirstSyncState),
        entry_delivery_repo: Arc::new(NoopEntryDeliveryRepo),
        entry_repo: Arc::new(NoopClipboardEntryRepo),
        event_repo: Arc::new(NoopClipboardEventRepo),
        trusted_peer_repo: Arc::clone(&trusted_peer_repo) as Arc<dyn TrustedPeerRepositoryPort>,
        // e2e 路径不需要前端事件 —— 装一根空 host_event_bus,无下游 emitter
        // 注册,emit_or_warn 直接走完空 fan-out,行为与改动前完全一致。
        host_event_bus: Arc::new(uc_application::facade::HostEventBus::new()),
    }));
    let ingest_handle = clipboard_sync.spawn_ingest_loop();

    Side {
        facade,
        _roster: roster,
        clipboard_sync,
        _ingest: ingest_handle,
        iroh_node,
        member_repo,
        device_id,
        _keystore_dir: keystore_dir,
    }
}

/// Drive A1 → B1 → B2 to completion, then refresh both sides' presence
/// caches so the dispatch use case sees the peer as `Online` (not
/// `Unknown`, which would skip it on the Phase 2 send path).
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

    // Prime presence on both sides so dispatch's Online filter passes.
    let _ = sponsor.facade.refresh_presence().await;
    let _ = joiner.facade.refresh_presence().await;
}

// ─── the actual E2E tests ───────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sponsor_dispatch_lands_on_joiner_within_2s() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_test_writer()
        .try_init();

    let server = MockServer::start().await;
    let vault: TicketVault = Arc::new(StdMutex::new(None));
    const CODE: &str = "E2EP-CL01";
    const EXPIRES_AT_MS: i64 = 1_900_000_000_000;

    mount_rendezvous(&server, &vault, CODE, EXPIRES_AT_MS).await;

    let sponsor = build_side("sponsor", server.uri()).await;
    let joiner = build_side("joiner", server.uri()).await;

    // Same race window the phase 1 e2e documents — give the sponsor's
    // pairing inbound orchestrator a tick to subscribe before the joiner
    // dials.
    tokio::time::sleep(Duration::from_millis(100)).await;

    pair_sponsor_and_joiner(&sponsor, &joiner, "hunter22hunter22").await;

    // Subscribe BEFORE dispatch so the relay task (spawned per
    // `subscribe_inbound_notices` call inside the facade) has a live
    // public sender by the time the inbound notice arrives.
    let mut joiner_notices = joiner.clipboard_sync.subscribe_inbound_notices();

    // Phase 3 upgrade(T11):use `dispatch_snapshot` — matches the
    // daemon wire format. Inbound notice's plaintext is a V3 envelope,
    // so we decode it and check the round-tripped representation.
    let text = "hello clipboard sync";
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1_700_000_000_000,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("text"),
            Some(MimeType("text/plain".to_string())),
            text.as_bytes().to_vec(),
        )],
    };
    let expected_hash = snapshot.snapshot_hash().to_string();
    let outcome = sponsor
        .clipboard_sync
        .dispatch_snapshot(snapshot, ClipboardChangeOrigin::LocalCapture, None, None)
        .await
        .expect("sponsor dispatch ok");
    assert_eq!(
        outcome.total_accepted, 1,
        "exactly one accepted ack expected (joiner is the only paired peer): outcome = {outcome:?}"
    );
    assert_eq!(
        outcome.per_target.len(),
        1,
        "per_target must list the single online peer"
    );
    assert_eq!(
        outcome.per_target[0].device_id, joiner.device_id,
        "the per_target entry must be the joiner"
    );

    // Plan §1 contract: ≤ 2s. Test wall here is 5s ceiling for CI jitter.
    let notice = tokio::time::timeout(Duration::from_secs(5), joiner_notices.recv())
        .await
        .expect("inbound notice arrives within 5s")
        .expect("notice broadcast still has a sender");
    assert_eq!(
        notice.from_device, sponsor.device_id,
        "notice must report sponsor as origin"
    );
    assert_eq!(
        notice.content_hash, expected_hash,
        "content_hash (canonical snapshot_hash) must round-trip unchanged"
    );

    // Decode the V3 envelope and check the text representation round-tripped.
    let decoded = decode_v3_bytes_to_snapshot(&notice.plaintext)
        .expect("notice plaintext must decode as V3 envelope");
    assert_eq!(decoded.representations.len(), 1);
    assert_eq!(
        decoded.representations[0].expect_inline_bytes(),
        text.as_bytes()
    );
    assert_eq!(
        decoded.representations[0].mime.as_ref().map(|m| m.as_str()),
        Some("text/plain")
    );

    sponsor.shutdown().await;
    joiner.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_dispatch_lands_twice_phase2_no_dedup() {
    // Phase 2 receiver does NOT dedup — adapter always acks Accepted on
    // success; only `clipboard_dispatch_adapter`'s wire layer can return
    // `DuplicateIgnored`, and no Phase 2 producer emits that ack code.
    // This test pins the actual Phase 2 behaviour: two identical
    // dispatches both succeed end-to-end and the joiner sees both
    // notices. Phase 3 will introduce receiver-side dedup once the
    // ingest path persists to `ClipboardEventWriter` — at that point
    // this test should be flipped to expect `DuplicateIgnored` on the
    // second attempt and adjusted accordingly.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_test_writer()
        .try_init();

    let server = MockServer::start().await;
    let vault: TicketVault = Arc::new(StdMutex::new(None));
    const CODE: &str = "E2EP-CL02";
    const EXPIRES_AT_MS: i64 = 1_900_000_000_000;

    mount_rendezvous(&server, &vault, CODE, EXPIRES_AT_MS).await;

    let sponsor = build_side("sponsor", server.uri()).await;
    let joiner = build_side("joiner", server.uri()).await;

    tokio::time::sleep(Duration::from_millis(100)).await;
    pair_sponsor_and_joiner(&sponsor, &joiner, "hunter22hunter22").await;

    let mut joiner_notices = joiner.clipboard_sync.subscribe_inbound_notices();

    // Phase 3 upgrade(T11):same fixture built twice — `snapshot_hash`
    // is deterministic so both sends produce identical `content_hash`.
    // At the wire/facade level, Phase 2 behaviour holds: receiver adapter
    // doesn't dedup, so both sends get Accepted. Phase 3 dedup(in
    // `ApplyInboundClipboardUseCase`) is a daemon concern, NOT exercised
    // by this raw facade-level e2e — that belongs in the T12 daemon e2e.
    let fixture_text = "duplicate fixture text";
    let build_snapshot = || SystemClipboardSnapshot {
        ts_ms: 1_700_000_000_000,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("text"),
            Some(MimeType("text/plain".to_string())),
            fixture_text.as_bytes().to_vec(),
        )],
    };
    let canonical_hash = build_snapshot().snapshot_hash().to_string();
    for attempt in 0..2 {
        let outcome = sponsor
            .clipboard_sync
            .dispatch_snapshot(
                build_snapshot(),
                ClipboardChangeOrigin::LocalCapture,
                None,
                None,
            )
            .await
            .unwrap_or_else(|e| panic!("attempt {attempt} dispatch must succeed: {e:?}"));
        assert_eq!(
            outcome.total_accepted, 1,
            "Phase 2 has no dedup — attempt {attempt} must Accept; outcome = {outcome:?}"
        );
        assert_eq!(
            outcome.total_duplicate, 0,
            "no Phase 2 producer returns DuplicateIgnored on the wire"
        );
        assert_eq!(
            outcome.content_hash, canonical_hash,
            "snapshot_hash should be deterministic across identical snapshots"
        );
    }

    let mut received = Vec::with_capacity(2);
    for _ in 0..2 {
        let notice = tokio::time::timeout(Duration::from_secs(5), joiner_notices.recv())
            .await
            .expect("inbound notice arrives within 5s")
            .expect("notice broadcast still has a sender");
        received.push(notice);
    }
    assert!(
        received.iter().all(|n| n.content_hash == canonical_hash),
        "both notices must carry identical canonical content_hash; got {received:?}"
    );
    for notice in &received {
        let decoded = decode_v3_bytes_to_snapshot(&notice.plaintext)
            .expect("notice plaintext must decode as V3 envelope");
        assert_eq!(
            decoded.representations[0].expect_inline_bytes(),
            fixture_text.as_bytes()
        );
    }

    sponsor.shutdown().await;
    joiner.shutdown().await;
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
