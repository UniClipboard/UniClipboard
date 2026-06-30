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
//! 6. **Content missing locally → pull from the sender (PR8).** Request the
//!    transfer envelope from the reporting peer (10s deadline), decrypt +
//!    store it, then fall through to the same OS-write → advance →
//!    re-broadcast tail. Any pull failure (timeout / offline / locked / decode
//!    / store) is a logged drop: no register advance, no re-broadcast, no
//!    retry. The pull/store seam is optional — when unwired this branch logs
//!    and returns, leaving the register untouched.

use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{debug, info, instrument, warn};

use uc_core::clipboard::{ActiveClipboardState, ClipboardContentCategorySet};
use uc_core::ids::{DeviceId, EntryId, SpaceId};
use uc_core::ports::clipboard::{
    ActiveClipboardDispatchPort, ActiveClipboardPullClientError, ActiveClipboardPullClientPort,
    ActiveClipboardReceiverPort, AdvanceActiveClipboardPort, CheckEntryAvailabilityPort,
    FindEntryIdBySnapshotHashPort, InboundActiveClipboardState, LoadActiveClipboardPort,
};
use uc_core::ports::space::IsSpaceUnlockedPort;
use uc_core::ports::{ClockPort, PeerAddressRepositoryPort, PresencePort};
use uc_core::MemberRepositoryPort;

use crate::clipboard_write::{ClipboardWriteCoordinator, ClipboardWriteIntent};

use super::super::receive_gate::MemberReceiveGate;
use super::super::send_gate::MemberSendGate;
use super::super::snapshot_from_entry::SnapshotReconstructor;
use super::fanout::fan_out_active_state;

/// The fixed space id of the single-space model. Active-clipboard state is
/// only meaningful while that space is unlocked.
const DEFAULT_SPACE_ID: &str = "space";

/// Reject an incoming activation timestamp this far ahead of the local wall
/// clock (issue #1017 D9). Bounds the damage a fast-clocked peer can do: a
/// state stamped wildly in the future would otherwise win every LWW
/// comparison and pin the register, suppressing real later activations.
const FUTURE_TIMESTAMP_TOLERANCE_MS: i64 = 300_000; // 300s

/// Emitted when the inbound active-clipboard register advances successfully.
/// External subscribers (e.g. a resurface worker) react to this; the
/// convergence use case itself does not touch clipboard history ordering.
#[derive(Debug, Clone)]
pub(crate) struct ActiveClipboardConvergedEvent {
    pub entry_id: EntryId,
}

/// Failure surface for storing a pulled transfer envelope locally.
#[derive(Debug, Error)]
pub(crate) enum InboundPulledContentStoreError {
    /// The envelope could not be decrypted (e.g. the session locked between
    /// the pull and the store, or the bytes were malformed / tampered).
    #[error("pulled content decrypt failed: {0}")]
    Decrypt(String),
    /// The decrypted envelope could not be decoded / persisted.
    #[error("pulled content store failed: {0}")]
    Store(String),
}

