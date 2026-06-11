//! `SwitchSpaceUseCase` 单元测试。
//!
//! 所有 9 个端口都用 `mockall` 替身——switch-space 是跨多 port 的复杂流程，
//! 用 mockall 才能精准断言"调用次数 / 调用顺序 / 参数 shape"。`Sequence`
//! 没用上：阶段顺序由 use case 流式调用保证，断言上"是否被调到"+"被调
//! 几次"+按 phase 分组的参数 shape 已足够覆盖回归。

use super::*;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use mockall::predicate;
use uc_core::ports::pairing::DiscoveryChannel;

use uc_core::ids::{DeviceId, EventId, RepresentationId};
use uc_core::membership::{MembershipError, SpaceMember};
use uc_core::pairing::invitation::InvitationCode;
use uc_core::ports::PeerAddressError;
use uc_core::security::IdentityFingerprint;
use uc_core::setup::SetupStatus;
use uc_core::trusted_peer::{TrustedPeer, TrustedPeerError};
use uc_observability::analytics::{
    AnalyticsFacade, NoopAnalyticsFacade, ResetIdentityError, SelfMintedAdoptRequest,
};
use uuid::Uuid;

// ── 端口替身（mockall）──────────────────────────────────────────────────

mockall::mock! {
    pub SetupStatusRepo {}

    #[async_trait]
    impl SetupStatusPort for SetupStatusRepo {
        async fn get_status(&self) -> anyhow::Result<SetupStatus>;
        async fn set_status(&self, status: &SetupStatus) -> anyhow::Result<()>;
    }
}

mockall::mock! {
    pub MigrationStateRepo {}

    #[async_trait]
    impl MigrationStatePort for MigrationStateRepo {
        async fn get_current(&self) -> Result<Option<MigrationPhase>, MigrationStateError>;
        async fn set_current(
            &self,
            phase: Option<MigrationPhase>,
        ) -> Result<(), MigrationStateError>;
    }
}

mockall::mock! {
    pub KeyMigration {}

    #[async_trait]
    impl KeyMigrationPort for KeyMigration {
        async fn prepare_migration_key(&self) -> Result<MigrationRunId, KeyMigrationError>;
        async fn encrypt_with_migration_key(
            &self,
            run_id: &MigrationRunId,
            plaintext: &uc_core::crypto::domain::Plaintext,
            aad: &Aad,
        ) -> Result<Ciphertext, KeyMigrationError>;
        async fn decrypt_with_migration_key(
            &self,
            run_id: &MigrationRunId,
            ciphertext: &Ciphertext,
            aad: &Aad,
        ) -> Result<uc_core::crypto::domain::Plaintext, KeyMigrationError>;
        async fn discard_migration_key(
            &self,
            run_id: &MigrationRunId,
        ) -> Result<(), KeyMigrationError>;
    }
}

mockall::mock! {
    pub BlobMigrationRepo {}

    #[async_trait]
    impl BlobMigrationRepoPort for BlobMigrationRepo {
        async fn list_main_inline_representations(
            &self,
        ) -> Result<Vec<(EventId, RepresentationId)>, BlobMigrationRepoError>;
        async fn read_main_inline_data(
            &self,
            event_id: &EventId,
            representation_id: &RepresentationId,
        ) -> Result<Option<Vec<u8>>, BlobMigrationRepoError>;
        async fn upsert_record(
            &self,
            record: &MigrationRecord,
        ) -> Result<(), BlobMigrationRepoError>;
        async fn count_records(&self) -> Result<u64, BlobMigrationRepoError>;
        async fn list_records(&self) -> Result<Vec<MigrationRecord>, BlobMigrationRepoError>;
        async fn update_main_inline_data(
            &self,
            event_id: &EventId,
            representation_id: &RepresentationId,
            new_ciphertext: &[u8],
        ) -> Result<(), BlobMigrationRepoError>;
        async fn discard_all_records(&self) -> Result<(), BlobMigrationRepoError>;
    }
}

mockall::mock! {
    pub BlobCipher {}

    #[async_trait]
    impl BlobCipherPort for BlobCipher {
        async fn encrypt(
            &self,
            space: &ActiveSpace,
            plaintext: &uc_core::crypto::domain::Plaintext,
            aad: &Aad,
        ) -> Result<Ciphertext, BlobCipherError>;
        async fn decrypt(
            &self,
            space: &ActiveSpace,
            ciphertext: &Ciphertext,
            aad: &Aad,
        ) -> Result<uc_core::crypto::domain::Plaintext, BlobCipherError>;
    }
}

