//! `ApplyInboundActiveClipboardStateUseCase` — drives the inbound
//! active-clipboard register state (0xC3) toward convergence.
//!
//! Per inbound observation `S` from peer `P`, in order (issue #1017 §4):
//!
//! 1. **Locked → drop.** A locked device is fully lazy (no register, no OS
//!    write, no re-broadcast) — it cannot decrypt content anyway.
//! 2. **Not newer / same activation → ignore.** The register is a convergent
//!    LWW value; an observation that does not supersede the stored value, or
//!    that *is* the stored value (full-key match), is already known — applying
//!    or re-broadcasting it would loop.
//! 3. **Future-timestamp guard → drop.** Reject an activation timestamp far
//!    ahead of the local wall clock so a fast-clocked peer can't pin the
//!    register and suppress real later activations.
//! 4. **Receive gate → drop.** A peer the user muted (or a denied content
//!    type) must not write our OS clipboard. A rejected observation advances
//!    nothing and is not re-broadcast, so its timestamp can never suppress a
//!    later legitimate one.
//! 5. **Content present locally → write OS, advance register, re-broadcast.**
//!    The OS write is detached; only its success advances the register and
//!    triggers the same-key re-broadcast, realizing the core invariant
//!    "register advanced ⟺ OS write succeeded ⟺ re-broadcast".
//! 6. **Content missing locally → log + return.** Pulling the content from the
//!    sender is PR8; this branch leaves the register untouched.

use std::sync::Arc;

use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{debug, info, instrument, warn};

use uc_core::clipboard::{ActiveClipboardState, ClipboardContentCategorySet};
use uc_core::ids::SpaceId;
use uc_core::ports::clipboard::{
    ActiveClipboardDispatchPort, ActiveClipboardReceiverPort, AdvanceActiveClipboardPort,
    FindEntryIdBySnapshotHashPort, InboundActiveClipboardState, LoadActiveClipboardPort,
};
use uc_core::ports::space::IsSpaceUnlockedPort;
use uc_core::ports::{ClockPort, PeerAddressRepositoryPort};
use uc_core::MemberRepositoryPort;

use crate::clipboard_write::{ClipboardWriteCoordinator, ClipboardWriteIntent};

use super::active_state_fanout::fan_out_active_state;
use super::receive_gate::MemberReceiveGate;
use super::send_gate::MemberSendGate;
use super::snapshot_from_entry::SnapshotReconstructor;

/// The fixed space id of the single-space model. Active-clipboard state is
/// only meaningful while that space is unlocked.
const DEFAULT_SPACE_ID: &str = "space";

/// Reject an incoming activation timestamp this far ahead of the local wall
/// clock (issue #1017 D9). Bounds the damage a fast-clocked peer can do: a
/// state stamped wildly in the future would otherwise win every LWW
/// comparison and pin the register, suppressing real later activations.
const FUTURE_TIMESTAMP_TOLERANCE_MS: i64 = 300_000; // 300s

/// Handle owning the spawned inbound active-clipboard loop. Drop or
/// `abort()` to stop it; the loop also exits on its own when the receiver
/// adapter shuts down (its broadcast senders drop).
///
/// `pub` (not `pub(crate)`) so bootstrap can hold the loop's lifetime via the
/// facade re-export; the use case itself stays `pub(crate)`.
pub struct ActiveClipboardInboundHandle {
    join: JoinHandle<()>,
}

impl ActiveClipboardInboundHandle {
    pub fn abort(&self) {
        self.join.abort();
    }
}

impl Drop for ActiveClipboardInboundHandle {
    fn drop(&mut self) {
        self.join.abort();
    }
}

