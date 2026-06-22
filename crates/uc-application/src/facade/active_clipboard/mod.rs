//! `ActiveClipboardFacade` — application entry point for the cross-device
//! active-clipboard register convergence (issue #1017).
//!
//! Owns the inbound state use case and exposes outbound origination actions:
//! spawn the background loop that subscribes to inbound 0xC3 observations and
//! drives the register toward convergence (write OS → advance register →
//! re-broadcast), plus the outbound origination workers (restore broadcast,
//! peer-online resync) and the mobile-push activation announce
//! ([`ActiveClipboardFacade::announce_local_activation`]).

mod reconcile;

pub use reconcile::{
    ActiveClipboardReconcileDeps, ActiveClipboardReconcileFacade, ActiveClipboardReconcileOutcome,
};

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{broadcast, mpsc::UnboundedReceiver};
use tokio::task::JoinHandle;
use tracing::{debug, instrument, warn};

use uc_core::clipboard::ClipboardContentCategorySet;
use uc_core::ids::{DeviceId, EntryId};
use uc_core::ports::clipboard::{
    ActiveClipboardDispatchPort, ActiveClipboardPullClientPort, ActiveClipboardPullServePort,
    ActiveClipboardReceiverPort, AdvanceActiveClipboardPort, ClipboardPayloadResolverPort,
    ClipboardSelectionRepositoryPort, FindEntryIdBySnapshotHashPort, GetClipboardEntryPort,
    GetRepresentationPort, LoadActiveClipboardPort, TouchClipboardEntryPort,
    UpdateRepresentationProcessingResultPort,
};
use uc_core::ports::security::TransferCipherPort;
use uc_core::ports::space::IsSpaceUnlockedPort;
use uc_core::ports::{
    ClockPort, DeviceIdentityPort, PeerAddressRepositoryPort, PresencePort, SettingsPort,
};
use uc_core::{blob::ports::BlobReaderPort, MemberRepositoryPort};

use crate::clipboard_write::{
    ClipboardWriteCoordinator, LocalActiveRegisterAdvancer, RestoreBroadcastRequest,
};
use crate::facade::blob_transfer::{BlobTransferFacade, SharedHostEventEmitter};
use crate::facade::clipboard_inbound::{
    InboundClipboardApplyInput, InboundClipboardApplyOutcome, InboundClipboardApplyPort,
};
use crate::facade::clipboard_outbound::OutboundBlobPublishGateway;
use crate::facade::host_event::{ClipboardHostEvent, ClipboardOriginKind, HostEvent};
use crate::usecases::clipboard_sync::active_state::apply_inbound::{
    ActiveClipboardConvergedEvent, ActiveClipboardInboundHandle,
    ApplyInboundActiveClipboardStateUseCase, InboundPulledContentStore,
    InboundPulledContentStoreError,
};
use crate::usecases::clipboard_sync::active_state::fanout::fan_out_active_state;
use crate::usecases::clipboard_sync::active_state::peer_online_resync_worker::{
    PeerOnlineResyncHandle, PeerOnlineResyncWorker,
};
use crate::usecases::clipboard_sync::active_state::restore_broadcast_worker::{
    RestoreBroadcastHandle, RestoreBroadcastWorker,
};
use crate::usecases::clipboard_sync::active_state::serve_pull::{
    ActiveClipboardPullServeDeps, ActiveClipboardPullServeUseCase,
};
use crate::usecases::clipboard_sync::send_gate::MemberSendGate;
use crate::usecases::clipboard_sync::snapshot_from_entry::SnapshotReconstructor;

/// The six repository / resolver ports needed to rebuild a
/// `SystemClipboardSnapshot` from a local entry id. Bundled so callers wire one
/// dependency instead of threading six identical ports; folded into a
/// `SnapshotReconstructor` at facade construction. Shared by the
/// inbound / resend / restore paths ([`ActiveClipboardDeps`]) and the pull
/// serve path ([`ActiveClipboardPullServeFacadeDeps`]).
pub struct ClipboardSnapshotDeps {
    pub entry_repo: Arc<dyn GetClipboardEntryPort>,
    pub selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    pub representation_repo: Arc<dyn GetRepresentationPort>,
    pub rep_processing_repo: Arc<dyn UpdateRepresentationProcessingResultPort>,
    pub payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    pub blob_store: Arc<dyn BlobReaderPort>,
}