mockall::mock! {
    pub Handshake {}

    #[async_trait]
    impl JoinerHandshakeRunner for Handshake {
        async fn run(
            &self,
            code: &InvitationCode,
            passphrase: &Passphrase,
        ) -> Result<JoinerHandshakeOutcome, RedeemPairingInvitationError>;
    }
}

mockall::mock! {
    pub MemberRepo {}

    #[async_trait]
    impl MemberRepositoryPort for MemberRepo {
        async fn get(
            &self,
            device_id: &DeviceId,
        ) -> Result<Option<SpaceMember>, MembershipError>;
        async fn list(&self) -> Result<Vec<SpaceMember>, MembershipError>;
        async fn save(&self, member: &SpaceMember) -> Result<(), MembershipError>;
        async fn remove(&self, device_id: &DeviceId) -> Result<bool, MembershipError>;
    }
}

mockall::mock! {
    pub TrustRepo {}

    #[async_trait]
    impl TrustedPeerRepositoryPort for TrustRepo {
        async fn get(
            &self,
            device_id: &DeviceId,
        ) -> Result<Option<TrustedPeer>, TrustedPeerError>;
        async fn list(&self) -> Result<Vec<TrustedPeer>, TrustedPeerError>;
        async fn save(&self, peer: &TrustedPeer) -> Result<(), TrustedPeerError>;
        async fn remove(&self, device_id: &DeviceId) -> Result<bool, TrustedPeerError>;
    }
}

mockall::mock! {
    pub PeerAddrRepo {}

    #[async_trait]
    impl PeerAddressRepositoryPort for PeerAddrRepo {
        async fn get(
            &self,
            device: &DeviceId,
        ) -> Result<Option<PeerAddressRecord>, PeerAddressError>;
        async fn upsert(&self, record: &PeerAddressRecord) -> Result<(), PeerAddressError>;
        async fn list(&self) -> Result<Vec<PeerAddressRecord>, PeerAddressError>;
        async fn remove(&self, device: &DeviceId) -> Result<(), PeerAddressError>;
    }
}

struct FixedClock(i64);
impl ClockPort for FixedClock {
    fn now_ms(&self) -> i64 {
        self.0
    }
}

/// Records analytics-facade invocations so tests can assert *which*
/// identity transition fired and how often. Other methods are inert.
#[derive(Default)]
struct RecordingAnalyticsFacade {
    adopted: Mutex<Vec<Uuid>>,
    released: Mutex<u32>,
}

impl RecordingAnalyticsFacade {
    fn adopted(&self) -> Vec<Uuid> {
        self.adopted.lock().unwrap().clone()
    }
    fn released_count(&self) -> u32 {
        *self.released.lock().unwrap()
    }
}

impl AnalyticsFacade for RecordingAnalyticsFacade {
    fn capture(&self, _: uc_observability::analytics::events::Event) {}
    fn adopt_self_minted(&self, _: SelfMintedAdoptRequest) {}
    fn adopt_from_sponsor(&self, space_person_id: Uuid) {
        self.adopted.lock().unwrap().push(space_person_id);
    }
    fn release_to_solo(&self) {
        *self.released.lock().unwrap() += 1;
    }
    fn reset_identity(&self) -> Result<(), ResetIdentityError> {
        Ok(())
    }
    fn current_space_person_id(&self) -> Option<Uuid> {
        None
    }
}

// ── 测试 fixtures ──────────────────────────────────────────────────────

