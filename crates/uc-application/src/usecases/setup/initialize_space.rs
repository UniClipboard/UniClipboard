//! A1 · `InitializeSpaceUseCase`.
//!
//! First-run flow on a fresh device:
//!
//! 0. Guard: `SetupStatus.has_completed == false`. A second A1 on an
//!    already-set-up device returns `AlreadySetup` — the user should hit
//!    A2 (unlock) instead, or factory-reset first.
//! 1. Validate the passphrase confirmation.
//! 2. Resolve & persist `device_name` (`Settings.general.device_name`).
//! 3. `SpaceAccessPort::initialize` — create the encrypted space.
//! 4. `LocalIdentityPort::ensure` — read or lazily generate the Ed25519
//!    identity fingerprint. In Slice 1 the iroh endpoint binds its
//!    identity at bootstrap, so by the time A1 runs the identity already
//!    exists; `ensure()` returns that existing fingerprint idempotently.
//! 5. `DeviceIdentityPort::current_device_id` — local UUID.
//! 6. Persist the owner `SpaceMember` record.
//! 7. Mark `SetupStatus.has_completed = true`.
//!
//! The use case is atomic in intent (all-or-nothing) but relies on
//! port-level idempotency (space access `AlreadyInitialized`, identity
//! `ensure` is idempotent by design) rather than a distributed
//! transaction — retry after mid-way failure is expected to either resume
//! from the failed step or surface the conflict to the caller.

use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use tracing::{debug, info, instrument, warn};

use uc_core::ids::SpaceId;
use uc_core::membership::{MemberRepositoryPort, MemberSyncPreferences, SpaceMember};
use uc_core::ports::space::{SpaceAccessError, SpaceAccessPort};
use uc_core::ports::{
    ClockPort, DeviceIdentityPort, LocalIdentityError, LocalIdentityPort, SettingsPort,
    SetupStatusPort,
};
use uc_core::setup::SetupStatus;
use uc_observability::analytics::{
    AnalyticsFacade, Event, NameLengthBucket, SelfMintedAdoptRequest, SetupEntry,
};

use crate::facade::space_setup::commands::InitializeSpaceCommand;
use crate::facade::space_setup::{InitializeSpaceError, InitializeSpaceResult};

pub(crate) struct InitializeSpaceUseCase {
    space_access: Arc<dyn SpaceAccessPort>,
    local_identity: Arc<dyn LocalIdentityPort>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    member_repo: Arc<dyn MemberRepositoryPort>,
    setup_status: Arc<dyn SetupStatusPort>,
    settings: Arc<dyn SettingsPort>,
    clock: Arc<dyn ClockPort>,
    /// Setup-funnel anchor (`setup_started` on entry, `device_name_set`
    /// after the device-name resolution, `setup_completed` at the end)
    /// plus the identity transition that runs between persisting setup
    /// status and emitting `setup_completed`. The device name itself
    /// never reaches the sink — only the bucketed character-count.
    analytics: Arc<dyn AnalyticsFacade>,
}

impl InitializeSpaceUseCase {
    pub(crate) fn new(
        space_access: Arc<dyn SpaceAccessPort>,
        local_identity: Arc<dyn LocalIdentityPort>,
        device_identity: Arc<dyn DeviceIdentityPort>,
        member_repo: Arc<dyn MemberRepositoryPort>,
        setup_status: Arc<dyn SetupStatusPort>,
        settings: Arc<dyn SettingsPort>,
        clock: Arc<dyn ClockPort>,
        analytics: Arc<dyn AnalyticsFacade>,
    ) -> Self {
        Self {
            space_access,
            local_identity,
            device_identity,
            member_repo,
            setup_status,
            settings,
            clock,
            analytics,
        }
    }