/// Drives one device's inbound active-clipboard state toward convergence.
pub(crate) struct ApplyInboundActiveClipboardStateUseCase {
    receiver: Arc<dyn ActiveClipboardReceiverPort>,
    is_unlocked: Arc<dyn IsSpaceUnlockedPort>,
    load_register: Arc<dyn LoadActiveClipboardPort>,
    advance_register: Arc<dyn AdvanceActiveClipboardPort>,
    receive_gate: MemberReceiveGate,
    entry_lookup: Arc<dyn FindEntryIdBySnapshotHashPort>,
    reconstructor: SnapshotReconstructor,
    coordinator: Arc<ClipboardWriteCoordinator>,
    dispatch: Arc<dyn ActiveClipboardDispatchPort>,
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    send_gate: MemberSendGate,
    clock: Arc<dyn ClockPort>,
}

impl ApplyInboundActiveClipboardStateUseCase {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        receiver: Arc<dyn ActiveClipboardReceiverPort>,
        is_unlocked: Arc<dyn IsSpaceUnlockedPort>,
        load_register: Arc<dyn LoadActiveClipboardPort>,
        advance_register: Arc<dyn AdvanceActiveClipboardPort>,
        member_repo: Arc<dyn MemberRepositoryPort>,
        entry_lookup: Arc<dyn FindEntryIdBySnapshotHashPort>,
        reconstructor: SnapshotReconstructor,
        coordinator: Arc<ClipboardWriteCoordinator>,
        dispatch: Arc<dyn ActiveClipboardDispatchPort>,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self {
            receiver,
            is_unlocked,
            load_register,
            advance_register,
            receive_gate: MemberReceiveGate::new(Arc::clone(&member_repo)),
            entry_lookup,
            reconstructor,
            coordinator,
            dispatch,
            peer_addr_repo,
            send_gate: MemberSendGate::new(member_repo),
            clock,
        }
    }

    /// Spawn the inbound loop. Takes `Arc<Self>` so the spawned task owns the
    /// use case's dependencies without moving them out of the owning facade.
    pub(crate) fn spawn_run(self: Arc<Self>) -> ActiveClipboardInboundHandle {
        let uc = Arc::clone(&self);
        let join = tokio::spawn(async move { uc.run().await });
        ActiveClipboardInboundHandle { join }
    }

    #[instrument(name = "active_state.inbound_loop", skip_all)]
    async fn run(self: Arc<Self>) {
        let mut rx = self.receiver.subscribe();
        loop {
            match rx.recv().await {
                Ok(inbound) => self.handle_one(inbound).await,
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    warn!(
                        missed,
                        "active state inbound receiver lagged; dropped observations"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("active state inbound receiver closed; exiting loop");
                    break;
                }
            }
        }
    }

    fn space_id() -> SpaceId {
        SpaceId::from(DEFAULT_SPACE_ID)
    }

    /// Handle one inbound observation end-to-end. Always returns; every
    /// failure mode is a logged drop (the register is convergent, so a
    /// dropped observation is recovered by the next one a peer reports).
    #[instrument(
        name = "active_state.apply_inbound",
        skip_all,
        fields(
            peer.device_id = %inbound.peer_device_id.as_str(),
            content_hash = %inbound.content_hash,
            activated_at_ms = inbound.activated_at_ms,
        ),
    )]
    pub(crate) async fn handle_one(&self, inbound: InboundActiveClipboardState) {
        let peer = inbound.peer_device_id.clone();
        let incoming = ActiveClipboardState::new(
            inbound.content_hash,
            // Placeholder: the cross-device identity is `content_hash`; the
            // sender's `entry_id` is never used to resolve local content, so
            // we keep the sender's value only for LWW/loop comparison and
            // overwrite it with the local entry id before advancing.
            uc_core::ids::EntryId::from(inbound.sender_entry_id),
            inbound.activated_at_ms,
            inbound.activated_by,
        );

        // 1. Locked → fully lazy (D5).
        if !self.is_unlocked.is_unlocked(&Self::space_id()).await {
            debug!("active state inbound dropped: space locked");
            return;
        }

        // 2. LWW / loop-stop. Load the current register and compare on the
        //    full activation key. A value that does not supersede the stored
        //    one (older, or an exact-key duplicate that is our own / already
        //    known) is ignored without an OS write or re-broadcast.
        let current = match self.load_register.load().await {
            Ok(c) => c,
            Err(err) => {
                warn!(error = %err, "active state inbound dropped: register load failed");
                return;
            }
        };
        if let Some(current) = &current {
            if incoming.is_same_activation(current) {
                debug!("active state inbound ignored: same activation already converged");
                return;
            }
            if !incoming.supersedes(current) {
                debug!("active state inbound ignored: stale under LWW order");
                return;
            }
        }

        // 3. Future-timestamp guard (D9).
        let now_ms = self.clock.now_ms();
        if incoming.activated_at_ms > now_ms + FUTURE_TIMESTAMP_TOLERANCE_MS {
            warn!(
                now_ms,
                tolerance_ms = FUTURE_TIMESTAMP_TOLERANCE_MS,
                "active state inbound dropped: activation timestamp too far in the future"
            );
            return;
        }

        // 4. Receive gate stage 1 — device-level kill switch (D2). A muted
        //    peer writes nothing here: no OS write, no register advance (so a
        //    rejected item can't suppress later legit ones via its ts), no
        //    re-broadcast (loop-safe).
        if !self.receive_gate.is_receive_allowed(&peer).await {
            return;
        }

        // 5. Resolve the content locally by `content_hash` (never by the
        //    sender's per-device entry_id).
        let local_entry_id = match self
            .entry_lookup
            .find_entry_id_by_snapshot_hash(&incoming.content_hash)
            .await
        {
            Ok(Some(id)) => id,
            Ok(None) => {
                // Content missing locally. Pulling it from the sender is PR8
                // (issue #1017 §6); until then leave the register untouched
                // so a later observation that *does* carry resolvable content
                // can still converge.
                //
                // TODO(PR8): pull the content from `peer` (10s timeout, V3
                // decrypt→re-encrypt; blob sub-path re-signs the ticket),
                // then fall through to the OS-write + advance + re-broadcast
                // branch below.
                info!("active state inbound: content not held locally; deferring to pull (PR8)");
                return;
            }
            Err(err) => {
                warn!(error = %err, "active state inbound dropped: entry lookup failed");
                return;
            }
        };

        // Reconstruct the snapshot for the resolved entry. A reconstruction
        // failure (payload lost / locked / blob unavailable) means we cannot
        // honour the activation — drop without advancing.
        let snapshot = match self.reconstructor.reconstruct(&local_entry_id).await {
            Ok(s) => s,
            Err(err) => {
                warn!(error = %err, entry_id = %local_entry_id, "active state inbound dropped: snapshot reconstruct failed");
                return;
            }
        };

        // Receive gate stage 2 — content-type filter (D2). Categories are
        // only known once the snapshot is reconstructed, so this runs here.
        let categories = ClipboardContentCategorySet::from_snapshot(&snapshot);
        if !self
            .receive_gate
            .is_receive_category_allowed(&peer, &categories)
            .await
        {
            return;
        }

        // 6. Schedule the detached OS write. The register advance + the
        //    re-broadcast live in the write task's success branch so they
        //    fire iff the OS write succeeded (core invariant). The write is
        //    detached because OS clipboard writes can block 1–3s on some
        //    platforms; coupling them inline would stall the inbound loop.
        let advance_state = ActiveClipboardState::new(
            incoming.content_hash.clone(),
            local_entry_id.clone(),
            incoming.activated_at_ms,
            incoming.activated_by.clone(),
        );
        self.spawn_write_then_converge(snapshot, advance_state, categories);
    }

    /// Spawn the OS write; on success advance the register (SQL CAS enforces
    /// LWW) and re-broadcast the same-key state to allowed peers. `categories`
    /// is the activation's content category set, threaded into the outbound
    /// gate (`send_content_types`) of the shared fan-out.
    fn spawn_write_then_converge(
        &self,
        snapshot: uc_core::SystemClipboardSnapshot,
        state: ActiveClipboardState,
        categories: ClipboardContentCategorySet,
    ) -> JoinHandle<()> {
        let coordinator = Arc::clone(&self.coordinator);
        let advance_register = Arc::clone(&self.advance_register);
        let dispatch = Arc::clone(&self.dispatch);
        let peer_addr_repo = Arc::clone(&self.peer_addr_repo);
        let send_gate = self.send_gate.clone();

        tokio::spawn(async move {
            // The active-clipboard write is a remote-originated push: use the
            // RemotePush intent so the OS-write origin guard matches the bulk
            // inbound path (avoids the watcher re-capturing our own write).
            if let Err(err) = coordinator
                .write(snapshot, ClipboardWriteIntent::RemotePush)
                .await
            {
                warn!(
                    error = %err,
                    content_hash = %state.content_hash,
                    "active state inbound: OS write failed; not advancing register or re-broadcasting"
                );
                return;
            }

            // OS write succeeded → advance the register. The SQL CAS is the
            // authoritative LWW arbiter; `advanced == false` means a
            // concurrent local/inbound write already moved the register past
            // this state, in which case we must NOT re-broadcast (loop-safe).
            match advance_register.advance(&state).await {
                Ok(true) => {}
                Ok(false) => {
                    debug!(
                        content_hash = %state.content_hash,
                        "active state inbound: register did not advance (lost LWW race); skipping re-broadcast"
                    );
                    return;
                }
                Err(err) => {
                    warn!(
                        error = %err,
                        content_hash = %state.content_hash,
                        "active state inbound: register advance failed; skipping re-broadcast"
                    );
                    return;
                }
            }

            // Re-broadcast the converged state to every allowed peer through
            // the shared fan-out (full outbound gate: send_enabled ∧
            // send_content_types, the latter via the activation's category
            // set). Same implementation as the restore broadcast path.
            fan_out_active_state(&dispatch, &peer_addr_repo, &send_gate, &state, &categories).await;
        })
    }
}