fn fp_local() -> IdentityFingerprint {
    IdentityFingerprint::from_raw_string("AAAAAAAAAAAAAAAA").unwrap()
}
fn fp_sponsor() -> IdentityFingerprint {
    IdentityFingerprint::from_raw_string("BBBBBBBBBBBBBBBB").unwrap()
}
fn run_id() -> MigrationRunId {
    MigrationRunId::new("mig-test-run-1")
}
fn target_space() -> SpaceId {
    SpaceId::from_str("new-space")
}
fn outcome_default() -> JoinerHandshakeOutcome {
    JoinerHandshakeOutcome {
        sponsor_device_id: DeviceId::new("sponsor-device"),
        sponsor_device_name: "sponsor-laptop".into(),
        sponsor_identity_fingerprint: fp_sponsor(),
        space_id: target_space(),
        self_device_id: DeviceId::new("local-device"),
        self_identity_fingerprint: fp_local(),
        discovery_channel: DiscoveryChannel::Cloud,
        sponsor_transport_address_blob: vec![],
        // Phase 098 默认 None：switch_space tests 关注迁移流程而非 person
        // 切换；PR 8 才接 switch_space 的 identify。
        sponsor_space_person_id: None,
    }
}
fn already_setup() -> SetupStatus {
    SetupStatus {
        has_completed: true,
        space_id: Some(SpaceId::from_str("old-space")),
    }
}
fn cmd_default() -> SwitchSpaceCommand {
    SwitchSpaceCommand {
        code: InvitationCode::new("CODE-1"),
        new_passphrase: Passphrase::new("hunter22hunter22"),
    }
}

/// 把 9 个 mock 装配成 `SwitchSpaceUseCase` 的 builder。
struct Env {
    setup_status: MockSetupStatusRepo,
    migration_state: MockMigrationStateRepo,
    key_migration: MockKeyMigration,
    blob_migration_repo: MockBlobMigrationRepo,
    blob_cipher: MockBlobCipher,
    handshake: MockHandshake,
    member_repo: MockMemberRepo,
    trust_repo: MockTrustRepo,
    peer_addr_repo: MockPeerAddrRepo,
}

impl Env {
    fn new() -> Self {
        Self {
            setup_status: MockSetupStatusRepo::new(),
            migration_state: MockMigrationStateRepo::new(),
            key_migration: MockKeyMigration::new(),
            blob_migration_repo: MockBlobMigrationRepo::new(),
            blob_cipher: MockBlobCipher::new(),
            handshake: MockHandshake::new(),
            member_repo: MockMemberRepo::new(),
            trust_repo: MockTrustRepo::new(),
            peer_addr_repo: MockPeerAddrRepo::new(),
        }
    }

    fn build(self) -> SwitchSpaceUseCase {
        self.build_with_facade(Arc::new(NoopAnalyticsFacade))
    }

    fn build_with_facade(self, analytics: Arc<dyn AnalyticsFacade>) -> SwitchSpaceUseCase {
        let admit = Arc::new(AdmitMemberUseCase::new(
            Arc::new(self.member_repo) as Arc<dyn MemberRepositoryPort>
        ));
        let trust = Arc::new(TrustPeerUseCase::new(
            Arc::new(self.trust_repo) as Arc<dyn TrustedPeerRepositoryPort>
        ));
        SwitchSpaceUseCase::new(
            Arc::new(self.setup_status),
            Arc::new(self.migration_state),
            Arc::new(self.key_migration),
            Arc::new(self.blob_migration_repo),
            Arc::new(self.blob_cipher),
            Arc::new(self.handshake) as Arc<dyn JoinerHandshakeRunner>,
            admit,
            trust,
            Arc::new(self.peer_addr_repo) as Arc<dyn PeerAddressRepositoryPort>,
            Arc::new(FixedClock(0)),
            analytics,
        )
    }
}

// ── 测试 ───────────────────────────────────────────────────────────────

/// Pre-flight 1：未 setup 直接拒绝，不动任何后续 port。
#[tokio::test]
async fn pre_flight_rejects_when_not_setup() {
    let mut env = Env::new();
    env.setup_status
        .expect_get_status()
        .return_once(|| Ok(SetupStatus::default())); // has_completed=false
                                                     // 其它 port 都 expect_*().times(0)（mockall 默认 strict——drop 时若被
                                                     // 意外调用就 panic）。下方 build 之后所有 mock drop 会校验。
    env.migration_state.expect_get_current().times(0);

    let err = env.build().execute(cmd_default()).await.unwrap_err();
    assert!(matches!(err, SwitchSpaceError::NotSetup));
}

/// Pre-flight 2：已有进行中迁移直接拒绝。
#[tokio::test]
async fn pre_flight_rejects_when_pending_migration() {
    let mut env = Env::new();
    env.setup_status
        .expect_get_status()
        .return_once(|| Ok(already_setup()));
    env.migration_state
        .expect_get_current()
        .return_once(|| Ok(Some(MigrationPhase::Prepared { run_id: run_id() })));

    let err = env.build().execute(cmd_default()).await.unwrap_err();
    match err {
        SwitchSpaceError::PendingMigration(MigrationPhase::Prepared { run_id: r }) => {
            assert_eq!(r, run_id());
        }
        other => panic!("expected PendingMigration(Prepared), got {other:?}"),
    }
}