impl ClipboardSnapshotDeps {
    /// Fold the bundled ports into the shared `SnapshotReconstructor`. The free
    /// function `reconstruct_snapshot_from_entry` stays the single source of
    /// truth; this just owns the ports.
    fn into_reconstructor(self) -> SnapshotReconstructor {
        SnapshotReconstructor::new(
            self.entry_repo,
            self.selection_repo,
            self.representation_repo,
            self.rep_processing_repo,
            self.payload_resolver,
            self.blob_store,
        )
    }
}

/// Wiring dependencies for [`ActiveClipboardFacade`]. Assembled by bootstrap.
pub struct ActiveClipboardDeps {
    pub receiver: Arc<dyn ActiveClipboardReceiverPort>,
    pub dispatch: Arc<dyn ActiveClipboardDispatchPort>,
    pub is_unlocked: Arc<dyn IsSpaceUnlockedPort>,
    pub load_register: Arc<dyn LoadActiveClipboardPort>,
    pub advance_register: Arc<dyn AdvanceActiveClipboardPort>,
    pub member_repo: Arc<dyn MemberRepositoryPort>,
    pub peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    /// Presence stream for the peer-online resync worker: an "online"
    /// transition triggers a resend of the current register to that peer.
    pub presence: Arc<dyn PresencePort>,
    pub entry_lookup: Arc<dyn FindEntryIdBySnapshotHashPort>,
    pub coordinator: Arc<ClipboardWriteCoordinator>,
    pub clock: Arc<dyn ClockPort>,
    /// Identity of this device, used to stamp a locally-originated activation
    /// (`activated_by = self`) when announcing a fresh active-clipboard state.
    pub device_identity: Arc<dyn DeviceIdentityPort>,
    /// Settings reader for the restore-broadcast feature gate
    /// (`sync.sync_on_restore`).
    pub settings: Arc<dyn SettingsPort>,
    /// Snapshot reconstruction ports (shared with restore / resend), folded
    /// into a `SnapshotReconstructor` at construction.
    pub snapshot: ClipboardSnapshotDeps,
    // ---- On-demand pull subsystem (issue #1017 PR8) ----
    /// Transfer cipher shared with the bulk sync path. The inbound store side
    /// decrypts a pulled envelope before persisting it.
    pub transfer_cipher: Arc<dyn TransferCipherPort>,
    /// Outbound pull client. `None` when the pull subsystem is unwired (e.g.
    /// the GUI/CLI client paths) — the inbound "content missing" branch then
    /// logs and returns. Paired with `pull_apply`.
    pub pull_client: Option<Arc<dyn ActiveClipboardPullClientPort>>,
    /// Store-only inbound apply path used to persist a pulled envelope. Must
    /// **not** advance the active-clipboard register itself — the inbound
    /// convergence tail owns the register advance (coupled to OS-write
    /// success). Paired with `pull_client`.
    pub pull_apply: Option<Arc<dyn InboundClipboardApplyPort>>,
    /// Resurfaces the converged entry in clipboard history.
    pub touch_entry: Arc<dyn TouchClipboardEntryPort>,
    /// Host event bus for notifying the frontend after a resurface.
    pub host_event_emitter: SharedHostEventEmitter,
    /// Wall clock for stamping the resurface time.
    pub resurface_clock: Arc<dyn ClockPort>,
}

/// Dependencies for the standalone pull serve port
/// ([`build_active_clipboard_pull_serve_port`]). Built separately from the
/// facade because the serve port must be registered on the pull accept handler
/// before the node spawns, whereas the facade (which owns the inbound loop) is
/// assembled after.
pub struct ActiveClipboardPullServeFacadeDeps {
    pub entry_lookup: Arc<dyn FindEntryIdBySnapshotHashPort>,
    pub settings: Arc<dyn SettingsPort>,
    pub transfer_cipher: Arc<dyn TransferCipherPort>,
    /// Blob transfer facade. The serve side publishes large/image reps and
    /// free-standing files into this device's blob store through it, re-issuing
    /// tickets pinned to this device (D3) before encoding the V3 envelope.
    pub blob_publisher: Arc<BlobTransferFacade>,
    /// Snapshot reconstruction ports (shared with restore / resend), folded
    /// into a `SnapshotReconstructor` at construction.
    pub snapshot: ClipboardSnapshotDeps,
}