    #[instrument(skip(self, cmd), fields(device_name_override = cmd.device_name.is_some()))]
    pub(crate) async fn execute(
        &self,
        cmd: InitializeSpaceCommand,
    ) -> Result<InitializeSpaceResult, InitializeSpaceError> {
        // schema doc §12.1 · 用 monotonic Instant 而非 ClockPort 测耗时：
        // `duration_ms_since_setup_started` 是流程内耗时，不需要 wall clock
        // 单调性保护；ClockPort 给业务时间戳用。
        let setup_started_at = Instant::now();

        // Slice 8d · setup funnel anchor. v1 fixes `entry = FirstRun` because
        // A1 (this use case) is the fresh-device flow by definition; once a
        // future "Manual setup retry" entry point exists, plumb it through
        // the command and switch on it here.
        self.analytics.capture(Event::SetupStarted {
            entry: SetupEntry::FirstRun,
        });

        // 0. Fresh-install guard. Slice 1 moved identity creation to
        //    bootstrap time (iroh endpoint bind), so identity-existence is
        //    no longer a reliable "was A1 already run?" signal; the setup
        //    status flag is.
        let status = self
            .setup_status
            .get_status()
            .await
            .map_err(|e| InitializeSpaceError::StorageFailed(e.to_string()))?;
        if status.has_completed {
            return Err(InitializeSpaceError::AlreadySetup);
        }

        // 1. Passphrase confirmation.
        if cmd.passphrase != cmd.passphrase_confirm {
            return Err(InitializeSpaceError::PassphraseMismatch);
        }

        // 2. Device name resolution + Settings persistence.
        let device_name = self.resolve_and_persist_device_name(&cmd).await?;

        // 3. Create the encrypted space. `space_id` is generated locally;
        //    the adapter treats it as an opaque identifier (keyslot lookup
        //    keys off the current profile, not this value).
        let space_id = SpaceId::new();
        self.space_access
            .initialize(&space_id, &cmd.passphrase)
            .await
            .map_err(map_initialize_space_access_err)?;
        debug!(%space_id, "space initialised");

        // 4. Resolve the local network identity. In Slice 1 the iroh
        //    endpoint binds its Ed25519 secret at bootstrap time, so
        //    `ensure()` returns the pre-existing fingerprint here.
        //    `AlreadyExists` from an older adapter implementation would be
        //    unexpected (the port's ensure contract is idempotent); map
        //    it through `StorageFailed` so the caller sees a typed error
        //    rather than a panic.
        let fingerprint = self.local_identity.ensure().await.map_err(|e| match e {
            LocalIdentityError::Storage(m) => InitializeSpaceError::StorageFailed(m),
            LocalIdentityError::AlreadyExists => InitializeSpaceError::StorageFailed(
                "local identity adapter raised AlreadyExists from ensure(); \
                 violates LocalIdentityPort idempotency contract"
                    .into(),
            ),
        })?;
        debug!(fingerprint = %fingerprint, "local identity resolved");

        // 5-6. Build and persist the owner SpaceMember record.
        let device_id = self.device_identity.current_device_id();
        let joined_at = self.now_utc()?;
        let member = SpaceMember {
            device_id: device_id.clone(),
            device_name,
            identity_fingerprint: fingerprint.clone(),
            joined_at,
            sync_preferences: MemberSyncPreferences::default(),
        };
        self.member_repo
            .save(&member)
            .await
            .map_err(|e| InitializeSpaceError::StorageFailed(e.to_string()))?;
        debug!(%device_id, "owner SpaceMember persisted");

        // 7. Mark setup as completed — and persist the minted
        //    `space_id` so A2 unlock / sponsor handshake / peer views
        //    all observe the same canonical id across process
        //    boundaries. Without this, every later process would mint
        //    its own UUID and the joiner would end up persisting a
        //    different id than the sponsor.
        self.setup_status
            .set_status(&SetupStatus {
                has_completed: true,
                space_id: Some(space_id.clone()),
            })
            .await
            .map_err(|e| InitializeSpaceError::StorageFailed(e.to_string()))?;
        info!(%space_id, %device_id, "space initialisation completed");

        // Identity switches before `setup_completed` fires so that event
        // already reports under the new person — keeps the activation
        // funnel terminal aggregated to the right id from the start.
        // Adopt failures are warn-logged inside the facade and skip the
        // identify; `setup_completed` still goes out under the Solo id
        // and aggregation retries on the next pairing.
        self.analytics.adopt_self_minted(SelfMintedAdoptRequest {
            space_id: space_id.to_string(),
            now_ms: self.clock.now_ms(),
        });

        // Activation funnel anchor. A1 is the fresh-device "create new
        // space" path — no pairing happens here, so
        // `has_paired_in_same_flow` is always false. The joiner path may
        // later emit its own `setup_completed` with the real value.
        // `duration_ms_since_setup_started` is `u32` (≤ 49 days);
        // overflow is structurally impossible, so we fall back to None
        // on the unreachable error path rather than widening the type.
        let duration_ms_since_setup_started =
            u32::try_from(setup_started_at.elapsed().as_millis()).ok();
        self.analytics.capture(Event::SetupCompleted {
            has_paired_in_same_flow: false,
            duration_ms_since_setup_started,
        });

        Ok(InitializeSpaceResult {
            space_id,
            self_device_id: device_id,
            fingerprint,
        })
    }

    async fn resolve_and_persist_device_name(
        &self,
        cmd: &InitializeSpaceCommand,
    ) -> Result<String, InitializeSpaceError> {
        let mut settings = self
            .settings
            .load()
            .await
            .map_err(|e| InitializeSpaceError::StorageFailed(e.to_string()))?;

        let incoming = cmd
            .device_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let effective = incoming
            .clone()
            .or_else(|| settings.general.device_name.clone())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or(InitializeSpaceError::DeviceNameRequired)?;

        // Persist the updated value only when the command explicitly
        // supplied a new one that differs from what is already on disk.
        if let Some(new_name) = incoming {
            if settings.general.device_name.as_deref() != Some(new_name.as_str()) {
                settings.general.device_name = Some(new_name);
                self.settings
                    .save(&settings)
                    .await
                    .map_err(|e| InitializeSpaceError::StorageFailed(e.to_string()))?;
            }
        }

        // Slice 8d · `device_name_set` fires only after a valid name lands
        // (`DeviceNameRequired` short-circuit above leaves the funnel
        // legitimately incomplete). Original name never leaves the device —
        // only the `NameLengthBucket` for the resolved value travels.
        self.analytics.capture(Event::DeviceNameSet {
            name_length_bucket: NameLengthBucket::from_char_count(effective.chars().count()),
        });

        Ok(effective)
    }