// ============================================================================
// Tests
// ============================================================================
//
// These exercise the early-return gates (locked / LWW-loop / clock-guard /
// receive). All of them return *before* the entry lookup, reconstruct, OS
// write, register advance, and re-broadcast, so a spy on those side effects
// asserts "nothing happened". The OS-write success path (content present →
// advance + re-broadcast) is covered end-to-end by the bootstrap/e2e layer
// where a real coordinator + reconstructor are wired.

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use chrono::Utc;
    use uc_core::blob::ports::BlobReaderPort;
    use uc_core::clipboard::{
        ClipboardEntry, ClipboardRepositoryError, ClipboardSelectionDecision, PayloadAvailability,
        PersistedClipboardRepresentation, SystemClipboardSnapshot,
    };
    use uc_core::ids::{DeviceId, EntryId, EventId, RepresentationId, SpaceId};
    use uc_core::membership::{MembershipError, SpaceMember};
    use uc_core::ports::clipboard::{
        ActiveClipboardRegisterError, ClipboardPayloadResolverPort, GetClipboardEntryPort,
        GetRepresentationPort, PayloadResolveError, ProcessingUpdateOutcome,
        ResolvedClipboardPayload, UpdateRepresentationProcessingResultPort,
    };
    use uc_core::ports::{
        ClipboardSelectionRepositoryPort, PeerAddressError, PeerAddressRecord, SystemClipboardPort,
    };
    use uc_core::{BlobId, MemberSyncPreferences};

    use crate::clipboard_write::ClipboardWriteCoordinator;

    // ---- spies / fakes ------------------------------------------------------

    struct FixedClock(i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    struct FixedUnlocked(bool);
    #[async_trait]
    impl IsSpaceUnlockedPort for FixedUnlocked {
        async fn is_unlocked(&self, _space_id: &SpaceId) -> bool {
            self.0
        }
    }

    struct FixedRegister(Option<ActiveClipboardState>);
    #[async_trait]
    impl LoadActiveClipboardPort for FixedRegister {
        async fn load(&self) -> Result<Option<ActiveClipboardState>, ActiveClipboardRegisterError> {
            Ok(self.0.clone())
        }
    }

    /// Spies on `advance` — the early-return tests assert it is never called.
    #[derive(Default)]
    struct AdvanceSpy {
        calls: AtomicUsize,
    }
    #[async_trait]
    impl AdvanceActiveClipboardPort for AdvanceSpy {
        async fn advance(
            &self,
            _state: &ActiveClipboardState,
        ) -> Result<bool, ActiveClipboardRegisterError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        }
    }

    /// Spies on `dispatch` — early-return tests assert it is never called.
    #[derive(Default)]
    struct DispatchSpy {
        calls: AtomicUsize,
    }
    #[async_trait]
    impl ActiveClipboardDispatchPort for DispatchSpy {
        async fn dispatch(
            &self,
            _target: &DeviceId,
            _state: &ActiveClipboardState,
        ) -> Result<(), uc_core::ports::clipboard::ActiveClipboardDispatchError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// `find_entry_id_by_snapshot_hash` must NOT be reached by the early-return
    /// gates — calling it is a test failure.
    struct EntryLookupNeverCalled;
    #[async_trait]
    impl FindEntryIdBySnapshotHashPort for EntryLookupNeverCalled {
        async fn find_entry_id_by_snapshot_hash(
            &self,
            _hash: &str,
        ) -> Result<Option<EntryId>, ClipboardRepositoryError> {
            panic!("entry lookup reached past an early-return gate");
        }
    }

    struct MemberRepoStub {
        receive_enabled: bool,
    }
    #[async_trait]
    impl MemberRepositoryPort for MemberRepoStub {
        async fn get(&self, device_id: &DeviceId) -> Result<Option<SpaceMember>, MembershipError> {
            let mut prefs = MemberSyncPreferences::default();
            prefs.receive_enabled = self.receive_enabled;
            Ok(Some(SpaceMember {
                device_id: device_id.clone(),
                device_name: "peer".to_string(),
                identity_fingerprint: uc_core::security::IdentityFingerprint::from_raw_string(
                    "0123456789abcdef",
                )
                .expect("valid test fingerprint"),
                joined_at: Utc::now(),
                sync_preferences: prefs,
            }))
        }
        async fn list(&self) -> Result<Vec<SpaceMember>, MembershipError> {
            Ok(vec![])
        }
        async fn save(&self, _member: &SpaceMember) -> Result<(), MembershipError> {
            Ok(())
        }
        async fn remove(&self, _device_id: &DeviceId) -> Result<bool, MembershipError> {
            Ok(false)
        }
    }

    #[derive(Default)]
    struct EmptyPeerAddrRepo;
    #[async_trait]
    impl PeerAddressRepositoryPort for EmptyPeerAddrRepo {
        async fn get(
            &self,
            _device: &DeviceId,
        ) -> Result<Option<PeerAddressRecord>, PeerAddressError> {
            Ok(None)
        }
        async fn upsert(&self, _record: &PeerAddressRecord) -> Result<(), PeerAddressError> {
            Ok(())
        }
        async fn list(&self) -> Result<Vec<PeerAddressRecord>, PeerAddressError> {
            Ok(vec![])
        }
        async fn remove(&self, _device: &DeviceId) -> Result<(), PeerAddressError> {
            Ok(())
        }
    }

    /// System clipboard whose `write_snapshot` panics — proves no OS write is
    /// attempted on an early-return path.
    struct NoWriteClipboard;
    impl SystemClipboardPort for NoWriteClipboard {
        fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
            unreachable!("read_snapshot must not be called")
        }
        fn write_snapshot(&self, _snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
            panic!("OS write reached past an early-return gate");
        }
    }

    /// Inert change-origin port for the coordinator (never reached on the
    /// early-return paths; only the required methods are implemented).
    struct StubOrigin;
    #[async_trait]
    impl uc_core::ports::clipboard::ClipboardChangeOriginPort for StubOrigin {
        async fn set_next_origin(
            &self,
            _origin: uc_core::ClipboardChangeOrigin,
            _ttl: std::time::Duration,
        ) {
        }
        async fn consume_origin_or_default(
            &self,
            default_origin: uc_core::ClipboardChangeOrigin,
        ) -> uc_core::ClipboardChangeOrigin {
            default_origin
        }
    }

    /// Reconstructor ports that all panic — none should be reached on an
    /// early-return path.
    struct ReconstructNeverCalled;
    #[async_trait]
    impl GetClipboardEntryPort for ReconstructNeverCalled {
        async fn get_entry(
            &self,
            _entry_id: &EntryId,
        ) -> Result<Option<ClipboardEntry>, ClipboardRepositoryError> {
            panic!("reconstruct reached past an early-return gate");
        }
    }
    #[async_trait]
    impl ClipboardSelectionRepositoryPort for ReconstructNeverCalled {
        async fn get_selection(
            &self,
            _entry_id: &EntryId,
        ) -> anyhow::Result<Option<ClipboardSelectionDecision>> {
            panic!("reconstruct reached past an early-return gate");
        }
        async fn delete_selection(&self, _entry_id: &EntryId) -> anyhow::Result<()> {
            unreachable!()
        }
    }
    #[async_trait]
    impl GetRepresentationPort for ReconstructNeverCalled {
        async fn get_representation(
            &self,
            _event_id: &EventId,
            _representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>, ClipboardRepositoryError> {
            panic!("reconstruct reached past an early-return gate");
        }
    }
    #[async_trait]
    impl UpdateRepresentationProcessingResultPort for ReconstructNeverCalled {
        async fn update_processing_result(
            &self,
            _rep_id: &RepresentationId,
            _expected_states: &[PayloadAvailability],
            _blob_id: Option<&BlobId>,
            _new_state: PayloadAvailability,
            _last_error: Option<&str>,
        ) -> Result<ProcessingUpdateOutcome, ClipboardRepositoryError> {
            unreachable!()
        }
    }
    #[async_trait]
    impl ClipboardPayloadResolverPort for ReconstructNeverCalled {
        async fn resolve(
            &self,
            _rep: &PersistedClipboardRepresentation,
        ) -> Result<ResolvedClipboardPayload, PayloadResolveError> {
            panic!("reconstruct reached past an early-return gate");
        }
    }
    #[async_trait]
    impl BlobReaderPort for ReconstructNeverCalled {
        async fn get(&self, _blob_id: &BlobId) -> anyhow::Result<Vec<u8>> {
            unreachable!()
        }
    }

    // ---- harness ------------------------------------------------------------

    struct Harness {
        advance: Arc<AdvanceSpy>,
        dispatch: Arc<DispatchSpy>,
        uc: ApplyInboundActiveClipboardStateUseCase,
    }

    /// A receiver port stub — `handle_one` is driven directly, so the loop /
    /// subscribe seam is not exercised here.
    struct NoopReceiver;
    #[async_trait]
    impl ActiveClipboardReceiverPort for NoopReceiver {
        fn subscribe(&self) -> tokio::sync::broadcast::Receiver<InboundActiveClipboardState> {
            let (_tx, rx) = tokio::sync::broadcast::channel(1);
            rx
        }
    }

    fn harness(
        unlocked: bool,
        register: Option<ActiveClipboardState>,
        receive_enabled: bool,
        now_ms: i64,
    ) -> Harness {
        let advance = Arc::new(AdvanceSpy::default());
        let dispatch = Arc::new(DispatchSpy::default());
        let reconstructor = SnapshotReconstructor::new(
            Arc::new(ReconstructNeverCalled),
            Arc::new(ReconstructNeverCalled),
            Arc::new(ReconstructNeverCalled),
            Arc::new(ReconstructNeverCalled),
            Arc::new(ReconstructNeverCalled),
            Arc::new(ReconstructNeverCalled),
        );
        let coordinator = Arc::new(ClipboardWriteCoordinator::new(
            Arc::new(NoWriteClipboard),
            Arc::new(StubOrigin),
        ));
        let uc = ApplyInboundActiveClipboardStateUseCase::new(
            Arc::new(NoopReceiver),
            Arc::new(FixedUnlocked(unlocked)),
            Arc::new(FixedRegister(register)),
            Arc::clone(&advance) as Arc<dyn AdvanceActiveClipboardPort>,
            Arc::new(MemberRepoStub { receive_enabled }),
            Arc::new(EntryLookupNeverCalled),
            reconstructor,
            coordinator,
            Arc::clone(&dispatch) as Arc<dyn ActiveClipboardDispatchPort>,
            Arc::new(EmptyPeerAddrRepo),
            Arc::new(FixedClock(now_ms)),
        );
        Harness {
            advance,
            dispatch,
            uc,
        }
    }

    fn inbound(content_hash: &str, ts: i64, by: &str) -> InboundActiveClipboardState {
        InboundActiveClipboardState {
            peer_device_id: DeviceId::new("peer-p"),
            content_hash: content_hash.to_string(),
            sender_entry_id: "sender-entry".to_string(),
            activated_at_ms: ts,
            activated_by: DeviceId::new(by),
        }
    }

    fn assert_inert(h: &Harness) {
        assert_eq!(
            h.advance.calls.load(Ordering::SeqCst),
            0,
            "register must not advance on an early-return gate"
        );
        assert_eq!(
            h.dispatch.calls.load(Ordering::SeqCst),
            0,
            "no re-broadcast on an early-return gate"
        );
    }

    #[tokio::test]
    async fn locked_device_drops_without_touching_register() {
        let h = harness(false, None, true, 1_000);
        h.uc.handle_one(inbound("blake3v1:aa", 1_000, "dev-x"))
            .await;
        assert_inert(&h);
    }

    #[tokio::test]
    async fn same_activation_is_a_noop() {
        let stored =
            ActiveClipboardState::new("blake3v1:aa", EntryId::new(), 500, DeviceId::new("dev-x"));
        // Incoming carries the same full key (different sender entry_id only).
        let h = harness(true, Some(stored), true, 10_000);
        h.uc.handle_one(inbound("blake3v1:aa", 500, "dev-x")).await;
        assert_inert(&h);
    }

    #[tokio::test]
    async fn stale_under_lww_is_a_noop() {
        let stored =
            ActiveClipboardState::new("blake3v1:bb", EntryId::new(), 900, DeviceId::new("dev-x"));
        // Older timestamp than the stored value → does not supersede.
        let h = harness(true, Some(stored), true, 10_000);
        h.uc.handle_one(inbound("blake3v1:aa", 800, "dev-x")).await;
        assert_inert(&h);
    }

    #[tokio::test]
    async fn future_timestamp_is_rejected() {
        // now=1_000, tolerance=300_000 → anything past 301_000 is rejected.
        let h = harness(true, None, true, 1_000);
        h.uc.handle_one(inbound(
            "blake3v1:aa",
            1_000 + FUTURE_TIMESTAMP_TOLERANCE_MS + 1,
            "dev-x",
        ))
        .await;
        assert_inert(&h);
    }

    #[tokio::test]
    async fn receive_disabled_peer_is_dropped() {
        // Unlocked, newer than empty register, sane clock — only the receive
        // gate stops it. Entry lookup / reconstruct / OS write would panic if
        // reached.
        let h = harness(true, None, false, 1_000);
        h.uc.handle_one(inbound("blake3v1:aa", 1_000, "dev-x"))
            .await;
        assert_inert(&h);
    }
}
