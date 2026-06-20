//! `PeerOnlineResyncWorker` — debounced, gated resend of this device's current
//! active-clipboard register to peers that just came online (issue #1017 PR5,
//! D10).
//!
//! When a peer `Q` becomes reachable it may hold a stale (or empty) view of
//! the active clipboard — it was offline while the register last advanced.
//! This worker reacts to "peer online" presence transitions and sends `Q` our
//! current register so the two ends converge under LWW. The resync is
//! **symmetric**: `Q` runs the same worker and sends us *its* register; the
//! LWW order picks the winner on both sides. There is no ack or handshake —
//! convergence is the register's job, not this send's.
//!
//! Behaviour:
//!
//! 1. **Coalesces** a burst of online transitions: it accumulates the set of
//!    devices that came online and, after a quiet window (D7, ~1.5s), resyncs
//!    each one once. A flapping peer or many peers reconnecting at once
//!    produce one resync per device, not one per transition.
//! 2. **Loads** the current register at emit time. `None` (never written) →
//!    nothing to send.
//! 3. **Resolves** the activation's content category by reconstructing the
//!    locally-held snapshot, so the outbound content-type gate can be applied
//!    (see [`Self::emit`] for why this is reconstructed rather than carried).
//! 4. **Gates** each target through the full outbound gate (D2): `send_enabled`
//!    ∧ `send_content_types`, identical to the restore-broadcast and inbound
//!    re-broadcast paths (shared single-target send).
//!
//! ## Convergence scope (D6)
//!
//! Presence transitions only fire for **directly-connected** peers (presence
//! is driven by the local endpoint's dial/connection state). A peer reachable
//! only through a relay chain is not observed here, so its resync is
//! best-effort and not guaranteed — consistent with the no-retry posture of
//! the rest of the feature.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tracing::{debug, info, instrument, warn};

use uc_core::clipboard::ClipboardContentCategorySet;
use uc_core::ids::DeviceId;
use uc_core::ports::clipboard::{ActiveClipboardDispatchPort, LoadActiveClipboardPort};
use uc_core::ports::presence::{PresenceEvent, ReachabilityState};
use uc_core::ports::PresencePort;
use uc_core::MemberRepositoryPort;

use super::super::send_gate::MemberSendGate;
use super::super::snapshot_from_entry::SnapshotReconstructor;
use super::fanout::send_active_state_to;

/// Debounce window for coalescing a burst of peer-online transitions into one
/// resync per device (D7). Distinct from the restore broadcast's 300ms window:
/// peer-online events are noisier (flapping, batch reconnects), so the window
/// is wider.
const PEER_ONLINE_RESYNC_DEBOUNCE: Duration = Duration::from_millis(1_500);

/// Handle owning the spawned peer-online resync worker. Drop or `abort()` to
/// stop it; the worker also exits on its own when the presence subscription's
/// senders drop (router shutdown).
pub struct PeerOnlineResyncHandle {
    join: JoinHandle<()>,
}

impl PeerOnlineResyncHandle {
    pub fn abort(&self) {
        self.join.abort();
    }
}

impl Drop for PeerOnlineResyncHandle {
    fn drop(&mut self) {
        self.join.abort();
    }
}

/// Dependencies for the peer-online resync worker.
pub(crate) struct PeerOnlineResyncWorker {
    presence: Arc<dyn PresencePort>,
    load_register: Arc<dyn LoadActiveClipboardPort>,
    reconstructor: SnapshotReconstructor,
    dispatch: Arc<dyn ActiveClipboardDispatchPort>,
    send_gate: MemberSendGate,
}

impl PeerOnlineResyncWorker {
    pub(crate) fn new(
        presence: Arc<dyn PresencePort>,
        load_register: Arc<dyn LoadActiveClipboardPort>,
        reconstructor: SnapshotReconstructor,
        dispatch: Arc<dyn ActiveClipboardDispatchPort>,
        member_repo: Arc<dyn MemberRepositoryPort>,
    ) -> Self {
        Self {
            presence,
            load_register,
            reconstructor,
            dispatch,
            send_gate: MemberSendGate::new(member_repo),
        }
    }