/// Happy path：setup 完整 + 1 条 representation 走完 phase 1-4，最终
/// `setup_status.set_status` 把 space_id 切到新空间，所有 cleanup 成功。
#[tokio::test]
async fn happy_path_executes_all_4_phases() {
    let mut env = Env::new();

    // Pre-flight
    env.setup_status
        .expect_get_status()
        .return_once(|| Ok(already_setup()));
    env.migration_state
        .expect_get_current()
        .return_once(|| Ok(None));

    // Phase 1 — prepare
    env.key_migration
        .expect_prepare_migration_key()
        .return_once(|| Ok(run_id()));
    env.blob_migration_repo
        .expect_list_main_inline_representations()
        .return_once(|| {
            Ok(vec![(
                EventId::from_string("evt-1".into()),
                RepresentationId::from("rep-1"),
            )])
        });
    env.blob_migration_repo
        .expect_read_main_inline_data()
        .return_once(|_, _| Ok(Some(b"old-ciphertext".to_vec())));
    env.blob_cipher
        .expect_decrypt()
        .return_once(|_, _, _| Ok(uc_core::crypto::domain::Plaintext::new(b"plain".to_vec())));
    env.key_migration
        .expect_encrypt_with_migration_key()
        .return_once(|_, _, _| Ok(Ciphertext::new(b"mig-encrypted".to_vec())));
    env.blob_migration_repo
        .expect_upsert_record()
        .return_once(|_| Ok(()));

    // Phase 2 — handshake + admit + trust
    env.handshake
        .expect_run()
        .return_once(|_, _| Ok(outcome_default()));
    // AdmitMemberUseCase / TrustPeerUseCase 都先 get 后 save——返回 None
    // 走 fresh-admit 路径(Some 会走 #1023 的重配替换路径)。
    env.member_repo.expect_get().return_once(|_| Ok(None));
    env.member_repo.expect_save().return_once(|_| Ok(()));
    env.trust_repo.expect_get().return_once(|_| Ok(None));
    env.trust_repo.expect_save().return_once(|_| Ok(()));
    // sponsor_transport_address_blob 是空 Vec，peer_addr_repo.upsert 不应被调。
    env.peer_addr_repo.expect_upsert().times(0);

    // Phase 3 — swap
    env.blob_migration_repo
        .expect_count_records()
        .return_once(|| Ok(1));
    env.blob_migration_repo
        .expect_list_records()
        .return_once(|| {
            Ok(vec![MigrationRecord {
                event_id: EventId::from_string("evt-1".into()),
                representation_id: RepresentationId::from("rep-1"),
                migration_ciphertext: b"mig-encrypted".to_vec(),
            }])
        });
    env.key_migration
        .expect_decrypt_with_migration_key()
        .return_once(|_, _, _| Ok(uc_core::crypto::domain::Plaintext::new(b"plain".to_vec())));
    env.blob_cipher
        .expect_encrypt()
        .return_once(|_, _, _| Ok(Ciphertext::new(b"new-ciphertext".to_vec())));
    env.blob_migration_repo
        .expect_update_main_inline_data()
        .return_once(|_, _, _| Ok(()));

    // Phase 4 — setup_status 切换 + cleanup
    env.setup_status
        .expect_set_status()
        .withf(|s| {
            s.has_completed
                && s.space_id
                    .as_ref()
                    .map(|sid| sid.inner() == "new-space")
                    .unwrap_or(false)
        })
        .return_once(|_| Ok(()));
    env.blob_migration_repo
        .expect_discard_all_records()
        .return_once(|| Ok(()));
    env.key_migration
        .expect_discard_migration_key()
        .return_once(|_| Ok(()));

    // migration_state.set_current 会被调 4 次：Prepared → HandshakeDone → Swapped → None。
    env.migration_state
        .expect_set_current()
        .times(4)
        .returning(|_| Ok(()));

    let result = env.build().execute(cmd_default()).await.unwrap();
    assert_eq!(result.space_id, target_space());
    assert_eq!(result.sponsor_device_id.as_str(), "sponsor-device");
    assert_eq!(result.self_device_id.as_str(), "local-device");
    assert_eq!(result.migrated_records, 1);
}