/// Build the active-clipboard pull serve port (issue #1017 PR8). Reuses the
/// resend crypto chain (reconstruct → publish blobs (re-issues self-pinned
/// tickets, D3) → encode V3 → encrypt with a fresh transfer identity, D4).
///
/// Standalone (not a facade method) so bootstrap can register it on the pull
/// accept handler before the node spawns.
pub fn build_active_clipboard_pull_serve_port(
    deps: ActiveClipboardPullServeFacadeDeps,
) -> Arc<dyn ActiveClipboardPullServePort> {
    let reconstructor = deps.snapshot.into_reconstructor();
    let blob_publisher: Arc<dyn OutboundBlobPublishGateway> = deps.blob_publisher;
    Arc::new(ActiveClipboardPullServeUseCase::new(
        ActiveClipboardPullServeDeps {
            entry_lookup: deps.entry_lookup,
            reconstructor,
            settings: deps.settings,
            blob_publisher,
            cipher: deps.transfer_cipher,
        },
    ))
}

/// Re-exported handle so bootstrap can hold the spawned loop's lifetime.
pub use crate::usecases::clipboard_sync::active_state::apply_inbound::ActiveClipboardInboundHandle as ActiveClipboardHandle;

/// Thin facade over the inbound active-clipboard state use case plus the
/// outbound origination workers — restore broadcast and peer-online resync
/// (issue #1017).
pub struct ActiveClipboardFacade {
    inbound_uc: Arc<ApplyInboundActiveClipboardStateUseCase>,
    dispatch: Arc<dyn ActiveClipboardDispatchPort>,
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    member_repo: Arc<dyn MemberRepositoryPort>,
    settings: Arc<dyn SettingsPort>,
    presence: Arc<dyn PresencePort>,
    load_register: Arc<dyn LoadActiveClipboardPort>,
    reconstructor: SnapshotReconstructor,
    local_advancer: LocalActiveRegisterAdvancer,
    send_gate: MemberSendGate,
    // Resurface deps — used by the converged-event subscriber worker.
    touch_entry: Arc<dyn TouchClipboardEntryPort>,
    host_event_emitter: SharedHostEventEmitter,
    resurface_clock: Arc<dyn ClockPort>,
}

impl ActiveClipboardFacade {
    pub fn new(deps: ActiveClipboardDeps) -> Self {
        let reconstructor = deps.snapshot.into_reconstructor();
        let local_advancer = LocalActiveRegisterAdvancer::new(
            Arc::clone(&deps.advance_register),
            deps.device_identity,
            Arc::clone(&deps.clock),
        );
        let send_gate = MemberSendGate::new(Arc::clone(&deps.member_repo));

        let (converged_tx, _) = broadcast::channel::<ActiveClipboardConvergedEvent>(16);

        let mut inbound_uc = ApplyInboundActiveClipboardStateUseCase::new(
            deps.receiver,
            deps.is_unlocked,
            Arc::clone(&deps.load_register),
            deps.advance_register,
            Arc::clone(&deps.member_repo),
            deps.entry_lookup,
            reconstructor.clone(),
            deps.coordinator,
            Arc::clone(&deps.dispatch),
            Arc::clone(&deps.peer_addr_repo),
            Arc::clone(&deps.presence),
            deps.clock,
            converged_tx,
        );

        match (&deps.pull_client, &deps.pull_apply) {
            (Some(_), None) | (None, Some(_)) => {
                warn!("active clipboard: partial pull dependency — both pull_client and pull_apply must be provided together; pull disabled");
            }
            _ => {}
        }
        if let (Some(pull_client), Some(pull_apply)) = (deps.pull_client, deps.pull_apply) {
            let store: Arc<dyn InboundPulledContentStore> = Arc::new(PulledContentStore {
                cipher: Arc::clone(&deps.transfer_cipher),
                apply: pull_apply,
            });
            inbound_uc = inbound_uc.with_pull(pull_client, store);
        }
        let inbound_uc = Arc::new(inbound_uc);

        Self {
            inbound_uc,
            dispatch: deps.dispatch,
            peer_addr_repo: deps.peer_addr_repo,
            member_repo: deps.member_repo,
            settings: deps.settings,
            presence: deps.presence,
            load_register: deps.load_register,
            reconstructor,
            local_advancer,
            send_gate,
            touch_entry: deps.touch_entry,
            host_event_emitter: deps.host_event_emitter,
            resurface_clock: deps.resurface_clock,
        }
    }