    /// Spawn the worker loop.
    pub(crate) fn spawn(self) -> PeerOnlineResyncHandle {
        let join = tokio::spawn(self.run());
        PeerOnlineResyncHandle { join }
    }

    #[instrument(name = "active_state.peer_online_resync_loop", skip_all)]
    async fn run(self) {
        let mut rx = self.presence.subscribe();
        loop {
            // Block until the first online transition (or all senders drop →
            // exit). Non-online transitions (offline / unknown) are not a
            // resync trigger and are ignored.
            let first = match Self::recv_next_online(&mut rx).await {
                Some(device) => device,
                None => {
                    info!("peer-online resync worker: presence subscription closed; exiting");
                    return;
                }
            };

            // Coalesce: accumulate the set of devices that came online during
            // the quiet window. A fresh online transition restarts the window,
            // so the timer measures quiet time, not a fixed batch interval.
            let mut pending: HashSet<DeviceId> = HashSet::new();
            pending.insert(first);
            loop {
                tokio::select! {
                    biased;
                    maybe = Self::recv_next_online(&mut rx) => match maybe {
                        Some(device) => {
                            pending.insert(device);
                            // loop again — window restarts
                        }
                        None => {
                            // Senders gone mid-window: resync what we have,
                            // then the outer loop's recv sees the close and
                            // exits.
                            break;
                        }
                    },
                    _ = sleep(PEER_ONLINE_RESYNC_DEBOUNCE) => break,
                }
            }

            self.emit(pending).await;
        }
    }