/// Decrypt + persist a pulled transfer envelope, returning the local entry id.
///
/// This is the inbound store half of the pull path: the requester has the
/// transfer-encrypted envelope a peer served and needs it materialized into a
/// local entry (decrypt → decode V3 → materialize blobs → persist) so the
/// active-clipboard convergence tail can resolve + write it. The store does
/// **not** advance the active-clipboard register or re-broadcast — that stays
/// with the convergence tail, which couples the advance to OS-write success.
#[async_trait]
pub(crate) trait InboundPulledContentStore: Send + Sync {
    /// Decrypt `transfer_envelope` (the bytes a peer served), persist the
    /// content as an entry attributed to `from_device`, and return its local
    /// entry id. `snapshot_hash` is the cross-device identity used for dedup.
    async fn store(
        &self,
        from_device: &DeviceId,
        snapshot_hash: &str,
        transfer_envelope: Vec<u8>,
    ) -> Result<EntryId, InboundPulledContentStoreError>;
}

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
    /// Reachability tracker: the re-broadcast fan-out skips peers already known
    /// offline rather than burning a dial timeout per stale/ghost roster entry.
    presence: Arc<dyn PresencePort>,
    send_gate: MemberSendGate,
    clock: Arc<dyn ClockPort>,
    /// On-demand pull of content this device observed but does not hold (D6).
    /// `None` when the pull subsystem is unwired — the "content missing"
    /// branch then logs and returns without converging.
    pull_client: Option<Arc<dyn ActiveClipboardPullClientPort>>,
    /// Decrypts + persists a pulled transfer envelope. Paired with
    /// `pull_client`; both are wired together or not at all.
    pulled_content_store: Option<Arc<dyn InboundPulledContentStore>>,
    /// Live availability query. When wired, a hash match is only converged if
    /// the matched entry is fully held; a partial match (e.g. a cancelled
    /// transfer placeholder) is treated as "not held" and pulled, so a
    /// `uniclip-missing://` placeholder is never written to the OS clipboard.
    /// `None` keeps the prior "any hash match converges" behavior.
    availability: Option<Arc<dyn CheckEntryAvailabilityPort>>,
    /// Domain event sender: fires after a successful register advance so
    /// external subscribers can react (e.g. resurface the entry in history).
    converged_tx: broadcast::Sender<ActiveClipboardConvergedEvent>,
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
        presence: Arc<dyn PresencePort>,
        clock: Arc<dyn ClockPort>,
        converged_tx: broadcast::Sender<ActiveClipboardConvergedEvent>,
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
            presence,
            send_gate: MemberSendGate::new(member_repo),
            clock,
            pull_client: None,
            pulled_content_store: None,
            availability: None,
            converged_tx,
        }
    }

    /// Wire the availability query so a partial local entry (matched by hash but
    /// not fully held) is pulled and completed before converging, rather than
    /// writing its `uniclip-missing://` placeholder to the OS clipboard. Without
    /// it, any hash match converges (prior behavior).
    pub(crate) fn with_check_entry_availability(
        mut self,
        availability: Arc<dyn CheckEntryAvailabilityPort>,
    ) -> Self {
        self.availability = Some(availability);
        self
    }

    /// Subscribe to convergence events.
    pub(crate) fn subscribe_converged(&self) -> broadcast::Receiver<ActiveClipboardConvergedEvent> {
        self.converged_tx.subscribe()
    }

    /// Wire the on-demand pull subsystem (issue #1017 PR8). When set, the
    /// "content missing locally" branch pulls the transfer envelope from the
    /// reporting peer (10s deadline), stores it, and converges; without it
    /// that branch logs and returns. The two ports are wired together — a pull
    /// is useless without the store and vice versa.
    pub(crate) fn with_pull(
        mut self,
        pull_client: Arc<dyn ActiveClipboardPullClientPort>,
        pulled_content_store: Arc<dyn InboundPulledContentStore>,
    ) -> Self {
        self.pull_client = Some(pull_client);
        self.pulled_content_store = Some(pulled_content_store);
        self
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
            snapshot_hash = %inbound.snapshot_hash,
            activated_at_ms = inbound.activated_at_ms,
        ),
    )]
    pub(crate) async fn handle_one(&self, inbound: InboundActiveClipboardState) {
        let peer = inbound.peer_device_id.clone();
        let incoming = ActiveClipboardState::new(
            inbound.snapshot_hash,
            // Placeholder: the cross-device identity is `snapshot_hash`; the
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

        // 5. Resolve the content locally by `snapshot_hash` (never by the
        //    sender's per-device entry_id). A hash match counts as "held" only
        //    if the entry is *fully available* — a partial match (e.g. a
        //    cancelled transfer's `uniclip-missing://` placeholder) is treated
        //    like a miss so we pull and complete it instead of converging a
        //    placeholder into the OS clipboard.
        let local_entry_id = match self
            .entry_lookup
            .find_entry_id_by_snapshot_hash(&incoming.snapshot_hash)
            .await
        {
            Ok(Some(id)) if self.is_entry_available(&id).await => id,
            Ok(Some(_)) | Ok(None) => {
                // 6. Content missing or only partially held → pull it from the
                //    reporting peer (D3/D4/D6). On success this stores/upgrades
                //    the entry and falls through to the same convergence tail.
                //    Any pull/store failure leaves the register untouched (no
                //    advance, no re-broadcast, no retry).
                match self.pull_and_store(&peer, &incoming.snapshot_hash).await {
                    Some(id) => id,
                    None => return,
                }
            }
            Err(err) => {
                warn!(error = %err, "active state inbound dropped: entry lookup failed");
                return;
            }
        };

        self.converge_with_entry(&peer, &incoming, local_entry_id)
            .await;
    }

    /// Whether `entry_id` is fully held locally. With no availability port
    /// wired, a hash match is treated as held (prior converge-on-match
    /// behavior). An availability-query error degrades to "unavailable" so a
    /// flaky query can never converge a partial `uniclip-missing://` placeholder
    /// to the OS clipboard; the worst case is a redundant pull of content we
    /// already hold, which is strictly safer than writing a placeholder.
    async fn is_entry_available(&self, entry_id: &EntryId) -> bool {
        match &self.availability {
            Some(availability) => match availability.is_entry_available(entry_id).await {
                Ok(is_available) => is_available,
                Err(err) => {
                    warn!(
                        error = %err,
                        entry_id = %entry_id,
                        "active state inbound: availability check failed; treating entry as unavailable"
                    );
                    false
                }
            },
            None => true,
        }
    }

    /// Pull the content for `snapshot_hash` from `peer` and store it locally,
    /// returning the stored entry id. Returns `None` on any failure (pull
    /// subsystem unwired, peer unreachable / timed out, holder locked or
    /// without the content, decrypt / store failure) — the caller must then
    /// leave the register untouched (no advance, no re-broadcast, no retry,
    /// per D6).
    async fn pull_and_store(&self, peer: &DeviceId, snapshot_hash: &str) -> Option<EntryId> {
        let (Some(pull_client), Some(store)) = (
            self.pull_client.as_ref(),
            self.pulled_content_store.as_ref(),
        ) else {
            info!("active state inbound: content not held locally and pull subsystem unwired; dropping");
            return None;
        };

        // Pull the transfer envelope (the client bounds this with the 10s pull
        // deadline). Every failure mode is a logged drop.
        let envelope = match pull_client.pull(peer, snapshot_hash).await {
            Ok(bytes) => bytes,
            Err(ActiveClipboardPullClientError::Unreachable) => {
                debug!(
                    "active state inbound: pull failed (peer unreachable / timed out); dropping"
                );
                return None;
            }
            Err(ActiveClipboardPullClientError::NotAvailable) => {
                debug!("active state inbound: pull failed (peer cannot serve content); dropping");
                return None;
            }
            Err(ActiveClipboardPullClientError::Io(reason)) => {
                warn!(reason, "active state inbound: pull failed (io); dropping");
                return None;
            }
        };

        // Decrypt + persist. A store failure (decrypt / decode / capture)
        // leaves the register untouched.
        match store.store(peer, snapshot_hash, envelope).await {
            Ok(entry_id) => {
                info!(entry_id = %entry_id, "active state inbound: pulled content stored");
                Some(entry_id)
            }
            Err(err) => {
                warn!(error = %err, "active state inbound: pulled content store failed; dropping");
                None
            }
        }
    }

    /// Reconstruct the resolved entry, apply the content-type receive gate,
    /// and schedule the detached OS write whose success advances the register
    /// and re-broadcasts the same-key state (the core invariant). Shared by
    /// the "content present locally" and "content pulled" paths so both
    /// converge identically.
    async fn converge_with_entry(
        &self,
        peer: &DeviceId,
        incoming: &ActiveClipboardState,
        local_entry_id: EntryId,
    ) {
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
            .is_receive_category_allowed(peer, &categories)
            .await
        {
            return;
        }

        // Schedule the detached OS write. The register advance + the
        // re-broadcast live in the write task's success branch so they fire
        // iff the OS write succeeded (core invariant). The write is detached
        // because OS clipboard writes can block 1–3s on some platforms;
        // coupling them inline would stall the inbound loop.
        let advance_state = ActiveClipboardState::new(
            incoming.snapshot_hash.clone(),
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
        let presence = Arc::clone(&self.presence);
        let send_gate = self.send_gate.clone();
        let converged_tx = self.converged_tx.clone();

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
                    snapshot_hash = %state.snapshot_hash,
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
                        snapshot_hash = %state.snapshot_hash,
                        "active state inbound: register did not advance (lost LWW race); skipping re-broadcast"
                    );
                    return;
                }
                Err(err) => {
                    warn!(
                        error = %err,
                        snapshot_hash = %state.snapshot_hash,
                        "active state inbound: register advance failed; skipping re-broadcast"
                    );
                    return;
                }
            }

            // Notify subscribers that this entry converged (e.g. resurface
            // worker bumps active_time_ms + notifies the frontend).
            let _ = converged_tx.send(ActiveClipboardConvergedEvent {
                entry_id: state.entry_id.clone(),
            });

            // Re-broadcast the converged state to every allowed peer through
            // the shared fan-out (full outbound gate: send_enabled ∧
            // send_content_types, the latter via the activation's category
            // set). Same implementation as the restore broadcast path.
            fan_out_active_state(
                &dispatch,
                &peer_addr_repo,
                &presence,
                &send_gate,
                &state,
                &categories,
            )
            .await;
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
        ClipboardSelectionRepositoryPort, PeerAddressError, PeerAddressRecord, PresenceError,
        PresenceEvent, PresencePort, ReachabilityState, SystemClipboardPort,
    };
    use uc_core::{BlobId, MemberSyncPreferences};

    /// Presence fake that reports every device with a fixed reachability. The
    /// early-return gate tests never reach the fan-out, and the convergence
    /// tests want their re-broadcast target reachable, so `Online` is the
    /// natural default; a test can pass `Offline` to exercise the skip.
    struct StaticPresence(ReachabilityState);
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
        fn subscribe(&self) -> tokio::sync::broadcast::Receiver<PresenceEvent> {
            tokio::sync::broadcast::channel(1).1
        }
    }

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
    impl uc_core::ports::clipboard::SelfWriteLedgerPort for StubOrigin {
        async fn record_self_write(
            &self,
            _matching: uc_core::ports::clipboard::SelfWriteMatch,
            _attribution: uc_core::ports::clipboard::SelfWriteAttribution,
            _ttl: std::time::Duration,
        ) {
        }
        async fn attribute_observed_change(
            &self,
            _snapshot_hash: &str,
        ) -> uc_core::ClipboardChangeOrigin {
            uc_core::ClipboardChangeOrigin::LocalCapture
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
        let (converged_tx, _) = broadcast::channel(16);
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
            Arc::new(StaticPresence(ReachabilityState::Online)),
            Arc::new(FixedClock(now_ms)),
            converged_tx,
        );
        Harness {
            advance,
            dispatch,
            uc,
        }
    }

    fn inbound(snapshot_hash: &str, ts: i64, by: &str) -> InboundActiveClipboardState {
        InboundActiveClipboardState {
            peer_device_id: DeviceId::new("peer-p"),
            snapshot_hash: snapshot_hash.to_string(),
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

    // ========================================================================
    // Pull path (issue #1017 PR8) — content missing locally → pull from peer
    // ========================================================================

    use std::sync::Mutex;

    /// Entry lookup that always reports "content not held locally", driving the
    /// inbound flow into the pull branch.
    struct EntryLookupAlwaysMissing;
    #[async_trait]
    impl FindEntryIdBySnapshotHashPort for EntryLookupAlwaysMissing {
        async fn find_entry_id_by_snapshot_hash(
            &self,
            _hash: &str,
        ) -> Result<Option<EntryId>, ClipboardRepositoryError> {
            Ok(None)
        }
    }

    /// Pull client spy with a canned result. Records the call so a test can
    /// assert the pull was (or was not) attempted.
    struct PullClientSpy {
        result: Mutex<Option<Result<Vec<u8>, ActiveClipboardPullClientError>>>,
        calls: AtomicUsize,
    }
    impl PullClientSpy {
        fn new(result: Result<Vec<u8>, ActiveClipboardPullClientError>) -> Arc<Self> {
            Arc::new(Self {
                result: Mutex::new(Some(result)),
                calls: AtomicUsize::new(0),
            })
        }
    }
    #[async_trait]
    impl ActiveClipboardPullClientPort for PullClientSpy {
        async fn pull(
            &self,
            _peer: &DeviceId,
            _snapshot_hash: &str,
        ) -> Result<Vec<u8>, ActiveClipboardPullClientError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result
                .lock()
                .unwrap()
                .take()
                .expect("pull called more than once")
        }
    }

    /// Store spy returning a fixed entry id; records the envelope it received.
    struct StoreSpy {
        entry_id: EntryId,
        seen_envelope: Mutex<Option<Vec<u8>>>,
    }
    #[async_trait]
    impl InboundPulledContentStore for StoreSpy {
        async fn store(
            &self,
            _from_device: &DeviceId,
            _snapshot_hash: &str,
            transfer_envelope: Vec<u8>,
        ) -> Result<EntryId, InboundPulledContentStoreError> {
            *self.seen_envelope.lock().unwrap() = Some(transfer_envelope);
            Ok(self.entry_id.clone())
        }
    }

    /// Store that must never be reached (pull failed before it).
    struct StoreNeverCalled;
    #[async_trait]
    impl InboundPulledContentStore for StoreNeverCalled {
        async fn store(
            &self,
            _from_device: &DeviceId,
            _snapshot_hash: &str,
            _transfer_envelope: Vec<u8>,
        ) -> Result<EntryId, InboundPulledContentStoreError> {
            panic!("store reached after a pull failure");
        }
    }

    /// System clipboard whose `write_snapshot` succeeds — used by the pull
    /// happy path so the convergence tail can advance + re-broadcast.
    struct OkWriteClipboard;
    impl SystemClipboardPort for OkWriteClipboard {
        fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
            unreachable!("read_snapshot must not be called")
        }
        fn write_snapshot(&self, _snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// Reconstruct ports backing a single inline text entry, so the pull happy
    /// path can reconstruct the stored content and converge.
    struct TextEntry {
        entry_id: EntryId,
        event_id: EventId,
        rep_id: RepresentationId,
        bytes: Vec<u8>,
    }
    #[async_trait]
    impl GetClipboardEntryPort for TextEntry {
        async fn get_entry(
            &self,
            _entry_id: &EntryId,
        ) -> Result<Option<ClipboardEntry>, ClipboardRepositoryError> {
            Ok(Some(ClipboardEntry::new(
                self.entry_id.clone(),
                self.event_id.clone(),
                0,
                None,
                0,
            )))
        }
    }
    #[async_trait]
    impl ClipboardSelectionRepositoryPort for TextEntry {
        async fn get_selection(
            &self,
            _entry_id: &EntryId,
        ) -> anyhow::Result<Option<ClipboardSelectionDecision>> {
            use uc_core::clipboard::{ClipboardSelection, SelectionPolicyVersion};
            Ok(Some(ClipboardSelectionDecision::new(
                self.entry_id.clone(),
                ClipboardSelection {
                    primary_rep_id: self.rep_id.clone(),
                    secondary_rep_ids: Vec::new(),
                    preview_rep_id: self.rep_id.clone(),
                    paste_rep_id: self.rep_id.clone(),
                    policy_version: SelectionPolicyVersion::V1,
                },
            )))
        }
        async fn delete_selection(&self, _entry_id: &EntryId) -> anyhow::Result<()> {
            unreachable!()
        }
    }
    #[async_trait]
    impl GetRepresentationPort for TextEntry {
        async fn get_representation(
            &self,
            _event_id: &EventId,
            representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>, ClipboardRepositoryError> {
            use uc_core::clipboard::MimeType;
            use uc_core::ids::FormatId;
            if *representation_id != self.rep_id {
                return Ok(None);
            }
            Ok(Some(PersistedClipboardRepresentation::new(
                self.rep_id.clone(),
                FormatId::from("public.utf8-plain-text"),
                Some(MimeType("text/plain".to_string())),
                self.bytes.len() as i64,
                Some(self.bytes.clone()),
                None,
            )))
        }
    }
    #[async_trait]
    impl UpdateRepresentationProcessingResultPort for TextEntry {
        async fn update_processing_result(
            &self,
            _rep_id: &RepresentationId,
            _expected_states: &[PayloadAvailability],
            _blob_id: Option<&BlobId>,
            _new_state: PayloadAvailability,
            _last_error: Option<&str>,
        ) -> Result<ProcessingUpdateOutcome, ClipboardRepositoryError> {
            Ok(ProcessingUpdateOutcome::StateMismatch)
        }
    }
    #[async_trait]
    impl ClipboardPayloadResolverPort for TextEntry {
        async fn resolve(
            &self,
            rep: &PersistedClipboardRepresentation,
        ) -> Result<ResolvedClipboardPayload, PayloadResolveError> {
            Ok(ResolvedClipboardPayload::Inline {
                mime: rep
                    .mime_type
                    .as_ref()
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
                bytes: self.bytes.clone(),
            })
        }
    }
    #[async_trait]
    impl BlobReaderPort for TextEntry {
        async fn get(&self, _blob_id: &BlobId) -> anyhow::Result<Vec<u8>> {
            unreachable!()
        }
    }

    /// Peer-address repo with a single reachable peer, so the convergence
    /// tail's fan-out has a target to re-broadcast to.
    struct OnePeerAddrRepo(String);
    #[async_trait]
    impl PeerAddressRepositoryPort for OnePeerAddrRepo {
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
            Ok(vec![PeerAddressRecord {
                device_id: DeviceId::new(&self.0),
                addr_blob: vec![0xAA; 8],
                observed_at: Utc::now(),
            }])
        }
        async fn remove(&self, _device: &DeviceId) -> Result<(), PeerAddressError> {
            Ok(())
        }
    }

    /// Build a UC whose entry lookup always misses (driving the pull branch),
    /// wired with the given pull client + store. The reconstruct + coordinator
    /// are real enough that a successful store can converge. `peer_addr_repo`
    /// lets a caller supply a re-broadcast target.
    #[allow(clippy::too_many_arguments)]
    fn pull_harness_with_peers(
        pull_client: Option<Arc<dyn ActiveClipboardPullClientPort>>,
        store: Option<Arc<dyn InboundPulledContentStore>>,
        stored_entry_id: EntryId,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    ) -> Harness {
        let advance = Arc::new(AdvanceSpy::default());
        let dispatch = Arc::new(DispatchSpy::default());
        let text = Arc::new(TextEntry {
            entry_id: stored_entry_id.clone(),
            event_id: EventId::from("evt-pull"),
            rep_id: RepresentationId::from("rep-pull"),
            bytes: b"pulled text".to_vec(),
        });
        let reconstructor = SnapshotReconstructor::new(
            text.clone(),
            text.clone(),
            text.clone(),
            text.clone(),
            text.clone(),
            text.clone(),
        );
        let coordinator = Arc::new(ClipboardWriteCoordinator::new(
            Arc::new(OkWriteClipboard),
            Arc::new(StubOrigin),
        ));
        let (converged_tx, _) = broadcast::channel(16);
        let mut uc = ApplyInboundActiveClipboardStateUseCase::new(
            Arc::new(NoopReceiver),
            Arc::new(FixedUnlocked(true)),
            Arc::new(FixedRegister(None)),
            Arc::clone(&advance) as Arc<dyn AdvanceActiveClipboardPort>,
            Arc::new(MemberRepoStub {
                receive_enabled: true,
            }),
            Arc::new(EntryLookupAlwaysMissing),
            reconstructor,
            coordinator,
            Arc::clone(&dispatch) as Arc<dyn ActiveClipboardDispatchPort>,
            peer_addr_repo,
            Arc::new(StaticPresence(ReachabilityState::Online)),
            Arc::new(FixedClock(1_000)),
            converged_tx,
        );
        if let (Some(pull_client), Some(store)) = (pull_client, store) {
            uc = uc.with_pull(pull_client, store);
        }
        Harness {
            advance,
            dispatch,
            uc,
        }
    }

    /// Convenience wrapper: no re-broadcast target (empty peer roster).
    fn pull_harness(
        pull_client: Option<Arc<dyn ActiveClipboardPullClientPort>>,
        store: Option<Arc<dyn InboundPulledContentStore>>,
        stored_entry_id: EntryId,
    ) -> Harness {
        pull_harness_with_peers(
            pull_client,
            store,
            stored_entry_id,
            Arc::new(EmptyPeerAddrRepo),
        )
    }

    /// Poll until `advance` is observed (the convergence tail runs detached).
    async fn wait_for_advance(advance: &AdvanceSpy) -> bool {
        for _ in 0..200 {
            if advance.calls.load(Ordering::SeqCst) > 0 {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        false
    }

    /// Pull subsystem unwired → the "content missing" branch logs + returns;
    /// nothing converges.
    #[tokio::test]
    async fn missing_content_without_pull_subsystem_is_inert() {
        let h = pull_harness(None, None, EntryId::new());
        h.uc.handle_one(inbound("blake3v1:aa", 1_000, "dev-x"))
            .await;
        // No async convergence is scheduled on this path, so a direct inert
        // assertion holds.
        assert_inert(&h);
    }

    /// Pull fails (peer unreachable / timed out) → no store, no advance, no
    /// re-broadcast, no retry.
    #[tokio::test]
    async fn pull_failure_does_not_advance_or_broadcast() {
        let pull_client = PullClientSpy::new(Err(ActiveClipboardPullClientError::Unreachable));
        let h = pull_harness(
            Some(Arc::clone(&pull_client) as _),
            Some(Arc::new(StoreNeverCalled)),
            EntryId::new(),
        );
        h.uc.handle_one(inbound("blake3v1:aa", 1_000, "dev-x"))
            .await;
        // Give any (erroneously) spawned convergence task a chance to run.
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(
            pull_client.calls.load(Ordering::SeqCst),
            1,
            "pull must be attempted exactly once (no retry)"
        );
        assert_inert(&h);
    }

    /// Pull succeeds → the envelope is stored, then the convergence tail writes
    /// OS, advances the register, and re-broadcasts the same-key state to a
    /// send-allowed peer.
    #[tokio::test]
    async fn pull_success_stores_then_advances_and_rebroadcasts() {
        let stored_entry_id = EntryId::from("entry-pulled");
        let pull_client = PullClientSpy::new(Ok(b"transfer-envelope".to_vec()));
        let store = Arc::new(StoreSpy {
            entry_id: stored_entry_id.clone(),
            seen_envelope: Mutex::new(None),
        });
        // A reachable peer distinct from the activator → fan-out re-broadcasts.
        let h = pull_harness_with_peers(
            Some(Arc::clone(&pull_client) as _),
            Some(Arc::clone(&store) as _),
            stored_entry_id,
            Arc::new(OnePeerAddrRepo("peer-rebroadcast".to_string())),
        );
        h.uc.handle_one(inbound("blake3v1:aa", 1_000, "dev-x"))
            .await;

        assert!(
            wait_for_advance(&h.advance).await,
            "register must advance after a successful pull + store + OS write"
        );
        assert_eq!(
            pull_client.calls.load(Ordering::SeqCst),
            1,
            "pull attempted once"
        );
        assert_eq!(
            store.seen_envelope.lock().unwrap().as_deref(),
            Some(b"transfer-envelope".as_slice()),
            "store must receive the pulled transfer envelope"
        );
        // The same-key re-broadcast fires through the shared fan-out to the
        // (send-allowed) peer — the core invariant's third clause.
        assert_eq!(
            h.dispatch.calls.load(Ordering::SeqCst),
            1,
            "converged state must be re-broadcast to the allowed peer"
        );
    }
}