/// Phase 1 中途解密失败：清空 backup 表，**不**写 Prepared 状态——
/// `migration_state.set_current` 不应该被调（连 Prepared 都没推进就失败了）。
#[tokio::test]
async fn phase1_decrypt_failure_aborts_and_cleans_backup() {
    let mut env = Env::new();
    env.setup_status
        .expect_get_status()
        .return_once(|| Ok(already_setup()));
    env.migration_state
        .expect_get_current()
        .return_once(|| Ok(None));

    env.key_migration
        .expect_prepare_migration_key()
        .return_once(|| Ok(run_id()));
    env.blob_migration_repo
        .expect_list_main_inline_representations()
        .return_once(|| {
            Ok(vec![(
                EventId::from_string("evt-1".into()),
                RepresentationId::from("rep-1"),
            )])
        });
    env.blob_migration_repo
        .expect_read_main_inline_data()
        .return_once(|_, _| Ok(Some(b"old-ciphertext".to_vec())));
    env.blob_cipher
        .expect_decrypt()
        .return_once(|_, _, _| Err(BlobCipherError::InvalidCiphertext));

    // Cleanup：discard_all_records 必须被调；migration_state 不应被推进。
    env.blob_migration_repo
        .expect_discard_all_records()
        .return_once(|| Ok(()));
    env.migration_state.expect_set_current().times(0);
    // Phase 1 失败 cleanup 不调 discard_migration_key——key 已经在 keyring，
    // 但本 cleanup 路径目前不知道 run_id (cleanup_after_phase1_failure 不
    // 接受 run_id 参数)，留给启动期补偿处理（旧 run_id 会成为孤儿条目，
    // 后续 prepare 时撞名才会暴露）。
    env.key_migration.expect_discard_migration_key().times(0);

    let err = env.build().execute(cmd_default()).await.unwrap_err();
    assert!(matches!(err, SwitchSpaceError::InvalidCiphertext));
}

/// Phase 2 handshake 失败：清空 backup + 销毁 migration_key + migration_state
/// 回到 None。Phase 1 已经推进过 Prepared，所以 set_current 总共 2 次：
/// 进 Prepared + 回 None。
#[tokio::test]
async fn phase2_handshake_failure_aborts_and_full_cleanup() {
    let mut env = Env::new();
    env.setup_status
        .expect_get_status()
        .return_once(|| Ok(already_setup()));
    env.migration_state
        .expect_get_current()
        .return_once(|| Ok(None));

    // Phase 1 happy path
    env.key_migration
        .expect_prepare_migration_key()
        .return_once(|| Ok(run_id()));
    env.blob_migration_repo
        .expect_list_main_inline_representations()
        .return_once(|| Ok(vec![]));

    // Phase 2 fails on handshake
    env.handshake
        .expect_run()
        .return_once(|_, _| Err(RedeemPairingInvitationError::SponsorUnreachable));

    // Cleanup：discard_all_records + discard_migration_key + set_current(None)
    env.blob_migration_repo
        .expect_discard_all_records()
        .return_once(|| Ok(()));
    env.key_migration
        .expect_discard_migration_key()
        .with(predicate::eq(run_id()))
        .return_once(|_| Ok(()));

    // set_current 调 2 次：Some(Prepared) + None
    env.migration_state
        .expect_set_current()
        .times(2)
        .returning(|_| Ok(()));

    let err = env.build().execute(cmd_default()).await.unwrap_err();
    assert!(matches!(err, SwitchSpaceError::SponsorUnreachable));
}

/// resume_pending: state=None → noop。
#[tokio::test]
async fn resume_none_is_noop() {
    let mut env = Env::new();
    env.migration_state
        .expect_get_current()
        .return_once(|| Ok(None));

    env.build().resume_pending().await.unwrap();
}

/// resume_pending: state=Prepared → 清空 backup + discard key + set None。
#[tokio::test]
async fn resume_prepared_aborts_and_cleans_up() {
    let mut env = Env::new();
    env.migration_state
        .expect_get_current()
        .return_once(|| Ok(Some(MigrationPhase::Prepared { run_id: run_id() })));

    env.blob_migration_repo
        .expect_discard_all_records()
        .return_once(|| Ok(()));
    env.key_migration
        .expect_discard_migration_key()
        .with(predicate::eq(run_id()))
        .return_once(|_| Ok(()));
    env.migration_state
        .expect_set_current()
        .withf(|p| p.is_none())
        .return_once(|_| Ok(()));

    env.build().resume_pending().await.unwrap();
}

