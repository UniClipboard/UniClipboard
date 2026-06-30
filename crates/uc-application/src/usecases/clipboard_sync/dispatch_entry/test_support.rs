//! Shared test doubles + fixtures for the `dispatch_entry` collaborator unit
//! tests (`target_selector`, `per_peer`, `header`).
//!
//! The use-case integration tests in `mod.rs` keep their own inline mocks
//! verbatim (they were verified port-for-port against the pre-split file and
//! must not move). This module exists so the sibling collaborators can each
//! assert against the same ports from their own focused test module without
//! re-deriving a mock per file. Names intentionally mirror the `mod.rs`
//! inline doubles; the two live in different modules, so there is no clash.

use std::sync::Mutex as StdMutex;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use tokio::sync::broadcast;

use uc_core::clipboard::ClipboardContentCategorySet;
use uc_core::ids::DeviceId;
use uc_core::ports::{
    ClipboardDispatchPort, ClipboardHeader, ClockPort, DispatchReport, FirstSyncStateError,
    FirstSyncStatePort, LocalIdentityError, LocalIdentityPort, PeerAddressError, PeerAddressRecord,
    PeerAddressRepositoryPort, PresenceError, PresenceEvent, PresencePort, ReachabilityState,
    SettingsPort, SyncPayload,
};
use uc_core::security::IdentityFingerprint;
use uc_core::settings::model::Settings;
use uc_core::{MemberRepositoryPort, MemberSyncPreferences, MembershipError, SpaceMember};
use uc_observability::analytics::{AnalyticsPort, Event};

use super::DispatchClipboardEntryInput;

// ── mockall ports ───────────────────────────────────────────────────────

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

mockall::mock! {
    pub MemberRepo {}

    #[async_trait]
    impl MemberRepositoryPort for MemberRepo {
        async fn get(&self, device_id: &DeviceId) -> Result<Option<SpaceMember>, MembershipError>;
        async fn list(&self) -> Result<Vec<SpaceMember>, MembershipError>;
        async fn save(&self, member: &SpaceMember) -> Result<(), MembershipError>;
        async fn remove(&self, device_id: &DeviceId) -> Result<bool, MembershipError>;
    }
}

mockall::mock! {
    pub Dispatch {}

    #[async_trait]
    impl ClipboardDispatchPort for Dispatch {
        async fn dispatch(
            &self,
            target: &DeviceId,
            header: &ClipboardHeader,
            payload: SyncPayload,
        ) -> DispatchReport;
    }
}

mockall::mock! {
    pub LocalIdentity {}

    #[async_trait]
    impl LocalIdentityPort for LocalIdentity {
        async fn create(&self) -> Result<IdentityFingerprint, LocalIdentityError>;
        async fn ensure(&self) -> Result<IdentityFingerprint, LocalIdentityError>;
        async fn get_current_fingerprint(
            &self,
        ) -> Result<Option<IdentityFingerprint>, LocalIdentityError>;
    }
}

mockall::mock! {
    pub Settings_ {}

    #[async_trait]
    impl SettingsPort for Settings_ {
        async fn load(&self) -> anyhow::Result<Settings>;
        async fn save(&self, s: &Settings) -> anyhow::Result<()>;
    }
}

// ── hand-written fakes ──────────────────────────────────────────────────

/// Sync, 1-line `ClockPort`; mockall's adapter would be strictly more code.
pub(crate) struct FixedClock(pub(crate) i64);
impl ClockPort for FixedClock {
    fn now_ms(&self) -> i64 {
        self.0
    }
}

/// Presence stub that always reports the same `ReachabilityState`. The
/// collaborators only read `current_state`; `ensure_reachable` / `subscribe`
/// are present to satisfy the trait.
pub(crate) struct StaticPresence(pub(crate) ReachabilityState);
#[async_trait]
impl PresencePort for StaticPresence {
    async fn ensure_reachable(
        &self,
        _device: &DeviceId,
    ) -> Result<ReachabilityState, PresenceError> {
        Ok(self.0)
    }

    async fn current_state(&self, _device: &DeviceId) -> ReachabilityState {
        self.0
    }

    fn subscribe(&self) -> broadcast::Receiver<PresenceEvent> {
        let (_tx, rx) = broadcast::channel(1);
        rx
    }
}

