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
use uc_observability::analytics::{AnalyticsPort, Event, NameLengthBucket, SetupEntry};

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
    /// Slice 8d · setup funnel anchor (`setup_started`) at execute() entry +
    /// `device_name_set` after the device-name resolution succeeds. The
    /// device name itself never reaches the sink — only the bucketed
    /// character-count region per `NameLengthBucket`.
    analytics: Arc<dyn AnalyticsPort>,
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
        analytics: Arc<dyn AnalyticsPort>,
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
    #[derive(Default)]
    struct CapturingAnalyticsSink {
        captured: Mutex<Vec<Event>>,
    }
    impl CapturingAnalyticsSink {
        fn events(&self) -> Vec<Event> {
            self.captured.lock().unwrap().clone()
        }
    }
    impl AnalyticsPort for CapturingAnalyticsSink {
        fn capture(&self, event: Event) {
            self.captured.lock().unwrap().push(event);
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

        let uc = InitializeSpaceUseCase::new(
            space_access.clone(),
            local_identity.clone(),
            device_identity,
            member_repo.clone(),
            setup_status.clone(),
            settings.clone(),
            clock,
            analytics.clone(),
        );
        Harness {
            uc,
            space_access,
            local_identity,
            member_repo,
            setup_status,
            settings,
            analytics,
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

        // Slice 8d · setup_started + device_name_set fire in order.
        // "My Mac" has 6 chars → Lt8 bucket.
        let events = h.analytics.events();
        assert_eq!(events.len(), 2, "expected exactly 2 setup events");
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
                2,
                "name={name}: setup_started + device_name_set"
            );
            match &events[1] {
                Event::DeviceNameSet { name_length_bucket } => {
                    assert_eq!(*name_length_bucket, expected, "name={name}");
                }
                other => panic!("expected DeviceNameSet, got {other:?}"),
            }
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
}