/// resume_pending: state=HandshakeDone → 跑 phase 3（swap）+ phase 4（commit）。
#[tokio::test]
async fn resume_handshake_done_replays_phase3_and_phase4() {
    let mut env = Env::new();
    env.migration_state.expect_get_current().return_once(|| {
        Ok(Some(MigrationPhase::HandshakeDone {
            run_id: run_id(),
            target_space_id: target_space(),
            sponsor_space_person_id: None,
        }))
    });

    // Phase 3 — list backup, swap one record
    env.blob_migration_repo
        .expect_list_records()
        .return_once(|| {
            Ok(vec![MigrationRecord {
                event_id: EventId::from_string("evt-X".into()),
                representation_id: RepresentationId::from("rep-X"),
                migration_ciphertext: b"mig-ct".to_vec(),
            }])
        });
    env.key_migration
        .expect_decrypt_with_migration_key()
        .return_once(|_, _, _| Ok(uc_core::crypto::domain::Plaintext::new(b"plain".to_vec())));
    env.blob_cipher
        .expect_encrypt()
        .return_once(|_, _, _| Ok(Ciphertext::new(b"new-ct".to_vec())));
    env.blob_migration_repo
        .expect_update_main_inline_data()
        .return_once(|_, _, _| Ok(()));

    // 进 Swapped + 进 None：set_current 2 次。
    env.migration_state
        .expect_set_current()
        .times(2)
        .returning(|_| Ok(()));

    // Phase 4 — setup_status 切到新 space + cleanup
    env.setup_status
        .expect_set_status()
        .withf(|s| {
            s.has_completed
                && s.space_id
                    .as_ref()
                    .map(|sid| sid.inner() == "new-space")
                    .unwrap_or(false)
        })
        .return_once(|_| Ok(()));
    env.blob_migration_repo
        .expect_discard_all_records()
        .return_once(|| Ok(()));
    env.key_migration
        .expect_discard_migration_key()
        .with(predicate::eq(run_id()))
        .return_once(|_| Ok(()));

    env.build().resume_pending().await.unwrap();
}

/// resume_pending: state=Swapped → 仅跑 phase 4（不再 swap 主表）。
#[tokio::test]
async fn resume_swapped_replays_phase4_only() {
    let mut env = Env::new();
    env.migration_state.expect_get_current().return_once(|| {
        Ok(Some(MigrationPhase::Swapped {
            run_id: run_id(),
            target_space_id: target_space(),
            sponsor_space_person_id: None,
        }))
    });

    // 不该再调 list_records / decrypt / encrypt / update_main —— phase 3
    // 已经做完。mockall 默认 strict，未 expect 的调用会 panic，所以这里
    // 不显式 .times(0) 也能保证。

    env.setup_status.expect_set_status().return_once(|_| Ok(()));
    env.blob_migration_repo
        .expect_discard_all_records()
        .return_once(|| Ok(()));
    env.key_migration
        .expect_discard_migration_key()
        .return_once(|_| Ok(()));
    env.migration_state
        .expect_set_current()
        .withf(|p| p.is_none())
        .return_once(|_| Ok(()));

    env.build().resume_pending().await.unwrap();
}

