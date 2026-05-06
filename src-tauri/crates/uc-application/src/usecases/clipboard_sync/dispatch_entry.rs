//! Slice 2 Phase 2 · T7 — `DispatchClipboardEntryUseCase`.
//!
//! Encrypts one clipboard plaintext payload via [`TransferCipherPort`] and
//! fans it out to every paired member (excluding self) on the clipboard
//! ALPN. Failure per target is isolated in the per-target report so a
//! single unreachable peer never blocks the rest of the roster.
//!
//! ## Inputs, not side-effects
//!
//! This use case takes a [`DispatchClipboardEntryInput`] — plaintext bytes
//! + `content_hash` + `payload_version`. Reading the system clipboard +
//! building the `ClipboardBinaryPayload` is the caller's responsibility
//! (CLI `send` / `watch` in T11, daemon in Phase 3). Keeping the
//! plaintext-production step outside keeps the use case testable with
//! deterministic bytes.
//!
//! ## Iteration source
//!
//! Follows the `EnsureReachableAllUseCase` pattern (T6 / Phase 1):
//! `peer_addr_repo.list()` is the authoritative roster of "members we
//! have an address blob for" and avoids iterating ghost entries in
//! `member_repo` that never completed pairing. We intentionally do **not**
//! pre-filter by `PresencePort::current_state == Online`: presence's
//! `last_state` is populated by our own outbound `ensure_reachable`
//! probes, so when a peer dials us first (accept path only), our cache
//! still reports `Unknown`/`Offline` and a pre-filter would drop a peer
//! that's in fact reachable. Instead we let the dispatch port try every
//! paired member and record `Err(Offline)` in `per_target` for whichever
//! ones the wire can't reach. The iroh dispatch adapter returns quickly
//! on unreachable peers, so this costs little even when many peers are
//! down.
//!
//! ## Concurrency
//!
//! `tokio::task::JoinSet` per target. Phase 1's mockall-Mutex lesson
//! (slice2-phase1-plan.md §12.3 decision 5) only applies when **wall-time
//! concurrency** is asserted — the tests below use mockall throughout
//! because none of them measure wall-clock duration; `.returning(...)`
//! closures return immediately, so the expectation Mutex never blocks
//! anything observable. Hand-written fakes are reserved for cases that
//! genuinely need them (broadcast `subscribe + emit`; see
//! `ingest_inbound.rs::tests` and Phase 1 `roster/facade.rs::FakePresence`).

use std::sync::Arc;

use bytes::Bytes;
use tokio::task::JoinSet;
use tracing::{debug, info, instrument, warn};

use uc_core::clipboard::ClipboardContentCategorySet;
use uc_core::ids::DeviceId;
use uc_core::ports::security::TransferCipherPort;
use uc_core::ports::{
    ClipboardDispatchError, ClipboardDispatchPort, ClipboardHeader, ClockPort, DeviceIdentityPort,
    DispatchAck, LocalIdentityPort, PeerAddressRepositoryPort, SettingsPort, SyncPayload,
};
use uc_core::MemberRepositoryPort;

/// Input to one dispatch pass. The caller owns the plaintext →
/// `ClipboardBinaryPayload` → bytes pipeline.
#[derive(Debug, Clone)]
pub(crate) struct DispatchClipboardEntryInput {
    /// Unencrypted payload bytes. Typically the postcard-encoded
    /// `ClipboardBinaryPayload` (V3) the caller built from the system
    /// clipboard snapshot.
    pub plaintext: Bytes,
    /// SHA256 hex of the plaintext above. Receiver uses this for dedup.
    pub content_hash: String,
    /// Payload codec tag, e.g. `3` for the V3 `ClipboardBinaryPayload`.
    pub payload_version: u8,
    /// Set of content categories present in the snapshot, used to gate
    /// against each peer's `send_content_types` toggle. Caller (facade
    /// `dispatch_snapshot*`) computes via
    /// `ClipboardContentCategorySet::from_snapshot`. CLI raw-bytes paths pass
    /// an empty set (fail open) since they can't enumerate reps.
    pub categories: ClipboardContentCategorySet,
}