/// Records every captured analytics `Event` so tests can assert the per-peer
/// funnel ordering and field values.
#[derive(Default)]
pub(crate) struct CapturingAnalyticsSink {
    captured: StdMutex<Vec<Event>>,
}
impl CapturingAnalyticsSink {
    pub(crate) fn events(&self) -> Vec<Event> {
        self.captured.lock().unwrap().clone()
    }
}
impl AnalyticsPort for CapturingAnalyticsSink {
    fn capture(&self, event: Event) {
        self.captured.lock().unwrap().push(event);
    }
}

/// `first_sync_state` whose every flag is already marked: every `mark_*`
/// returns `Ok(false)`, so the dispatcher never fires a `first_*` event.
pub(crate) struct AllMarkedFirstSyncState;
#[async_trait]
impl FirstSyncStatePort for AllMarkedFirstSyncState {
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

/// In-memory `first_sync_state` mirroring production: the first `mark_*`
/// returns `Ok(true)`, subsequent calls `Ok(false)`; each flag independent.
#[derive(Default)]
pub(crate) struct InMemoryFirstSyncState {
    attempted: tokio::sync::Mutex<bool>,
    succeeded: tokio::sync::Mutex<bool>,
    file_succeeded: tokio::sync::Mutex<bool>,
}
#[async_trait]
impl FirstSyncStatePort for InMemoryFirstSyncState {
    async fn mark_first_sync_attempted(&self) -> Result<bool, FirstSyncStateError> {
        Ok(flip_once(&self.attempted).await)
    }
    async fn mark_first_sync_succeeded(&self) -> Result<bool, FirstSyncStateError> {
        Ok(flip_once(&self.succeeded).await)
    }
    async fn mark_first_file_sync_succeeded(&self) -> Result<bool, FirstSyncStateError> {
        Ok(flip_once(&self.file_succeeded).await)
    }
}

async fn flip_once(flag: &tokio::sync::Mutex<bool>) -> bool {
    let mut guard = flag.lock().await;
    if *guard {
        false
    } else {
        *guard = true;
        true
    }
}

// ── fixtures ────────────────────────────────────────────────────────────

pub(crate) fn dev(id: &str) -> DeviceId {
    DeviceId::new(id)
}

pub(crate) fn fp(seed: u8) -> IdentityFingerprint {
    IdentityFingerprint::from_raw_string(
        (0..16)
            .map(|i| char::from(b'A' + ((seed as usize + i) % 26) as u8))
            .collect::<String>(),
    )
    .expect("valid fingerprint")
}

pub(crate) fn record(device: &str) -> PeerAddressRecord {
    PeerAddressRecord {
        device_id: DeviceId::new(device),
        addr_blob: vec![0xAA; 32],
        observed_at: Utc::now(),
    }
}

/// Build a `SpaceMember` for `device` carrying the given sync preferences.
pub(crate) fn member(device: &DeviceId, prefs: MemberSyncPreferences) -> SpaceMember {
    SpaceMember {
        device_id: device.clone(),
        device_name: format!("Test {}", device.as_str()),
        identity_fingerprint: fp(0),
        joined_at: Utc::now(),
        sync_preferences: prefs,
    }
}

pub(crate) fn settings_with_device_name(name: &str) -> Settings {
    let mut s = Settings::default();
    s.general.device_name = Some(name.to_string());
    s
}

pub(crate) fn default_settings() -> Settings {
    Settings::default()
}

/// Default dispatch input (empty category set, no resend filter). Tests
/// mutate the `pub` fields they care about.
pub(crate) fn dispatch_input() -> DispatchClipboardEntryInput {
    DispatchClipboardEntryInput {
        plaintext: Bytes::from_static(b"hello world"),
        snapshot_hash: "9".repeat(64),
        payload_version: 3,
        categories: ClipboardContentCategorySet::empty(),
        entry_id: None,
        target_filter: None,
    }
}

/// A minimal wire header for the per-peer dispatch tests, which only care
/// that the header is forwarded unchanged to the dispatch port.
pub(crate) fn test_header() -> ClipboardHeader {
    ClipboardHeader {
        version: ClipboardHeader::CURRENT_VERSION,
        snapshot_hash: "0".repeat(64),
        captured_at_ms: 0,
        origin_device_id: "self-device".to_string(),
        origin_device_name: "Self".to_string(),
        payload_version: 3,
        flow_id: None,
    }
}

pub(crate) fn sync_payload() -> SyncPayload {
    SyncPayload {
        ciphertext: Bytes::from_static(b"ciphertext"),
    }
}