    /// Announce a locally-originated activation of this device's clipboard
    /// (issue #1017 D1 call-sites 3 & 4, D2 "Mobile push → fan-out").
    ///
    /// Stamps a fresh activation `(now, this_device)` for `snapshot_hash` (held
    /// locally as `entry_id`), advances the cross-device register, then fans
    /// the converged 0xC3 state out to every send-allowed peer through the
    /// shared fan-out. The outbound gate is the full per-device send gate
    /// (`send_enabled` ∧ `send_content_types`, the latter via `categories`) —
    /// **not** `sync_on_restore`, which gates only history-restore broadcasts.
    ///
    /// Best-effort and fire-and-forget at the call site: a register storage
    /// hiccup is logged and swallowed by the advancer, and per-peer dispatch
    /// failures are isolated by the fan-out.
    pub async fn announce_local_activation(
        &self,
        snapshot_hash: String,
        entry_id: EntryId,
        categories: ClipboardContentCategorySet,
    ) {
        let state = self
            .local_advancer
            .advance_local(snapshot_hash, entry_id)
            .await;
        fan_out_active_state(
            &self.dispatch,
            &self.peer_addr_repo,
            &self.presence,
            &self.send_gate,
            &state,
            &categories,
        )
        .await;
    }

    /// Spawn the inbound convergence loop. Caller owns the returned handle;
    /// dropping it (or `abort()`) terminates the loop. The loop also exits on
    /// its own when the receiver adapter shuts down.
    pub fn spawn_inbound_loop(&self) -> ActiveClipboardInboundHandle {
        Arc::clone(&self.inbound_uc).spawn_run()
    }

    /// Spawn the outbound restore-broadcast worker. `rx` is the receiving end
    /// of the restore-broadcast channel whose sender side
    /// ([`RestoreBroadcastTrigger`](crate::clipboard_write::RestoreBroadcastTrigger))
    /// the restore use cases hold. The worker debounces rapid restores, gates
    /// on `sync_on_restore` plus the per-device send preferences, and fans the
    /// activation out to allowed peers through the shared fan-out. Caller owns
    /// the returned handle; dropping it terminates the worker (which also exits
    /// on its own once every trigger sender is dropped).
    pub fn spawn_restore_broadcast(
        &self,
        rx: UnboundedReceiver<RestoreBroadcastRequest>,
    ) -> RestoreBroadcastHandle {
        RestoreBroadcastWorker::new(
            rx,
            Arc::clone(&self.settings),
            Arc::clone(&self.dispatch),
            Arc::clone(&self.peer_addr_repo),
            Arc::clone(&self.presence),
            Arc::clone(&self.member_repo),
        )
        .spawn()
    }

    /// Spawn the peer-online resync worker (issue #1017 PR5, D10). The worker
    /// subscribes to presence transitions; when a peer comes online it
    /// debounces a burst (D7, ~1.5s), loads the current register, reconstructs
    /// the activation's content category for the outbound gate, and resends
    /// the current state to each newly-online peer (full outbound gate:
    /// `send_enabled` ∧ `send_content_types`). The resync is symmetric — the
    /// peer runs the same worker and resends to us; LWW converges both ends
    /// with no ack. Caller owns the returned handle; dropping it terminates
    /// the worker (which also exits when the presence subscription closes at
    /// router shutdown).
    pub fn spawn_peer_online_resync(&self) -> PeerOnlineResyncHandle {
        PeerOnlineResyncWorker::new(
            Arc::clone(&self.presence),
            Arc::clone(&self.load_register),
            self.reconstructor.clone(),
            Arc::clone(&self.dispatch),
            Arc::clone(&self.member_repo),
        )
        .spawn()
    }