/// One target's dispatch result. `Ok` + `DispatchAck` when the peer
/// accepted or duplicate-ignored the frame; `Err` when the wire boundary
/// rejected or the peer is offline.
#[derive(Debug, Clone)]
pub(crate) struct DispatchPerTarget {
    pub device_id: DeviceId,
    pub outcome: Result<DispatchAck, String>,
}

/// Aggregated per-pass outcome. `total_accepted` counts peers that
/// returned `Accepted` (the ones whose repos now carry the new entry);
/// `total_offline` counts peers the dispatch port reported as unreachable.
#[derive(Debug, Clone)]
pub(crate) struct DispatchOutcome {
    pub content_hash: String,
    pub per_target: Vec<DispatchPerTarget>,
    pub total_accepted: usize,
    pub total_duplicate: usize,
    pub total_offline: usize,
    pub total_errored: usize,
    pub at_ms: i64,
}

/// Fatal errors that abort the whole pass. Per-peer failures land in
/// `per_target`; they are not errors in this sense.
#[derive(Debug, thiserror::Error)]
pub(crate) enum DispatchSyncError {
    /// Encryption failed — typically because the space session is locked.
    #[error("encryption session not unlocked")]
    LockedSpace,
    /// Encryption failed for any other reason.
    #[error("transfer cipher failure: {0}")]
    CipherFailure(String),
    /// Listing the peer address repository failed.
    #[error("peer_addr_repo.list: {0}")]
    Repository(String),
}

pub(crate) struct DispatchClipboardEntryUseCase {
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    member_repo: Arc<dyn MemberRepositoryPort>,
    transfer_cipher: Arc<dyn TransferCipherPort>,
    clipboard_dispatch: Arc<dyn ClipboardDispatchPort>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    local_identity: Arc<dyn LocalIdentityPort>,
    settings: Arc<dyn SettingsPort>,
    clock: Arc<dyn ClockPort>,
}

impl DispatchClipboardEntryUseCase {
    pub(crate) fn new(
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
        member_repo: Arc<dyn MemberRepositoryPort>,
        transfer_cipher: Arc<dyn TransferCipherPort>,
        clipboard_dispatch: Arc<dyn ClipboardDispatchPort>,
        device_identity: Arc<dyn DeviceIdentityPort>,
        local_identity: Arc<dyn LocalIdentityPort>,
        settings: Arc<dyn SettingsPort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self {
            peer_addr_repo,
            member_repo,
            transfer_cipher,
            clipboard_dispatch,
            device_identity,
            local_identity,
            settings,
            clock,
        }
    }