/// 覆盖：handshake 后 admit 失败短路——peer_addr_repo.upsert 不应被调到，
/// trust 也不会。但 backup 表 + migration_key + state 仍要清掉。
#[tokio::test]
async fn admit_failure_aborts_phase2_and_cleans_up() {
    let mut env = Env::new();
    env.setup_status
        .expect_get_status()
        .return_once(|| Ok(already_setup()));
    env.migration_state
        .expect_get_current()
        .return_once(|| Ok(None));

    env.key_migration
        .expect_prepare_migration_key()
        .return_once(|| Ok(run_id()));
    env.blob_migration_repo
        .expect_list_main_inline_representations()
        .return_once(|| Ok(vec![]));

    env.handshake
        .expect_run()
        .return_once(|_, _| Ok(outcome_default()));
    // admit 流程会先 get 再 save——get 返 None 后 save 才报 db 错。
    env.member_repo.expect_get().return_once(|_| Ok(None));
    env.member_repo
        .expect_save()
        .return_once(|_| Err(MembershipError::Repository("db down".into())));
    // trust_repo / peer_addr_repo.upsert 不应被调
    env.trust_repo.expect_get().times(0);
    env.trust_repo.expect_save().times(0);
    env.peer_addr_repo.expect_upsert().times(0);

    // Cleanup
    env.blob_migration_repo
        .expect_discard_all_records()
        .return_once(|| Ok(()));
    env.key_migration
        .expect_discard_migration_key()
        .return_once(|_| Ok(()));
    env.migration_state
        .expect_set_current()
        .times(2)
        .returning(|_| Ok(())); // Prepared + None

    let err = env.build().execute(cmd_default()).await.unwrap_err();
    match err {
        SwitchSpaceError::Internal(m) => {
            assert!(m.contains("admit_member"), "msg = {m}");
        }
        other => panic!("expected Internal(admit_member: ...), got {other:?}"),
    }
}

// ── Identity-switch persistence regressions ────────────────────────────
//
// These cover the contract that telemetry identity transitions are
// resilient to crashes between `phase_4_commit` and the `$identify` /
// `$release` call: the intent is recorded in `MigrationPhase::Swapped`
// before phase 4 runs, so any resume that completes phase 4 also runs
// the identity switch.

fn sponsor_person() -> Uuid {
    Uuid::from_u128(0xDEAD_BEEF)
}

fn outcome_with_sponsor_person() -> JoinerHandshakeOutcome {
    JoinerHandshakeOutcome {
        sponsor_space_person_id: Some(sponsor_person()),
        ..outcome_default()
    }
}

/// `execute()` happy path with a sponsor-issued `space_person_id`:
/// `adopt_from_sponsor(target_person)` must fire exactly once *and*
/// the `Swapped` phase written before phase 4 must carry that id, so
/// a crash between commit and identify can be replayed.
#[tokio::test]
async fn happy_path_persists_identity_intent_and_invokes_adopt() {
    let mut env = Env::new();

    env.setup_status
        .expect_get_status()
        .return_once(|| Ok(already_setup()));
    env.migration_state
        .expect_get_current()
        .return_once(|| Ok(None));

    // Phase 1 — no representations, fastest happy path.
    env.key_migration
        .expect_prepare_migration_key()
        .return_once(|| Ok(run_id()));
    env.blob_migration_repo
        .expect_list_main_inline_representations()
        .return_once(|| Ok(vec![]));

    // Phase 2 — sponsor delivers a `space_person_id`.
    env.handshake
        .expect_run()
        .return_once(|_, _| Ok(outcome_with_sponsor_person()));
    env.member_repo.expect_get().return_once(|_| Ok(None));
    env.member_repo.expect_save().return_once(|_| Ok(()));
    env.trust_repo.expect_get().return_once(|_| Ok(None));
    env.trust_repo.expect_save().return_once(|_| Ok(()));
    env.peer_addr_repo.expect_upsert().times(0);

    // Phase 3 — no records to swap.
    env.blob_migration_repo
        .expect_count_records()
        .return_once(|| Ok(0));
    env.blob_migration_repo
        .expect_list_records()
        .return_once(|| Ok(vec![]));

    // Phase 4 — setup_status flips, cleanup runs.
    env.setup_status.expect_set_status().return_once(|_| Ok(()));
    env.blob_migration_repo
        .expect_discard_all_records()
        .return_once(|| Ok(()));
    env.key_migration
        .expect_discard_migration_key()
        .return_once(|_| Ok(()));

    // The critical assertions: every `Some(MigrationPhase)` set_current
    // call after phase 2 carries the sponsor's person id, so a crash
    // before identify can still recover the intent.
    env.migration_state
        .expect_set_current()
        .withf(|phase| match phase {
            Some(MigrationPhase::Prepared { .. }) => true,
            Some(MigrationPhase::HandshakeDone {
                sponsor_space_person_id,
                ..
            })
            | Some(MigrationPhase::Swapped {
                sponsor_space_person_id,
                ..
            }) => *sponsor_space_person_id == Some(sponsor_person()),
            None => true,
        })
        .times(4)
        .returning(|_| Ok(()));

    let recorder = Arc::new(RecordingAnalyticsFacade::default());
    let uc = env.build_with_facade(recorder.clone());
    uc.execute(cmd_default()).await.unwrap();

    assert_eq!(recorder.adopted(), vec![sponsor_person()]);
    assert_eq!(recorder.released_count(), 0);
}