    /// Pull the next presence event that is an *online* transition, skipping
    /// offline / unknown transitions and lag gaps. Returns `None` when the
    /// subscription is closed.
    async fn recv_next_online(rx: &mut broadcast::Receiver<PresenceEvent>) -> Option<DeviceId> {
        loop {
            match rx.recv().await {
                Ok(event) if event.state == ReachabilityState::Online => {
                    return Some(event.device_id);
                }
                // Offline / Unknown transitions are not a resync trigger.
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    // A missed online transition self-heals: presence
                    // re-emits, or the peer's own resync reaches us.
                    warn!(missed, "peer-online resync: presence receiver lagged");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }

    /// Resync the current register to every coalesced online peer.
    ///
    /// The register stores only `{snapshot_hash, entry_id, …}` with no content
    /// category, but the outbound content-type gate (D2) needs one. We
    /// reconstruct the locally-held snapshot to derive it: the register only
    /// ever advanced because the content was written to *this* device's OS
    /// clipboard, so the entry is held locally and reconstructable. Applying
    /// the full gate keeps every outbound 0xC3 path symmetric — restore
    /// broadcast and inbound re-broadcast both gate on `send_content_types`,
    /// so peer-online resync does too rather than leaking a state pointer for
    /// a muted content type.
    async fn emit(&self, targets: HashSet<DeviceId>) {
        let state = match self.load_register.load().await {
            Ok(Some(state)) => state,
            Ok(None) => {
                debug!("peer-online resync: register empty; nothing to resend");
                return;
            }
            Err(err) => {
                warn!(error = %err, "peer-online resync skipped: register load failed");
                return;
            }
        };

        // Resolve the activation's content category for the outbound gate.
        // A reconstruction failure (payload lost / locked / blob unavailable)
        // means we cannot establish what the content is, so we cannot gate it
        // safely — skip the whole resync rather than send ungated.
        let categories = match self.reconstructor.reconstruct(&state.entry_id).await {
            Ok(snapshot) => ClipboardContentCategorySet::from_snapshot(&snapshot),
            Err(err) => {
                warn!(
                    error = %err,
                    entry_id = %state.entry_id,
                    "peer-online resync skipped: snapshot reconstruct failed"
                );
                return;
            }
        };

        for target in targets {
            // Never resend the state to the device that activated it: it is
            // already the source of truth for this activation.
            if target == state.activated_by {
                continue;
            }
            send_active_state_to(
                &self.dispatch,
                &self.send_gate,
                &target,
                &state,
                &categories,
            )
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    use async_trait::async_trait;
    use chrono::Utc;

    use uc_core::blob::ports::BlobReaderPort;
    use uc_core::clipboard::{
        ActiveClipboardState, ClipboardEntry, ClipboardRepositoryError, ClipboardSelection,
        ClipboardSelectionDecision, MimeType, PayloadAvailability,
        PersistedClipboardRepresentation, SelectionPolicyVersion,
    };
    use uc_core::ids::{DeviceId, EntryId, EventId, FormatId, RepresentationId};
    use uc_core::membership::{MembershipError, SpaceMember};
    use uc_core::ports::clipboard::{
        ActiveClipboardDispatchError, ActiveClipboardRegisterError, ClipboardPayloadResolverPort,
        GetClipboardEntryPort, GetRepresentationPort, PayloadResolveError, ProcessingUpdateOutcome,
        ResolvedClipboardPayload, UpdateRepresentationProcessingResultPort,
    };
    use uc_core::ports::ClipboardSelectionRepositoryPort;
    use uc_core::{BlobId, MemberSyncPreferences};

    // ---- spies / fakes ------------------------------------------------------

    /// Presence port whose `subscribe()` hands out receivers attached to a
    /// caller-controlled broadcast sender.
    struct FakePresence {
        tx: broadcast::Sender<PresenceEvent>,
    }
    impl FakePresence {
        fn new() -> (Arc<Self>, broadcast::Sender<PresenceEvent>) {
            let (tx, _) = broadcast::channel(16);
            (Arc::new(Self { tx: tx.clone() }), tx)
        }
    }
    #[async_trait]
    impl PresencePort for FakePresence {
        async fn ensure_reachable(
            &self,
            _device: &DeviceId,
        ) -> Result<ReachabilityState, uc_core::ports::presence::PresenceError> {
            Ok(ReachabilityState::Online)
        }
        async fn current_state(&self, _device: &DeviceId) -> ReachabilityState {
            ReachabilityState::Unknown
        }
        fn subscribe(&self) -> broadcast::Receiver<PresenceEvent> {
            self.tx.subscribe()
        }
    }

    fn online(device: &str) -> PresenceEvent {
        PresenceEvent {
            device_id: DeviceId::new(device),
            state: ReachabilityState::Online,
            at: Utc::now(),
        }
    }
    fn offline(device: &str) -> PresenceEvent {
        PresenceEvent {
            device_id: DeviceId::new(device),
            state: ReachabilityState::Offline,
            at: Utc::now(),
        }
    }

    struct FixedRegister(Option<ActiveClipboardState>);
    #[async_trait]
    impl LoadActiveClipboardPort for FixedRegister {
        async fn load(&self) -> Result<Option<ActiveClipboardState>, ActiveClipboardRegisterError> {
            Ok(self.0.clone())
        }
    }

    /// Records the (target, snapshot_hash) pairs dispatched, in order.
    #[derive(Default)]
    struct DispatchSpy {
        sent: Mutex<Vec<(String, String)>>,
    }
    #[async_trait]
    impl ActiveClipboardDispatchPort for DispatchSpy {
        async fn dispatch(
            &self,
            target: &DeviceId,
            state: &ActiveClipboardState,
        ) -> Result<(), ActiveClipboardDispatchError> {
            self.sent
                .lock()
                .unwrap()
                .push((target.as_str().to_string(), state.snapshot_hash.clone()));
            Ok(())
        }
    }

    struct AllowAllMembers;
    #[async_trait]
    impl MemberRepositoryPort for AllowAllMembers {
        async fn get(&self, device_id: &DeviceId) -> Result<Option<SpaceMember>, MembershipError> {
            Ok(Some(SpaceMember {
                device_id: device_id.clone(),
                device_name: "peer".to_string(),
                identity_fingerprint: uc_core::security::IdentityFingerprint::from_raw_string(
                    "0123456789abcdef",
                )
                .expect("valid test fingerprint"),
                joined_at: Utc::now(),
                sync_preferences: MemberSyncPreferences::default(),
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

    // -- reconstructor fakes: resolve a single text rep so `from_snapshot`
    //    yields a known category set (Text). --

    struct OneEntryRepo;
    #[async_trait]
    impl GetClipboardEntryPort for OneEntryRepo {
        async fn get_entry(
            &self,
            entry_id: &EntryId,
        ) -> Result<Option<ClipboardEntry>, ClipboardRepositoryError> {
            Ok(Some(ClipboardEntry::new(
                entry_id.clone(),
                EventId::from("evt-1"),
                0,
                None,
                0,
            )))
        }
    }

    struct OneSelectionRepo;
    #[async_trait]
    impl ClipboardSelectionRepositoryPort for OneSelectionRepo {
        async fn get_selection(
            &self,
            entry_id: &EntryId,
        ) -> anyhow::Result<Option<ClipboardSelectionDecision>> {
            let rep = RepresentationId::from("rep-text");
            Ok(Some(ClipboardSelectionDecision::new(
                entry_id.clone(),
                ClipboardSelection {
                    primary_rep_id: rep.clone(),
                    secondary_rep_ids: Vec::new(),
                    preview_rep_id: rep.clone(),
                    paste_rep_id: rep,
                    policy_version: SelectionPolicyVersion::V1,
                },
            )))
        }
        async fn delete_selection(&self, _entry_id: &EntryId) -> anyhow::Result<()> {
            unreachable!()
        }
    }

    struct OneRepRepo;
    #[async_trait]
    impl GetRepresentationPort for OneRepRepo {
        async fn get_representation(
            &self,
            _event_id: &EventId,
            representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>, ClipboardRepositoryError> {
            Ok(Some(PersistedClipboardRepresentation::new(
                representation_id.clone(),
                FormatId::from("public.utf8-plain-text"),
                Some(MimeType("text/plain".to_string())),
                5,
                Some(b"hello".to_vec()),
                None,
            )))
        }
    }

    struct StubProcessingRepo;
    #[async_trait]
    impl UpdateRepresentationProcessingResultPort for StubProcessingRepo {
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

    struct InlineResolver;
    #[async_trait]
    impl ClipboardPayloadResolverPort for InlineResolver {
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
                bytes: b"hello".to_vec(),
            })
        }
    }

    struct UnusedBlobStore;
    #[async_trait]
    impl BlobReaderPort for UnusedBlobStore {
        async fn get(&self, _blob_id: &BlobId) -> anyhow::Result<Vec<u8>> {
            unreachable!()
        }
    }

    fn reconstructor() -> SnapshotReconstructor {
        SnapshotReconstructor::new(
            Arc::new(OneEntryRepo),
            Arc::new(OneSelectionRepo),
            Arc::new(OneRepRepo),
            Arc::new(StubProcessingRepo),
            Arc::new(InlineResolver),
            Arc::new(UnusedBlobStore),
        )
    }

    fn state(snapshot_hash: &str, by: &str) -> ActiveClipboardState {
        ActiveClipboardState::new(
            snapshot_hash,
            EntryId::from("entry-local"),
            1_000,
            DeviceId::new(by),
        )
    }

    #[allow(clippy::type_complexity)]
    fn build(
        register: Option<ActiveClipboardState>,
    ) -> (
        PeerOnlineResyncWorker,
        broadcast::Sender<PresenceEvent>,
        Arc<DispatchSpy>,
    ) {
        let (presence, presence_tx) = FakePresence::new();
        let dispatch = Arc::new(DispatchSpy::default());
        let worker = PeerOnlineResyncWorker::new(
            presence,
            Arc::new(FixedRegister(register)),
            reconstructor(),
            Arc::clone(&dispatch) as Arc<dyn ActiveClipboardDispatchPort>,
            Arc::new(AllowAllMembers),
        );
        (worker, presence_tx, dispatch)
    }

    // A debounce window plus a slice of slack — keep tests above the 1.5s
    // window so the coalesce timer fires before we sample.
    fn past_window() -> Duration {
        PEER_ONLINE_RESYNC_DEBOUNCE + Duration::from_millis(200)
    }

    #[tokio::test]
    async fn empty_register_sends_nothing() {
        let (worker, tx, dispatch) = build(None);
        let handle = worker.spawn();
        tokio::task::yield_now().await;

        tx.send(online("peer-1")).unwrap();
        tokio::time::sleep(past_window()).await;

        assert!(
            dispatch.sent.lock().unwrap().is_empty(),
            "no register → nothing to resend"
        );
        handle.abort();
    }

    #[tokio::test]
    async fn online_peer_receives_current_register() {
        let (worker, tx, dispatch) = build(Some(state("blake3v1:aa", "self")));
        let handle = worker.spawn();
        tokio::task::yield_now().await;

        tx.send(online("peer-1")).unwrap();
        tokio::time::sleep(past_window()).await;

        let sent = dispatch.sent.lock().unwrap();
        assert_eq!(sent.len(), 1, "the online peer gets exactly one resync");
        assert_eq!(sent[0], ("peer-1".to_string(), "blake3v1:aa".to_string()));
        drop(sent);
        handle.abort();
    }

    #[tokio::test]
    async fn burst_of_online_events_coalesces_to_one_per_device() {
        let (worker, tx, dispatch) = build(Some(state("blake3v1:aa", "self")));
        let handle = worker.spawn();
        tokio::task::yield_now().await;

        // Two distinct peers, with the same peer flapping twice inside the
        // window — expect one resync per distinct device (2 total), not four.
        tx.send(online("peer-1")).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        tx.send(online("peer-2")).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        tx.send(online("peer-1")).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        tx.send(online("peer-2")).unwrap();

        tokio::time::sleep(past_window()).await;

        let mut sent: Vec<String> = dispatch
            .sent
            .lock()
            .unwrap()
            .iter()
            .map(|(target, _)| target.clone())
            .collect();
        sent.sort();
        assert_eq!(
            sent,
            vec!["peer-1".to_string(), "peer-2".to_string()],
            "burst coalesces to one resync per distinct online device"
        );
        handle.abort();
    }

    #[tokio::test]
    async fn offline_transitions_do_not_trigger_resync() {
        let (worker, tx, dispatch) = build(Some(state("blake3v1:aa", "self")));
        let handle = worker.spawn();
        tokio::task::yield_now().await;

        tx.send(offline("peer-1")).unwrap();
        tokio::time::sleep(past_window()).await;

        assert!(
            dispatch.sent.lock().unwrap().is_empty(),
            "an offline transition is not a resync trigger"
        );
        handle.abort();
    }

    #[tokio::test]
    async fn activator_is_not_resynced_to_itself() {
        // The register was activated by "peer-1"; when peer-1 comes online we
        // must not echo its own activation back.
        let (worker, tx, dispatch) = build(Some(state("blake3v1:aa", "peer-1")));
        let handle = worker.spawn();
        tokio::task::yield_now().await;

        tx.send(online("peer-1")).unwrap();
        tokio::time::sleep(past_window()).await;

        assert!(
            dispatch.sent.lock().unwrap().is_empty(),
            "the activating device is never resynced its own state"
        );
        handle.abort();
    }
}