    #[instrument(skip_all, fields(content_hash = %input.content_hash))]
    pub(crate) async fn execute(
        &self,
        input: DispatchClipboardEntryInput,
    ) -> Result<DispatchOutcome, DispatchSyncError> {
        // 1. Encrypt. A locked session surfaces here — let it short-circuit
        //    so we don't spam the dispatch wire with encrypt-retries.
        let ciphertext = match self.transfer_cipher.encrypt(&input.plaintext).await {
            Ok(bytes) => Bytes::from(bytes),
            Err(err) => {
                return Err(match err {
                    uc_core::ports::security::TransferCipherError::NotUnlocked => {
                        DispatchSyncError::LockedSpace
                    }
                    other => DispatchSyncError::CipherFailure(other.to_string()),
                });
            }
        };

        // 2. Enumerate targets. `peer_addr_repo.list()` is the iteration
        //    source (see module doc); self is the only filter. Presence
        //    state is intentionally NOT consulted — see module doc for
        //    rationale. The dispatch port reports `Offline` per-target
        //    for unreachable peers, which we fold into the outcome below.
        let records =
            self.peer_addr_repo.list().await.map_err(|err| {
                DispatchSyncError::Repository(format!("peer_addr_repo.list: {err}"))
            })?;

        let local_device = self.device_identity.current_device_id();
        let mut candidates: Vec<DeviceId> = Vec::with_capacity(records.len());
        for record in records {
            if record.device_id == local_device {
                continue;
            }
            if !self
                .is_send_allowed(&record.device_id, &input.categories)
                .await
            {
                continue;
            }
            candidates.push(record.device_id);
        }

        // 3. Build the header once and clone per target.
        let origin_device_name = self.load_origin_device_name().await;
        let header = ClipboardHeader {
            version: ClipboardHeader::CURRENT_VERSION,
            content_hash: input.content_hash.clone(),
            captured_at_ms: self.clock.now_ms(),
            origin_device_id: local_device.as_str().to_string(),
            origin_device_name,
            payload_version: input.payload_version,
        };

        if candidates.is_empty() {
            info!("dispatch: no paired peers; skipping fan-out");
            return Ok(DispatchOutcome {
                content_hash: input.content_hash,
                per_target: Vec::new(),
                total_accepted: 0,
                total_duplicate: 0,
                total_offline: 0,
                total_errored: 0,
                at_ms: self.clock.now_ms(),
            });
        }

        // 4. Fan-out. One JoinSet task per target; results merged at the end.
        let mut set: JoinSet<(DeviceId, Result<DispatchAck, ClipboardDispatchError>)> =
            JoinSet::new();
        for device_id in &candidates {
            let dispatch = Arc::clone(&self.clipboard_dispatch);
            let header = header.clone();
            let device_id = device_id.clone();
            let payload = SyncPayload {
                ciphertext: ciphertext.clone(),
            };
            set.spawn(async move {
                let result = dispatch.dispatch(&device_id, &header, payload).await;
                (device_id, result)
            });
        }

        let mut per_target = Vec::with_capacity(candidates.len());
        let mut total_accepted = 0;
        let mut total_duplicate = 0;
        let mut total_offline = 0;
        let mut total_errored = 0;

        while let Some(joined) = set.join_next().await {
            match joined {
                Ok((device_id, Ok(DispatchAck::Accepted))) => {
                    total_accepted += 1;
                    debug!(device_id = %device_id.as_str(), "dispatch → Accepted");
                    per_target.push(DispatchPerTarget {
                        device_id,
                        outcome: Ok(DispatchAck::Accepted),
                    });
                }
                Ok((device_id, Ok(DispatchAck::DuplicateIgnored))) => {
                    total_duplicate += 1;
                    debug!(device_id = %device_id.as_str(), "dispatch → DuplicateIgnored");
                    per_target.push(DispatchPerTarget {
                        device_id,
                        outcome: Ok(DispatchAck::DuplicateIgnored),
                    });
                }
                Ok((device_id, Err(ClipboardDispatchError::Offline))) => {
                    total_offline += 1;
                    warn!(device_id = %device_id.as_str(), "dispatch → Offline");
                    per_target.push(DispatchPerTarget {
                        device_id,
                        outcome: Err("offline".to_string()),
                    });
                }
                Ok((device_id, Err(err))) => {
                    total_errored += 1;
                    warn!(device_id = %device_id.as_str(), error = %err, "dispatch failed");
                    per_target.push(DispatchPerTarget {
                        device_id,
                        outcome: Err(err.to_string()),
                    });
                }
                Err(err) => {
                    total_errored += 1;
                    warn!(error = %err, "dispatch task panicked or cancelled");
                }
            }
        }

        Ok(DispatchOutcome {
            content_hash: input.content_hash,
            per_target,
            total_accepted,
            total_duplicate,
            total_offline,
            total_errored,
            at_ms: self.clock.now_ms(),
        })
    }

    /// Per-device sync gate: returns `true` when the local device should
    /// fan a clipboard frame out to `device_id`. Two stages:
    ///
    /// 1. Device-level kill switch (`send_enabled`).
    /// 2. Content-type filter (`send_content_types`, AND-of-allowed across
    ///    the snapshot's category set — see `uc-core` `category.rs` module doc).
    ///    Empty set (raw-bytes / unrecognised payload) passes (fail open)
    ///    so we don't stall sync silently.
    ///
    /// Member-record miss / repo error → fail open with a WARN, mirroring
    /// the device-level gate's posture: a transient glitch should not
    /// silently kill sync.
    async fn is_send_allowed(
        &self,
        device_id: &DeviceId,
        categories: &ClipboardContentCategorySet,
    ) -> bool {
        match self.member_repo.get(device_id).await {
            Ok(Some(member)) => {
                if !member.sync_preferences.send_enabled {
                    info!(
                        device_id = %device_id.as_str(),
                        reason = "send_disabled_by_user",
                        "dispatch: skipping peer per per-device sync preferences"
                    );
                    return false;
                }
                if !categories.allowed_by(&member.sync_preferences.send_content_types) {
                    info!(
                        device_id = %device_id.as_str(),
                        categories = %categories.labels(),
                        denied = %categories
                            .denied_labels(&member.sync_preferences.send_content_types),
                        reason = "content_type_disabled_by_user",
                        "dispatch: skipping peer per per-device content_types filter"
                    );
                    return false;
                }
                true
            }
            Ok(None) => {
                warn!(
                    device_id = %device_id.as_str(),
                    "dispatch: peer in addr repo but missing from member repo; failing open"
                );
                true
            }
            Err(err) => {
                warn!(
                    device_id = %device_id.as_str(),
                    error = %err,
                    "dispatch: member repo lookup failed; failing open"
                );
                true
            }
        }
    }