/// Crash-then-resume from `HandshakeDone` must rebuild the adopt call
/// from the persisted `sponsor_space_person_id`. No outcome is
/// available at resume time — the persisted intent is the only source.
#[tokio::test]
async fn resume_handshake_done_with_persisted_person_invokes_adopt() {
    let mut env = Env::new();
    env.migration_state.expect_get_current().return_once(|| {
        Ok(Some(MigrationPhase::HandshakeDone {
            run_id: run_id(),
            target_space_id: target_space(),
            sponsor_space_person_id: Some(sponsor_person()),
        }))
    });

    // Phase 3 — no records.
    env.blob_migration_repo
        .expect_list_records()
        .return_once(|| Ok(vec![]));

    // The Swapped row written between phase 3 and phase 4 must carry
    // the same person id forward.
    env.migration_state
        .expect_set_current()
        .withf(|phase| match phase {
            Some(MigrationPhase::Swapped {
                sponsor_space_person_id,
                ..
            }) => *sponsor_space_person_id == Some(sponsor_person()),
            None => true,
            _ => false,
        })
        .times(2)
        .returning(|_| Ok(()));

    env.setup_status.expect_set_status().return_once(|_| Ok(()));
    env.blob_migration_repo
        .expect_discard_all_records()
        .return_once(|| Ok(()));
    env.key_migration
        .expect_discard_migration_key()
        .return_once(|_| Ok(()));

    let recorder = Arc::new(RecordingAnalyticsFacade::default());
    let uc = env.build_with_facade(recorder.clone());
    uc.resume_pending().await.unwrap();

    assert_eq!(recorder.adopted(), vec![sponsor_person()]);
    assert_eq!(recorder.released_count(), 0);
}

/// Crash-then-resume from `Swapped` (the narrowest window: setup_status
/// already swapped but identify not yet emitted) must still run the
/// identity switch using the persisted intent.
#[tokio::test]
async fn resume_swapped_with_persisted_person_invokes_adopt() {
    let mut env = Env::new();
    env.migration_state.expect_get_current().return_once(|| {
        Ok(Some(MigrationPhase::Swapped {
            run_id: run_id(),
            target_space_id: target_space(),
            sponsor_space_person_id: Some(sponsor_person()),
        }))
    });

    env.setup_status.expect_set_status().return_once(|_| Ok(()));
    env.blob_migration_repo
        .expect_discard_all_records()
        .return_once(|| Ok(()));
    env.key_migration
        .expect_discard_migration_key()
        .return_once(|_| Ok(()));
    env.migration_state
        .expect_set_current()
        .withf(|p| p.is_none())
        .return_once(|_| Ok(()));

    let recorder = Arc::new(RecordingAnalyticsFacade::default());
    let uc = env.build_with_facade(recorder.clone());
    uc.resume_pending().await.unwrap();

    assert_eq!(recorder.adopted(), vec![sponsor_person()]);
    assert_eq!(recorder.released_count(), 0);
}

/// Resume from a `Swapped` row whose `sponsor_space_person_id` is
/// `None` (v1→v2 path or a state file written before the field
/// existed): the identity switch must fall back to Solo, not be
/// silently skipped.
#[tokio::test]
async fn resume_swapped_with_none_person_falls_back_to_solo() {
    let mut env = Env::new();
    env.migration_state.expect_get_current().return_once(|| {
        Ok(Some(MigrationPhase::Swapped {
            run_id: run_id(),
            target_space_id: target_space(),
            sponsor_space_person_id: None,
        }))
    });

    env.setup_status.expect_set_status().return_once(|_| Ok(()));
    env.blob_migration_repo
        .expect_discard_all_records()
        .return_once(|| Ok(()));
    env.key_migration
        .expect_discard_migration_key()
        .return_once(|_| Ok(()));
    env.migration_state
        .expect_set_current()
        .withf(|p| p.is_none())
        .return_once(|_| Ok(()));

    let recorder = Arc::new(RecordingAnalyticsFacade::default());
    let uc = env.build_with_facade(recorder.clone());
    uc.resume_pending().await.unwrap();

    assert!(recorder.adopted().is_empty());
    assert_eq!(recorder.released_count(), 1);
}