    fn now_utc(&self) -> Result<DateTime<Utc>, InitializeSpaceError> {
        let ms = self.clock.now_ms();
        DateTime::<Utc>::from_timestamp_millis(ms).ok_or_else(|| {
            warn!(ms, "clock returned a timestamp outside chrono's range");
            InitializeSpaceError::Internal("clock returned invalid timestamp".into())
        })
    }
}

fn map_initialize_space_access_err(err: SpaceAccessError) -> InitializeSpaceError {
    match err {
        SpaceAccessError::AlreadyInitialized => InitializeSpaceError::AlreadyInitialized,
        SpaceAccessError::Internal(m) => InitializeSpaceError::Internal(m),
        // `initialize` should not raise these — map to Internal so we
        // surface bugs rather than silently miscategorising.
        other => InitializeSpaceError::Internal(format!(
            "unexpected space access error during initialize: {other}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use uuid::Uuid;

    use uc_core::crypto::domain::{ActiveSpace, Passphrase};
    use uc_core::ids::{DeviceId, SpaceId};
    use uc_core::membership::{MembershipError, SpaceMember};
    use uc_core::ports::space::{SpaceAccessError, SpaceAccessPort};
    use uc_core::security::IdentityFingerprint;
    use uc_core::settings::model::Settings;
    use uc_core::space_access::{JoinOffer, ProofDerivedKey};

    // ---------- Fakes ----------

    #[derive(Default)]
    struct FakeSpaceAccess {
        initialized: Mutex<bool>,
        initialize_err: Mutex<Option<SpaceAccessError>>,
        unlock_err: Mutex<Option<SpaceAccessError>>,
        unlock_calls: Mutex<u32>,
    }

    #[async_trait]
    impl SpaceAccessPort for FakeSpaceAccess {
        async fn initialize(
            &self,
            space_id: &SpaceId,
            _passphrase: &Passphrase,
        ) -> Result<ActiveSpace, SpaceAccessError> {
            if let Some(err) = self.initialize_err.lock().unwrap().take() {
                return Err(err);
            }
            *self.initialized.lock().unwrap() = true;
            Ok(ActiveSpace::new(space_id.clone()))
        }
        async fn unlock(
            &self,
            space_id: &SpaceId,
            _passphrase: &Passphrase,
        ) -> Result<ActiveSpace, SpaceAccessError> {
            *self.unlock_calls.lock().unwrap() += 1;
            if let Some(err) = self.unlock_err.lock().unwrap().take() {
                return Err(err);
            }
            Ok(ActiveSpace::new(space_id.clone()))
        }
        async fn is_unlocked(&self, _space_id: &SpaceId) -> bool {
            true
        }
        async fn lock(&self, _space_id: &SpaceId) -> Result<(), SpaceAccessError> {
            Ok(())
        }
        async fn factory_reset(&self, _space_id: &SpaceId) -> Result<(), SpaceAccessError> {
            Ok(())
        }
        async fn try_resume_session(
            &self,
            _space_id: &SpaceId,
        ) -> Result<Option<ActiveSpace>, SpaceAccessError> {
            Ok(None)
        }
        async fn verify_keychain_access(&self) -> Result<bool, SpaceAccessError> {
            Ok(true)
        }
        async fn derive_subkey(
            &self,
            _salt: &[u8],
            _info: &[u8],
        ) -> Result<[u8; 32], SpaceAccessError> {
            Ok([0; 32])
        }
        async fn current_session_proof_key(
            &self,
        ) -> Result<Option<ProofDerivedKey>, SpaceAccessError> {
            Ok(None)
        }
        async fn prepare_join_offer(
            &self,
            _space_id: &SpaceId,
            _passphrase: &Passphrase,
        ) -> Result<JoinOffer, SpaceAccessError> {
            unimplemented!("not used in A1/A2 tests")
        }
        async fn derive_master_key_for_proof(
            &self,
            _offer: &JoinOffer,
            _passphrase: &Passphrase,
        ) -> Result<ProofDerivedKey, SpaceAccessError> {
            unimplemented!("not used in A1/A2 tests")
        }
    }

    #[derive(Default)]
    struct FakeLocalIdentity {
        fingerprint: Mutex<Option<IdentityFingerprint>>,
        create_err: Mutex<Option<LocalIdentityError>>,
        create_calls: Mutex<u32>,
    }

    #[async_trait]
    impl LocalIdentityPort for FakeLocalIdentity {
        async fn create(&self) -> Result<IdentityFingerprint, LocalIdentityError> {
            *self.create_calls.lock().unwrap() += 1;
            if let Some(err) = self.create_err.lock().unwrap().take() {
                return Err(err);
            }
            let fp = IdentityFingerprint::from_raw_string("ABCDEFGHIJKLMNOP").unwrap();
            *self.fingerprint.lock().unwrap() = Some(fp.clone());
            Ok(fp)
        }
        async fn ensure(&self) -> Result<IdentityFingerprint, LocalIdentityError> {
            if let Some(fp) = self.fingerprint.lock().unwrap().clone() {
                return Ok(fp);
            }
            self.create().await
        }
        async fn get_current_fingerprint(
            &self,
        ) -> Result<Option<IdentityFingerprint>, LocalIdentityError> {
            Ok(self.fingerprint.lock().unwrap().clone())
        }
    }

    struct FixedDeviceIdentity {
        id: DeviceId,
    }
    impl DeviceIdentityPort for FixedDeviceIdentity {
        fn current_device_id(&self) -> DeviceId {
            self.id.clone()
        }
    }

    #[derive(Default)]
    struct InMemoryMemberRepo {
        rows: Mutex<Vec<SpaceMember>>,
        save_err: Mutex<Option<MembershipError>>,
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
            if let Some(err) = self.save_err.lock().unwrap().take() {
                return Err(err);
            }
            let mut rows = self.rows.lock().unwrap();
            rows.retain(|m| m.device_id != member.device_id);
            rows.push(member.clone());
            Ok(())
        }
        async fn remove(&self, device_id: &DeviceId) -> Result<bool, MembershipError> {
            let mut rows = self.rows.lock().unwrap();
            let len_before = rows.len();
            rows.retain(|m| &m.device_id != device_id);
            Ok(rows.len() < len_before)
        }
    }

    #[derive(Default)]
    struct InMemorySetupStatus {
        status: Mutex<SetupStatus>,
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

    #[derive(Default)]
    struct InMemorySettings {
        settings: Mutex<Settings>,
    }
    #[async_trait]
    impl SettingsPort for InMemorySettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            Ok(self.settings.lock().unwrap().clone())
        }
        async fn save(&self, settings: &Settings) -> anyhow::Result<()> {
            *self.settings.lock().unwrap() = settings.clone();
            Ok(())
        }
    }

    struct FixedClock(i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    // ---------- Harness ----------

    /// Test-only `AnalyticsPort` that records every captured event for
    /// later inspection. Mirrors the joiner-side / sponsor-side fakes
    /// used by other Slice 8 tests.
    ///
    /// PR 4 起也记录 `$identify` 调用——sponsor A1 完成 setup 后会触发一次
    /// person 切换；测试需要断言它出现在 setup_completed *之前*。
    #[derive(Default)]
    struct CapturingAnalyticsSink {
        captured: Mutex<Vec<CapturedAnalytics>>,
    }
    #[derive(Debug, Clone)]
    enum CapturedAnalytics {
        Capture(Event),
        Identify(uc_observability::analytics::IdentifyPayload),
        GroupIdentify(uc_observability::analytics::GroupIdentifyPayload),
    }
    impl CapturingAnalyticsSink {
        fn events(&self) -> Vec<Event> {
            self.captured
                .lock()
                .unwrap()
                .iter()
                .filter_map(|c| match c {
                    CapturedAnalytics::Capture(e) => Some(e.clone()),
                    _ => None,
                })
                .collect()
        }
        fn ordered(&self) -> Vec<CapturedAnalytics> {
            self.captured.lock().unwrap().clone()
        }
        fn identify_calls(&self) -> Vec<uc_observability::analytics::IdentifyPayload> {
            self.captured
                .lock()
                .unwrap()
                .iter()
                .filter_map(|c| match c {
                    CapturedAnalytics::Identify(p) => Some(p.clone()),
                    _ => None,
                })
                .collect()
        }
        fn group_identify_calls(&self) -> Vec<uc_observability::analytics::GroupIdentifyPayload> {
            self.captured
                .lock()
                .unwrap()
                .iter()
                .filter_map(|c| match c {
                    CapturedAnalytics::GroupIdentify(p) => Some(p.clone()),
                    _ => None,
                })
                .collect()
        }
    }
    impl uc_observability::analytics::AnalyticsPort for CapturingAnalyticsSink {
        fn capture(&self, event: Event) {
            self.captured
                .lock()
                .unwrap()
                .push(CapturedAnalytics::Capture(event));
        }
        fn identify(&self, payload: uc_observability::analytics::IdentifyPayload) {
            self.captured
                .lock()
                .unwrap()
                .push(CapturedAnalytics::Identify(payload));
        }
        fn group_identify(&self, payload: uc_observability::analytics::GroupIdentifyPayload) {
            self.captured
                .lock()
                .unwrap()
                .push(CapturedAnalytics::GroupIdentify(payload));
        }
    }

    /// Test-only `AnalyticsIdentityPort` 跟踪 adopt/release 调用次数，
    /// 并允许测试注入失败。返回的 `previous_distinct_id` 来自 fixture，
    /// 不依赖 `global_event_context`——避免测试间相互污染。
    struct FakeAnalyticsIdentity {
        previous_anon: Uuid,
        adopted: Mutex<Vec<Uuid>>,
        released_count: Mutex<u32>,
        adopt_err: Mutex<Option<String>>,
    }
    impl FakeAnalyticsIdentity {
        fn new(previous_anon: Uuid) -> Self {
            Self {
                previous_anon,
                adopted: Mutex::new(Vec::new()),
                released_count: Mutex::new(0),
                adopt_err: Mutex::new(None),
            }
        }
        fn adopted(&self) -> Vec<Uuid> {
            self.adopted.lock().unwrap().clone()
        }
    }
    impl uc_observability::analytics::AnalyticsIdentityPort for FakeAnalyticsIdentity {
        fn adopt_space_person(
            &self,
            space_person_id: Uuid,
        ) -> Result<
            uc_observability::analytics::AdoptOutcome,
            uc_observability::analytics::AnalyticsIdentityError,
        > {
            if let Some(msg) = self.adopt_err.lock().unwrap().take() {
                return Err(
                    uc_observability::analytics::AnalyticsIdentityError::PersistFailed(
                        anyhow::anyhow!(msg),
                    ),
                );
            }
            self.adopted.lock().unwrap().push(space_person_id);
            Ok(uc_observability::analytics::AdoptOutcome {
                previous_distinct_id: self.previous_anon,
                new_distinct_id: space_person_id,
            })
        }
        fn release_space_person(
            &self,
        ) -> Result<
            uc_observability::analytics::ReleaseOutcome,
            uc_observability::analytics::AnalyticsIdentityError,
        > {
            *self.released_count.lock().unwrap() += 1;
            Ok(uc_observability::analytics::ReleaseOutcome {
                previous_distinct_id: self.previous_anon,
                new_distinct_id: self.previous_anon,
            })
        }

        fn current_space_person_id(&self) -> Option<Uuid> {
            // Fake：返回最近 adopt 的 ID（取最后一个），让 sponsor pairing
            // 测试在 A1 之后的场景能通过本 fake 的 SponsorHandshake 路径。
            self.adopted.lock().unwrap().last().copied()
        }
        fn reset_telemetry_identity(
            &self,
        ) -> Result<
            uc_observability::analytics::ReleaseOutcome,
            uc_observability::analytics::AnalyticsIdentityError,
        > {
            *self.released_count.lock().unwrap() += 1;
            // Fake：模拟 reset 后切回新 anonymous（这里复用 previous_anon 占位）。
            Ok(uc_observability::analytics::ReleaseOutcome {
                previous_distinct_id: self.previous_anon,
                new_distinct_id: self.previous_anon,
            })
        }
    }

    struct Harness {
        uc: InitializeSpaceUseCase,
        space_access: Arc<FakeSpaceAccess>,
        local_identity: Arc<FakeLocalIdentity>,
        member_repo: Arc<InMemoryMemberRepo>,
        setup_status: Arc<InMemorySetupStatus>,
        settings: Arc<InMemorySettings>,
        analytics: Arc<CapturingAnalyticsSink>,
        analytics_identity: Arc<FakeAnalyticsIdentity>,
    }

    fn build_harness() -> Harness {
        let space_access = Arc::new(FakeSpaceAccess::default());
        let local_identity = Arc::new(FakeLocalIdentity::default());
        let device_identity = Arc::new(FixedDeviceIdentity {
            id: DeviceId::new("device-1"),
        });
        let member_repo = Arc::new(InMemoryMemberRepo::default());
        let setup_status = Arc::new(InMemorySetupStatus::default());
        let settings = Arc::new(InMemorySettings::default());
        let clock: Arc<dyn ClockPort> = Arc::new(FixedClock(1_700_000_000_000));
        let analytics = Arc::new(CapturingAnalyticsSink::default());
        // Fixed UUID stands in for the sponsor device's anonymous_user_id
        // so tests can assert identify.previous_distinct_id equals it.
        let analytics_identity = Arc::new(FakeAnalyticsIdentity::new(Uuid::now_v7()));
        // The production code only sees `AnalyticsFacade`; the recording
        // sink + identity are stashed on the harness for assertions.
        let facade: Arc<dyn AnalyticsFacade> =
            Arc::new(uc_observability::analytics::DefaultAnalyticsFacade::new(
                analytics.clone(),
                analytics_identity.clone(),
            ));

        let uc = InitializeSpaceUseCase::new(
            space_access.clone(),
            local_identity.clone(),
            device_identity,
            member_repo.clone(),
            setup_status.clone(),
            settings.clone(),
            clock,
            facade,
        );
        Harness {
            uc,
            space_access,
            local_identity,
            member_repo,
            setup_status,
            settings,
            analytics,
            analytics_identity,
        }
    }

    fn ok_cmd(device_name: Option<&str>) -> InitializeSpaceCommand {
        InitializeSpaceCommand {
            passphrase: Passphrase::new("correct horse battery staple"),
            passphrase_confirm: Passphrase::new("correct horse battery staple"),
            device_name: device_name.map(String::from),
        }
    }

    // ---------- Tests ----------

    #[tokio::test]
    async fn happy_path_initialises_space_creates_identity_persists_member_marks_complete() {
        let h = build_harness();
        let result = h.uc.execute(ok_cmd(Some("My Mac"))).await.unwrap();

        assert_eq!(result.self_device_id, DeviceId::new("device-1"));
        assert_eq!(
            result.fingerprint,
            IdentityFingerprint::from_raw_string("ABCDEFGHIJKLMNOP").unwrap()
        );

        assert!(
            *h.space_access.initialized.lock().unwrap(),
            "space_access.initialize should have been called"
        );
        // A1 now calls ensure() rather than create(); the fake funnels
        // ensure-on-empty through create(), so create_calls still
        // increments exactly once on the fresh-install path.
        assert_eq!(*h.local_identity.create_calls.lock().unwrap(), 1);

        let members = h.member_repo.list().await.unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].device_id, DeviceId::new("device-1"));
        assert_eq!(members[0].device_name, "My Mac");

        let status = h.setup_status.get_status().await.unwrap();
        assert!(status.has_completed);

        let settings = h.settings.load().await.unwrap();
        assert_eq!(settings.general.device_name.as_deref(), Some("My Mac"));

        // setup_started → device_name_set → setup_completed 三件套按序触发。
        // "My Mac" has 6 chars → Lt8 bucket.
        let events = h.analytics.events();
        assert_eq!(events.len(), 3, "expected exactly 3 setup events");
        assert!(matches!(
            &events[0],
            Event::SetupStarted {
                entry: SetupEntry::FirstRun
            }
        ));
        assert!(matches!(
            &events[1],
            Event::DeviceNameSet {
                name_length_bucket: NameLengthBucket::Lt8
            }
        ));
        // schema doc §12.1 · A1 路径 has_paired_in_same_flow 恒为 false；
        // duration 字段由 monotonic Instant 推断，必须存在（u32 不可能溢出
        // 49 天）。
        match &events[2] {
            Event::SetupCompleted {
                has_paired_in_same_flow,
                duration_ms_since_setup_started,
            } => {
                assert!(!has_paired_in_same_flow);
                assert!(
                    duration_ms_since_setup_started.is_some(),
                    "duration should be populated for in-process A1"
                );
            }
            other => panic!("expected SetupCompleted, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn setup_completed_not_emitted_on_failure_before_status_persist() {
        // schema doc §12.1 · setup_completed 必须在 SetupStatus.has_completed
        // 落地之后才发——member_repo.save 失败属于第 6 步失败，第 7 步未执行，
        // 不应 emit 该事件，否则 Activation 漏斗会把"未完成"误判为"已完成"。
        let h = build_harness();
        *h.member_repo.save_err.lock().unwrap() =
            Some(MembershipError::Repository("boom".to_string()));
        let _ = h.uc.execute(ok_cmd(Some("My Mac"))).await.unwrap_err();
        let events = h.analytics.events();
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::SetupCompleted { .. })),
            "setup_completed must not be emitted when setup fails before status persist; \
             got events: {events:?}"
        );
    }

    #[tokio::test]
    async fn passphrase_mismatch_short_circuits_before_any_port_call() {
        let h = build_harness();
        let cmd = InitializeSpaceCommand {
            passphrase: Passphrase::new("one"),
            passphrase_confirm: Passphrase::new("two"),
            device_name: Some("My Mac".into()),
        };

        let err = h.uc.execute(cmd).await.unwrap_err();
        assert!(matches!(err, InitializeSpaceError::PassphraseMismatch));
        assert!(!*h.space_access.initialized.lock().unwrap());
        assert_eq!(*h.local_identity.create_calls.lock().unwrap(), 0);

        // Slice 8d · setup_started fires regardless (funnel anchor),
        // device_name_set must NOT — the name was never resolved.
        let events = h.analytics.events();
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], Event::SetupStarted { .. }));
    }

    #[tokio::test]
    async fn device_name_missing_errors_before_touching_space_access() {
        let h = build_harness();
        // Neither command nor settings provides a name.
        let err = h.uc.execute(ok_cmd(None)).await.unwrap_err();
        assert!(matches!(err, InitializeSpaceError::DeviceNameRequired));
        assert!(!*h.space_access.initialized.lock().unwrap());

        // Slice 8d · setup_started fires; device_name_set absent because
        // no valid name materialised.
        let events = h.analytics.events();
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], Event::SetupStarted { .. }));
    }

    #[tokio::test]
    async fn device_name_set_uses_name_length_bucket_boundaries() {
        // 0..=7 → Lt8, 8..=16 → Range8To16, >16 → Gt16.
        // Verify the threshold used by the use case matches the documented
        // NameLengthBucket boundaries by exercising one name from each range.
        for (name, expected) in [
            ("Mac", NameLengthBucket::Lt8),
            ("My Macbook Pro", NameLengthBucket::Range8To16),
            ("This Is A Very Long Device Name", NameLengthBucket::Gt16),
        ] {
            let h = build_harness();
            h.uc.execute(ok_cmd(Some(name))).await.unwrap();
            let events = h.analytics.events();
            assert_eq!(
                events.len(),
                3,
                "name={name}: setup_started + device_name_set + setup_completed"
            );
            match &events[1] {
                Event::DeviceNameSet { name_length_bucket } => {
                    assert_eq!(*name_length_bucket, expected, "name={name}");
                }
                other => panic!("expected DeviceNameSet, got {other:?}"),
            }
            assert!(
                matches!(&events[2], Event::SetupCompleted { .. }),
                "name={name}: third event should be SetupCompleted"
            );
        }
    }

    #[tokio::test]
    async fn falls_back_to_persisted_device_name_when_command_omits_it() {
        let h = build_harness();
        {
            let mut settings = h.settings.settings.lock().unwrap();
            settings.general.device_name = Some("Persisted Mac".into());
        }
        let result = h.uc.execute(ok_cmd(None)).await.unwrap();
        let _ = result; // result is fine; we care about member/device_name
        let members = h.member_repo.list().await.unwrap();
        assert_eq!(members[0].device_name, "Persisted Mac");
    }

    #[tokio::test]
    async fn space_access_already_initialized_maps_to_already_initialized() {
        let h = build_harness();
        *h.space_access.initialize_err.lock().unwrap() = Some(SpaceAccessError::AlreadyInitialized);
        let err = h.uc.execute(ok_cmd(Some("My Mac"))).await.unwrap_err();
        assert!(matches!(err, InitializeSpaceError::AlreadyInitialized));
        // Identity must NOT be created before space access succeeds.
        assert_eq!(*h.local_identity.create_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn already_completed_setup_rejects_before_touching_space_access() {
        let h = build_harness();
        // Pre-mark setup as done — simulates a second A1 call on a device
        // that has already onboarded.
        *h.setup_status.status.lock().unwrap() = SetupStatus {
            has_completed: true,
            space_id: None,
        };

        let err = h.uc.execute(ok_cmd(Some("My Mac"))).await.unwrap_err();

        assert!(matches!(err, InitializeSpaceError::AlreadySetup));
        // Must bail BEFORE any port call so the persisted space stays
        // untouched.
        assert!(!*h.space_access.initialized.lock().unwrap());
        assert_eq!(*h.local_identity.create_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn identity_ensure_adapter_bug_raises_storage_failed() {
        // The LocalIdentityPort contract says ensure() is idempotent — it
        // must not raise AlreadyExists. If an adapter violates that, A1
        // surfaces it as a typed StorageFailed so callers can log/alert
        // rather than panic.
        let h = build_harness();
        *h.local_identity.create_err.lock().unwrap() = Some(LocalIdentityError::AlreadyExists);
        let err = h.uc.execute(ok_cmd(Some("My Mac"))).await.unwrap_err();
        match err {
            InitializeSpaceError::StorageFailed(msg) => {
                assert!(msg.contains("idempotency"), "msg was {msg}");
            }
            other => panic!("expected StorageFailed, got {other:?}"),
        }
        let status = h.setup_status.get_status().await.unwrap();
        assert!(!status.has_completed);
    }

    #[tokio::test]
    async fn member_repo_save_failure_maps_to_storage_failed() {
        let h = build_harness();
        *h.member_repo.save_err.lock().unwrap() =
            Some(MembershipError::Repository("boom".to_string()));
        let err = h.uc.execute(ok_cmd(Some("My Mac"))).await.unwrap_err();
        assert!(matches!(err, InitializeSpaceError::StorageFailed(_)));
        let status = h.setup_status.get_status().await.unwrap();
        assert!(
            !status.has_completed,
            "should not mark setup complete when member persistence fails"
        );
    }

    #[tokio::test]
    async fn new_device_name_updates_persisted_settings() {
        let h = build_harness();
        {
            let mut settings = h.settings.settings.lock().unwrap();
            settings.general.device_name = Some("Old Name".into());
        }
        h.uc.execute(ok_cmd(Some("New Name"))).await.unwrap();
        let settings = h.settings.load().await.unwrap();
        assert_eq!(settings.general.device_name.as_deref(), Some("New Name"));
    }

    // —— Phase 098 / PR 4 · v2 跨设备 person 聚合 sponsor 端 ——————————

    /// happy path：A1 完成 setup 后必须先调一次 adopt_space_person，再发
    /// 一次 `$identify`，最后才 emit setup_completed —— 顺序不能颠倒。
    #[tokio::test]
    async fn a1_emits_identify_before_setup_completed() {
        let h = build_harness();
        h.uc.execute(ok_cmd(Some("My Mac"))).await.unwrap();

        // adopt_space_person 必须正好被调一次，且参数是新 UUIDv7（与持久化文件
        // 的 set 行为一致）。
        let adopted = h.analytics_identity.adopted();
        assert_eq!(adopted.len(), 1, "adopt_space_person 应正好一次");
        assert_eq!(
            adopted[0].get_version_num(),
            7,
            "新生成的 space_person_id 应是 UUIDv7"
        );

        // analytics 流水：setup_started → device_name_set → $identify → setup_completed。
        // identify 必须在 setup_completed 之前出现，否则 setup_completed 会停在
        // 老 distinct_id 名下，破坏 dashboard 的 Activation funnel 归属。
        let ordered = h.analytics.ordered();
        let identify_pos = ordered
            .iter()
            .position(|c| matches!(c, CapturedAnalytics::Identify(_)))
            .expect("expected one $identify call");
        let setup_completed_pos = ordered
            .iter()
            .position(|c| matches!(c, CapturedAnalytics::Capture(Event::SetupCompleted { .. })))
            .expect("expected setup_completed event");
        assert!(
            identify_pos < setup_completed_pos,
            "identify 必须在 setup_completed 之前：{ordered:?}"
        );

        // identify payload 端点：old_distinct_id 应等于 sponsor 设备的原
        // anonymous_user_id（fixture 注入）；new_distinct_id 应等于刚 adopt
        // 的 space_person_id。
        let identify_calls = h.analytics.identify_calls();
        assert_eq!(identify_calls.len(), 1, "identify 应正好一次");
        assert_eq!(
            identify_calls[0].old_distinct_id, h.analytics_identity.previous_anon,
            "identify.old_distinct_id 必须是 sponsor anonymous_user_id"
        );
        assert_eq!(
            identify_calls[0].new_distinct_id, adopted[0],
            "identify.new_distinct_id 必须等于刚 adopt 的 space_person_id"
        );
    }

    /// adopt 失败时 identify 不发出，但 setup_completed 仍发出 ——
    /// task_plan §PR 4 / 开放问题 3 决策 A：person 聚合可推迟（fire-and-forget），
    /// 但 setup 的"完成"事实必须如实上报。
    #[tokio::test]
    async fn a1_skips_identify_when_adopt_space_person_fails() {
        let h = build_harness();
        *h.analytics_identity.adopt_err.lock().unwrap() = Some("simulated persist failure".into());

        h.uc.execute(ok_cmd(Some("My Mac"))).await.unwrap();

        assert!(
            h.analytics_identity.adopted().is_empty(),
            "adopt 失败时不应记录任何成功 adopt"
        );
        assert!(
            h.analytics.identify_calls().is_empty(),
            "adopt 失败必须跳过 identify（避免服务端误合并）"
        );
        // setup_completed 仍 emit，distinct_id 沿用 Solo 状态的 anonymous_user_id。
        let events = h.analytics.events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::SetupCompleted { .. })),
            "adopt 失败不应阻止 setup_completed：events={events:?}"
        );
    }

    /// setup 在第 6 步失败（member_repo.save）时既不 adopt，也不 identify ——
    /// 整个身份切换流程都应在 setup_completed 路径里，setup 还没成功就不该发。
    #[tokio::test]
    async fn a1_skips_identify_when_setup_fails_before_status_persist() {
        let h = build_harness();
        *h.member_repo.save_err.lock().unwrap() = Some(MembershipError::Repository("boom".into()));

        let _ = h.uc.execute(ok_cmd(Some("My Mac"))).await.unwrap_err();

        assert!(
            h.analytics_identity.adopted().is_empty(),
            "setup 失败时不应触发 adopt"
        );
        assert!(
            h.analytics.identify_calls().is_empty(),
            "setup 失败时不应触发 identify"
        );
    }

    /// PR 7：A1 完成 setup 后必须 fire 一次 `$groupidentify`，把 Space group
    /// 维度的 created_at + device_count=1 写入。dashboard 据此识别 group 出现。
    #[tokio::test]
    async fn a1_emits_group_identify_after_identify() {
        let h = build_harness();
        h.uc.execute(ok_cmd(Some("My Mac"))).await.unwrap();

        let group_calls = h.analytics.group_identify_calls();
        assert_eq!(group_calls.len(), 1, "group_identify 必须正好一次");
        assert_eq!(
            group_calls[0].group_type, "space",
            "group_type 必须固定为 space"
        );
        // group_key 是 16-hex 的 space_id_hash。
        assert_eq!(
            group_calls[0].group_key.len(),
            16,
            "group_key 应为 16 字符 hex"
        );
        // group set 至少包含 created_at 与 device_count=1。
        assert!(
            group_calls[0].set.contains_key("created_at"),
            "group set 应携带 created_at"
        );
        assert_eq!(
            group_calls[0].set["device_count"],
            serde_json::Value::Number(1.into()),
            "首台设备 device_count 必须为 1"
        );

        // 时序：group_identify 必须出现在 identify 之后、setup_completed 之前。
        let ordered = h.analytics.ordered();
        let identify_pos = ordered
            .iter()
            .position(|c| matches!(c, CapturedAnalytics::Identify(_)))
            .unwrap();
        let group_pos = ordered
            .iter()
            .position(|c| matches!(c, CapturedAnalytics::GroupIdentify(_)))
            .unwrap();
        let setup_pos = ordered
            .iter()
            .position(|c| matches!(c, CapturedAnalytics::Capture(Event::SetupCompleted { .. })))
            .unwrap();
        assert!(
            identify_pos < group_pos && group_pos < setup_pos,
            "时序应为 identify → group_identify → setup_completed：{ordered:?}"
        );
    }

    /// adopt 失败时连 group_identify 都不应发——group 没有 person 锚点，
    /// 提前 group_identify 会让 PostHog 端拿到一个无 person 的 group。
    #[tokio::test]
    async fn a1_skips_group_identify_when_adopt_fails() {
        let h = build_harness();
        *h.analytics_identity.adopt_err.lock().unwrap() = Some("simulated".into());

        h.uc.execute(ok_cmd(Some("My Mac"))).await.unwrap();

        assert!(
            h.analytics.group_identify_calls().is_empty(),
            "adopt 失败时不应 group_identify"
        );
    }
}