    /// Load the device's own display name to embed in the outbound header
    /// so the peer can show "from <Alice's Laptop>". Falls back to the
    /// fingerprint if settings are unreadable or empty.
    async fn load_origin_device_name(&self) -> String {
        match self.settings.load().await {
            Ok(settings) => {
                if let Some(name) = settings.general.device_name {
                    if !name.is_empty() {
                        return name;
                    }
                }
            }
            Err(err) => {
                warn!(error = %err, "dispatch: settings load failed; using fingerprint fallback");
            }
        }
        match self.local_identity.get_current_fingerprint().await {
            Ok(Some(fp)) => fp.as_display().to_string(),
            _ => "unknown-device".to_string(),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================
//
// **Mocking convention** — locked in by Slice 2 Phase 1 T6 (`ensure_reachable_all`)
// and reaffirmed by Phase 2 plan §10 risk row 4:
//
// * Use `mockall::mock!` for ports whose tests assert call-count + return-
//   value behavior. Every Phase 1 use case test does this; we follow.
// * Use a hand-written fake **only** when ergonomics demand it:
//     - `subscribe()` returning a non-Clone `broadcast::Receiver` plus an
//       `emit(...)` helper to drive the test (see `roster/facade.rs` ::
//       `FakePresence` for the canonical example), or
//     - wall-time concurrency assertions where mockall's internal
//       `Mutex<FnMut>` would serialise concurrent `.returning()` closures
//       (Phase 1 T6's `SleepyPresence`).
//
// For this file: the dispatch use case calls 2 async ports + 4 read-only
// ports; no broadcast emit, no wall-time concurrency assertion. All six
// ports are mocked with `mockall::mock!`. `PresencePort` was dropped
// from this use case's deps once we stopped pre-filtering by online
// state (see module doc); peers are enumerated from `peer_addr_repo`
// only, and per-target offline verdicts come from the dispatch port.

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use chrono::Utc;
    use mockall::predicate::*;

    use uc_core::ports::security::{TransferCipherError, TransferCipherPort};
    use uc_core::ports::{
        ClockPort, DeviceIdentityPort, LocalIdentityError, LocalIdentityPort, PeerAddressError,
        PeerAddressRecord, PeerAddressRepositoryPort, SettingsPort,
    };
    use uc_core::security::IdentityFingerprint;
    use uc_core::settings::model::Settings;
    use uc_core::{MemberRepositoryPort, MemberSyncPreferences, MembershipError, SpaceMember};

    // ── mockall: PeerAddressRepositoryPort ──────────────────────────────

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

    // ── mockall: TransferCipherPort ─────────────────────────────────────

    mockall::mock! {
        pub Cipher {}

        #[async_trait]
        impl TransferCipherPort for Cipher {
            async fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, TransferCipherError>;
            async fn decrypt(&self, encrypted: &[u8]) -> Result<Vec<u8>, TransferCipherError>;
        }
    }

    // ── mockall: ClipboardDispatchPort ──────────────────────────────────
    //
    // The use case fan-outs via JoinSet, which spawns one task per target.
    // mockall's internal expectation `Mutex<FnMut>` would serialise
    // concurrent `.returning()` closures — but only when those closures
    // perform an `.await` that yields. Our `.returning(|_, _, _| ...)`
    // bodies return immediately, so there's nothing to serialise. The
    // Phase 1 T6 lesson (SleepyPresence) only applies when asserting
    // wall-time concurrency; per-target outcome assertions don't need it.

    mockall::mock! {
        pub Dispatch {}

        #[async_trait]
        impl ClipboardDispatchPort for Dispatch {
            async fn dispatch(
                &self,
                target: &DeviceId,
                header: &ClipboardHeader,
                payload: SyncPayload,
            ) -> Result<DispatchAck, ClipboardDispatchError>;
        }
    }

    // ── mockall: DeviceIdentityPort / LocalIdentityPort / SettingsPort ──

    mockall::mock! {
        pub DeviceId_ {}

        impl DeviceIdentityPort for DeviceId_ {
            fn current_device_id(&self) -> DeviceId;
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

    // ── mockall: MemberRepositoryPort ───────────────────────────────────

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

    // ── hand-written: ClockPort ─────────────────────────────────────────
    //
    // `ClockPort::now_ms` is sync + 4 lines; mockall's adapter would be
    // strictly more code than the hand-written `FixedClock`. Phase 1's
    // ensure_reachable_all uses the same pattern (`FixedDevice`).

    struct FixedClock(i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    // ── helpers ─────────────────────────────────────────────────────────

    fn fp(seed: u8) -> IdentityFingerprint {
        IdentityFingerprint::from_raw_string(
            (0..16)
                .map(|i| char::from(b'A' + ((seed as usize + i) % 26) as u8))
                .collect::<String>(),
        )
        .expect("valid fingerprint")
    }

    fn record(device: &str) -> PeerAddressRecord {
        PeerAddressRecord {
            device_id: DeviceId::new(device),
            addr_blob: vec![0xAA; 32],
            observed_at: Utc::now(),
        }
    }

    /// Build a `Settings` whose `general.device_name` round-trips to a
    /// stable header value.
    fn settings_with_device_name(name: &str) -> Settings {
        let mut s = Settings::default();
        s.general.device_name = Some(name.to_string());
        s
    }

    /// Wire the use case from a set of mock ports. The clock is always
    /// the same fixed value so header `captured_at_ms` assertions are
    /// deterministic.
    fn build_uc(
        peer_addr_repo: MockPeerAddrRepo,
        member_repo: MockMemberRepo,
        cipher: MockCipher,
        dispatch: MockDispatch,
        device_identity: MockDeviceId_,
        local_identity: MockLocalIdentity,
        settings: MockSettings_,
    ) -> DispatchClipboardEntryUseCase {
        DispatchClipboardEntryUseCase::new(
            Arc::new(peer_addr_repo),
            Arc::new(member_repo),
            Arc::new(cipher),
            Arc::new(dispatch),
            Arc::new(device_identity),
            Arc::new(local_identity),
            Arc::new(settings),
            Arc::new(FixedClock(1_700_000_000_000)),
        )
    }

    /// Build a `MemberRepo` mock that returns a stub `SpaceMember` with
    /// default (all-enabled) `sync_preferences` for every device. Used by
    /// the existing verdicts whose contract predates per-device gating —
    /// they should still observe the same fan-out behaviour.
    fn make_member_repo_all_enabled() -> MockMemberRepo {
        let mut m = MockMemberRepo::new();
        m.expect_get().returning(|did| {
            Ok(Some(SpaceMember {
                device_id: did.clone(),
                device_name: format!("Test {}", did.as_str()),
                identity_fingerprint: fp(0),
                joined_at: Utc::now(),
                sync_preferences: MemberSyncPreferences::default(),
            }))
        });
        m
    }

    /// Build a `DeviceIdentity` mock that returns the same `device_id`
    /// every call. Always-present helper because every test sets self.
    fn make_device_identity(local: &str) -> MockDeviceId_ {
        let local = DeviceId::new(local);
        let mut m = MockDeviceId_::new();
        m.expect_current_device_id()
            .returning(move || local.clone());
        m
    }

    /// Default settings + identity stubs that every test wires identically.
    fn make_local_identity_stub() -> MockLocalIdentity {
        let mut m = MockLocalIdentity::new();
        m.expect_get_current_fingerprint()
            .returning(|| Ok(Some(fp(7))));
        m
    }

    fn make_settings_stub() -> MockSettings_ {
        let mut m = MockSettings_::new();
        m.expect_load()
            .returning(|| Ok(settings_with_device_name("Test Device")));
        m
    }

    fn input() -> DispatchClipboardEntryInput {
        DispatchClipboardEntryInput {
            plaintext: Bytes::from_static(b"hello world"),
            content_hash: "9".repeat(64),
            payload_version: 3,
            // Existing verdicts predate the content-type filter; default
            // to an empty set so they always pass the gate (fail open).
            categories: ClipboardContentCategorySet::empty(),
        }
    }

    // ── verdicts ────────────────────────────────────────────────────────

    /// 1. Happy path — two paired peers, both accept. mockall asserts
    /// dispatch is called exactly twice (once per peer) and the encrypt
    /// path runs exactly once.
    #[tokio::test]
    async fn fan_outs_to_all_peers_and_counts_accepted() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-a"), record("peer-b")]));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-a")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-b")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let uc = build_uc(
            repo,
            make_member_repo_all_enabled(),
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        let outcome = uc.execute(input()).await.expect("dispatch ok");
        assert_eq!(outcome.total_accepted, 2);
        assert_eq!(outcome.total_offline, 0);
        assert_eq!(outcome.total_errored, 0);
        assert_eq!(outcome.per_target.len(), 2);
    }

    /// 2. Unreachable peer — dispatch port returns `Err(Offline)` for a
    /// peer the wire can't reach. The outcome reports it as offline
    /// instead of silently dropping it pre-flight; the other peer still
    /// gets the frame. This is the key contract change that fixes the
    /// "no online peers; skipping fan-out" silent regression where our
    /// local presence cache was empty because the peer dialed us first
    /// (accept-side only updates the peer's cache, not ours).
    #[tokio::test]
    async fn unreachable_peer_is_reported_offline_after_dispatch_attempt() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-on"), record("peer-off")]));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-on")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));
        // Crucial: dispatch IS called for `peer-off` (no pre-filter). The
        // port returns `Offline`, and the outcome surfaces that — callers
        // can then decide whether to retry or surface to the user.
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-off")), always(), always())
            .times(1)
            .returning(|_, _, _| Err(ClipboardDispatchError::Offline));