    /// Spawn a worker that subscribes to inbound convergence events and
    /// resurfaces the converged entry in clipboard history (bumps
    /// `active_time_ms` + notifies the frontend). Decouples history ordering
    /// from the convergence use case.
    pub fn spawn_resurface_worker(&self) -> ActiveClipboardResurfaceHandle {
        let mut rx = self.inbound_uc.subscribe_converged();
        let touch = Arc::clone(&self.touch_entry);
        let bus = Arc::clone(&self.host_event_emitter);
        let clock = Arc::clone(&self.resurface_clock);

        let join = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        resurface_entry(touch.as_ref(), &bus, clock.as_ref(), &event.entry_id)
                            .await;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!(
                            missed = n,
                            "resurface worker lagged; some entries may not resurface immediately"
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        ActiveClipboardResurfaceHandle { join }
    }
}

/// Handle owning the resurface worker. Drop or `abort()` to stop it.
pub struct ActiveClipboardResurfaceHandle {
    join: JoinHandle<()>,
}

impl ActiveClipboardResurfaceHandle {
    pub fn abort(&self) {
        self.join.abort();
    }
}

impl Drop for ActiveClipboardResurfaceHandle {
    fn drop(&mut self) {
        self.join.abort();
    }
}

#[instrument(name = "active_state.resurface", skip_all, fields(entry_id = %entry_id))]
async fn resurface_entry(
    touch: &dyn TouchClipboardEntryPort,
    bus: &SharedHostEventEmitter,
    clock: &dyn ClockPort,
    entry_id: &EntryId,
) {
    let now_ms = clock.now_ms();
    match touch.touch_entry(entry_id, now_ms).await {
        Ok(true) => {
            debug!("entry resurfaced");
            bus.emit_or_warn(HostEvent::Clipboard(ClipboardHostEvent::NewContent {
                entry_id: entry_id.as_ref().to_string(),
                preview: "Clipboard restored".to_string(),
                origin: ClipboardOriginKind::Remote,
            }));
        }
        Ok(false) => {
            debug!("touch_entry found no row (entry deleted?)");
        }
        Err(err) => {
            warn!(error = %err, "touch_entry failed (best-effort, ignored)");
        }
    }
}

/// Inbound store half of the pull path (issue #1017 PR8). Decrypts a pulled
/// transfer envelope and persists it through the shared inbound apply path,
/// returning the local entry id. The wrapped apply path must **not** advance
/// the active-clipboard register — the inbound convergence tail owns that.
struct PulledContentStore {
    cipher: Arc<dyn TransferCipherPort>,
    apply: Arc<dyn InboundClipboardApplyPort>,
}

#[async_trait]
impl InboundPulledContentStore for PulledContentStore {
    async fn store(
        &self,
        from_device: &DeviceId,
        snapshot_hash: &str,
        transfer_envelope: Vec<u8>,
    ) -> Result<EntryId, InboundPulledContentStoreError> {
        // Decrypt the transfer envelope into the V3 plaintext the inbound apply
        // path decodes. A locked session (between the pull and the store) or a
        // tampered envelope surfaces here.
        let plaintext = self
            .cipher
            .decrypt(&transfer_envelope)
            .await
            .map_err(|err| InboundPulledContentStoreError::Decrypt(err.to_string()))?;

        // Persist via the shared inbound apply path (decode V3 → materialize
        // blobs → capture). Reuses the same pipeline the bulk 0xC1 path uses,
        // so the pulled entry's schema matches a normal inbound entry.
        let outcome = self
            .apply
            .apply(InboundClipboardApplyInput {
                from_device: from_device.as_str().to_string(),
                snapshot_hash: snapshot_hash.to_string(),
                plaintext: plaintext.into(),
                flow_id: None,
            })
            .await
            .map_err(|err| InboundPulledContentStoreError::Store(err.to_string()))?;

        match outcome {
            InboundClipboardApplyOutcome::Applied { entry_id } => Ok(EntryId::from(entry_id)),
            // A duplicate means the content landed locally between the pull and
            // the store (e.g. the bulk path raced us); the existing entry is
            // exactly what we wanted, so converge on it.
            InboundClipboardApplyOutcome::DuplicateSkipped {
                existing_entry_id, ..
            } => Ok(EntryId::from(existing_entry_id)),
            InboundClipboardApplyOutcome::DecodeFailed { reason } => {
                warn!(reason, "pulled content store: envelope decode failed");
                Err(InboundPulledContentStoreError::Store(format!(
                    "decode: {reason}"
                )))
            }
        }
    }
}

/// Re-exported handle so bootstrap can hold the restore-broadcast worker's
/// lifetime alongside the inbound loop handle.
pub use crate::usecases::clipboard_sync::active_state::restore_broadcast_worker::RestoreBroadcastHandle as ActiveClipboardRestoreBroadcastHandle;

/// Re-exported handle so bootstrap can hold the peer-online resync worker's
/// lifetime alongside the other active-clipboard worker handles.
pub use crate::usecases::clipboard_sync::active_state::peer_online_resync_worker::PeerOnlineResyncHandle as ActiveClipboardPeerOnlineResyncHandle;