        let uc = build_uc(
            repo,
            make_member_repo_all_enabled(),
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        let outcome = uc.execute(input()).await.expect("dispatch ok");
        assert_eq!(outcome.total_accepted, 1);
        assert_eq!(outcome.total_offline, 1);
        assert_eq!(outcome.per_target.len(), 2);
    }

    /// 3. Self-filter — `peer_addr_repo` inadvertently contains the local
    /// device. mockall enforces self-skip: no dispatch expectation is
    /// registered for `self-device`, so a self-dial would panic.
    #[tokio::test]
    async fn skips_self_even_if_peer_addr_repo_contains_local_device() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("self-device"), record("peer-a")]));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-a")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let uc = build_uc(
            repo,
            make_member_repo_all_enabled(),
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        let outcome = uc.execute(input()).await.expect("dispatch ok");
        assert_eq!(outcome.per_target.len(), 1);
        assert_eq!(outcome.per_target[0].device_id.as_str(), "peer-a");
    }

    /// 4. Locked space — `transfer_cipher.encrypt` returns `NotUnlocked`.
    /// Use case short-circuits with `LockedSpace`. mockall enforces "no
    /// dispatch ever called" by registering zero dispatch expectations.
    #[tokio::test]
    async fn locked_space_short_circuits_before_dispatch() {
        // peer_addr_repo isn't reached — register zero expectations so an
        // accidental call would panic.
        let repo = MockPeerAddrRepo::new();

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|_| Err(TransferCipherError::NotUnlocked));

        let dispatch = MockDispatch::new();

        let uc = build_uc(
            repo,
            make_member_repo_all_enabled(),
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        let err = uc
            .execute(input())
            .await
            .expect_err("locked space must short-circuit");
        assert!(matches!(err, DispatchSyncError::LockedSpace));
    }

    /// 5. Mixed outcomes — one accept, one offline, one rejected. Each
    /// target's expectation is registered independently with `.with(...)`
    /// matching the device id; mockall picks the right one per call,
    /// ignoring task ordering (JoinSet).
    #[tokio::test]
    async fn mixed_per_target_outcomes_are_reported_independently() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list().times(1).returning(|| {
            Ok(vec![
                record("peer-ok"),
                record("peer-off"),
                record("peer-rej"),
            ])
        });

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-ok")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-off")), always(), always())
            .times(1)
            .returning(|_, _, _| Err(ClipboardDispatchError::Offline));
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-rej")), always(), always())
            .times(1)
            .returning(|_, _, _| Err(ClipboardDispatchError::PeerRejected("too big".to_string())));

        let uc = build_uc(
            repo,
            make_member_repo_all_enabled(),
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        let outcome = uc.execute(input()).await.expect("dispatch ok");
        assert_eq!(outcome.total_accepted, 1);
        assert_eq!(outcome.total_offline, 1);
        assert_eq!(outcome.total_errored, 1);
        assert_eq!(outcome.per_target.len(), 3);

        use std::collections::HashSet;
        let seen: HashSet<(String, String)> = outcome
            .per_target
            .iter()
            .map(|t| {
                let key = match &t.outcome {
                    Ok(DispatchAck::Accepted) => "accepted",
                    Ok(DispatchAck::DuplicateIgnored) => "duplicate",
                    Err(msg) if msg == "offline" => "offline",
                    Err(_) => "rejected",
                };
                (t.device_id.as_str().to_string(), key.to_string())
            })
            .collect();
        assert!(seen.contains(&("peer-ok".to_string(), "accepted".to_string())));
        assert!(seen.contains(&("peer-off".to_string(), "offline".to_string())));
        assert!(seen.contains(&("peer-rej".to_string(), "rejected".to_string())));
    }

    /// 6. Per-device send gate — `peer-mute` has `send_enabled=false` in
    /// its `MemberSyncPreferences`. The dispatch port must NEVER be
    /// invoked for it; the other peer still receives the frame. mockall
    /// enforces "no dispatch ever for peer-mute" by registering zero
    /// expectations on that arm — any sneaky call would panic.
    #[tokio::test]
    async fn send_disabled_peer_is_skipped_before_dispatch() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-on"), record("peer-mute")]));

        let mut member_repo = MockMemberRepo::new();
        member_repo
            .expect_get()
            .returning(|did| match did.as_str() {
                "peer-mute" => {
                    let mut prefs = MemberSyncPreferences::default();
                    prefs.send_enabled = false;
                    Ok(Some(SpaceMember {
                        device_id: did.clone(),
                        device_name: "Peer Mute".to_string(),
                        identity_fingerprint: fp(0),
                        joined_at: Utc::now(),
                        sync_preferences: prefs,
                    }))
                }
                _ => Ok(Some(SpaceMember {
                    device_id: did.clone(),
                    device_name: format!("Test {}", did.as_str()),
                    identity_fingerprint: fp(0),
                    joined_at: Utc::now(),
                    sync_preferences: MemberSyncPreferences::default(),
                })),
            });

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        // Only peer-on is allowed; peer-mute must never be dispatched to.
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-on")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));
        // No expect_dispatch for peer-mute → mockall would panic on call.

        let uc = build_uc(
            repo,
            member_repo,
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        let outcome = uc.execute(input()).await.expect("dispatch ok");
        assert_eq!(outcome.total_accepted, 1);
        assert_eq!(
            outcome.per_target.len(),
            1,
            "muted peer must not appear in per_target report"
        );
        assert_eq!(outcome.per_target[0].device_id.as_str(), "peer-on");
    }

    /// 7. Fail-open on member lookup miss — peer is in `peer_addr_repo`
    /// but `member_repo.get` returns `Ok(None)` (the two stores drifted).
    /// The dispatch port must still fire so a transient repo gap doesn't
    /// silently kill sync; the operator-visible signal is the WARN log.
    #[tokio::test]
    async fn missing_member_record_fails_open_and_still_dispatches() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-orphan")]));

        let mut member_repo = MockMemberRepo::new();
        member_repo
            .expect_get()
            .with(eq(DeviceId::new("peer-orphan")))
            .times(1)
            .returning(|_| Ok(None));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-orphan")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let uc = build_uc(
            repo,
            member_repo,
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        let outcome = uc.execute(input()).await.expect("dispatch ok");
        assert_eq!(outcome.total_accepted, 1);
        assert_eq!(outcome.per_target.len(), 1);
    }

    /// 8. Per-device content-type gate — `peer-no-text` has
    /// `send_content_types.text=false`. Dispatching a `Text` snapshot
    /// must skip that peer; the other peer (default-allowed) still gets
    /// the frame. mockall enforces "no dispatch ever for peer-no-text".
    #[tokio::test]
    async fn send_content_types_text_disabled_peer_is_skipped() {
        use uc_core::settings::model::ContentTypes;

        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-on"), record("peer-no-text")]));

        let mut member_repo = MockMemberRepo::new();
        member_repo
            .expect_get()
            .returning(|did| match did.as_str() {
                "peer-no-text" => {
                    let mut prefs = MemberSyncPreferences::default();
                    let mut ct = ContentTypes::default();
                    ct.text = false;
                    prefs.send_content_types = ct;
                    Ok(Some(SpaceMember {
                        device_id: did.clone(),
                        device_name: "Peer NoText".to_string(),
                        identity_fingerprint: fp(0),
                        joined_at: Utc::now(),
                        sync_preferences: prefs,
                    }))
                }
                _ => Ok(Some(SpaceMember {
                    device_id: did.clone(),
                    device_name: format!("Test {}", did.as_str()),
                    identity_fingerprint: fp(0),
                    joined_at: Utc::now(),
                    sync_preferences: MemberSyncPreferences::default(),
                })),
            });

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-on")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));
        // No expect_dispatch for peer-no-text → mockall would panic on call.

        let uc = build_uc(
            repo,
            member_repo,
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        // Hand-craft an input whose category set is `{Text}` — the
        // simplest scenario where the text-muted peer must be skipped.
        use uc_core::clipboard::ClipboardContentCategory;
        let mut categories = ClipboardContentCategorySet::empty();
        categories.insert(ClipboardContentCategory::Text);
        let text_input = DispatchClipboardEntryInput {
            plaintext: Bytes::from_static(b"hello world"),
            content_hash: "9".repeat(64),
            payload_version: 3,
            categories,
        };

        let outcome = uc.execute(text_input).await.expect("dispatch ok");
        assert_eq!(outcome.total_accepted, 1);
        assert_eq!(
            outcome.per_target.len(),
            1,
            "text-muted peer must not appear in per_target"
        );
        assert_eq!(outcome.per_target[0].device_id.as_str(), "peer-on");
    }

    /// 9. Empty category set bypasses content-type gate even when the
    /// peer has all content types disabled. Mirrors the CLI raw-bytes
    /// path where the snapshot can't be classified — must fail open.
    #[tokio::test]
    async fn empty_category_set_bypasses_content_types_filter() {
        use uc_core::settings::model::ContentTypes;

        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-strict")]));

        let mut member_repo = MockMemberRepo::new();
        member_repo
            .expect_get()
            .with(eq(DeviceId::new("peer-strict")))
            .times(1)
            .returning(|did| {
                let mut prefs = MemberSyncPreferences::default();
                // Every content type off — only an empty category set should pass.
                let mut ct = ContentTypes::default();
                ct.text = false;
                ct.image = false;
                ct.link = false;
                ct.file = false;
                ct.code_snippet = false;
                ct.rich_text = false;
                prefs.send_content_types = ct;
                Ok(Some(SpaceMember {
                    device_id: did.clone(),
                    device_name: "Peer Strict".to_string(),
                    identity_fingerprint: fp(0),
                    joined_at: Utc::now(),
                    sync_preferences: prefs,
                }))
            });

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-strict")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let uc = build_uc(
            repo,
            member_repo,
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        // input() defaults to an empty `ClipboardContentCategorySet` — an
        // unrecognised payload should fail open even against an all-off filter.
        let outcome = uc.execute(input()).await.expect("dispatch ok");
        assert_eq!(outcome.total_accepted, 1);
    }
}
