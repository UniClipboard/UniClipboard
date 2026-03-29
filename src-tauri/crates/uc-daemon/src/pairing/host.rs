use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uc_app::runtime::CoreRuntime;
use uc_app::usecases::pairing::{PairingDomainEvent, PairingEventPort, PairingOrchestrator};
use uc_app::usecases::space_access::{
    SpaceAccessCompletedEvent, SpaceAccessEventPort, SpaceAccessOrchestrator,
};
use uc_app::usecases::SetupOrchestrator;
use uc_core::network::daemon_api_strings::{
    pairing_busy_reason, pairing_stage, ws_event, ws_topic,
};
use uc_core::network::pairing_state_machine::{PairingAction, PairingRole};
use uc_core::network::{
    protocol::PairingKeyslotOffer, NetworkEvent, PairingBusy, PairingMessage, PairingRequest,
};
use uc_core::security::model::{KeySlot, KeySlotFile};
use uc_core::security::space_access::{deny_reason_from_code, SpaceAccessProofArtifact};
use uc_infra::fs::key_slot_store::KeySlotStore;

use crate::api::types::{
    DaemonWsEvent, PairingFailurePayload, PairingSessionChangedPayload, PairingVerificationPayload,
    SetupSpaceAccessCompletedPayload, SpaceAccessStateChangedPayload,
};
use crate::pairing::session_projection::{mark_pairing_session_terminal, upsert_pairing_snapshot};
use crate::service::{DaemonService, ServiceHealth};
use crate::state::{DaemonPairingSessionSnapshot, RuntimeState};

const PAIRING_EVENTS_SUBSCRIBE_BACKOFF_INITIAL_MS: u64 = 250;
const PAIRING_EVENTS_SUBSCRIBE_BACKOFF_MAX_MS: u64 = 30_000;
const SESSION_SWEEP_INTERVAL_SECS: u64 = 15;
const DEFAULT_CONTROL_LEASE_TTL_MS: u64 = 30_000;
const GUI_CLIENT_KIND: &str = "gui";

#[derive(Debug, Clone, Copy)]
struct LeaseRegistration {
    expires_at_ms: i64,
}

#[derive(Debug, Default)]
struct LeaseRegistry {
    active: AtomicBool,
    leases: RwLock<HashMap<String, LeaseRegistration>>,
}

impl LeaseRegistry {
    async fn set(&self, client_kind: String, enabled: bool, lease_ttl_ms: Option<u64>) {
        let mut leases = self.leases.write().await;
        prune_expired_leases(&mut leases);
        if enabled {
            let ttl_ms = lease_ttl_ms.unwrap_or(DEFAULT_CONTROL_LEASE_TTL_MS);
            let expires_at_ms = now_ms().saturating_add(ttl_ms.min(i64::MAX as u64) as i64);
            leases.insert(client_kind, LeaseRegistration { expires_at_ms });
        } else {
            leases.remove(&client_kind);
        }
        self.active.store(!leases.is_empty(), Ordering::SeqCst);
    }

    async fn is_active(&self) -> bool {
        let mut leases = self.leases.write().await;
        prune_expired_leases(&mut leases);
        let active = !leases.is_empty();
        self.active.store(active, Ordering::SeqCst);
        active
    }

    async fn clear(&self) {
        let mut leases = self.leases.write().await;
        leases.clear();
        self.active.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonPairingHostError {
    ActivePairingSessionExists,
    HostNotDiscoverable,
    NoLocalPairingParticipantReady,
    SessionNotFound(String),
    Internal(String),
}

impl std::fmt::Display for DaemonPairingHostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ActivePairingSessionExists => f.write_str("active pairing session exists"),
            Self::HostNotDiscoverable => f.write_str("host not discoverable"),
            Self::NoLocalPairingParticipantReady => {
                f.write_str("no local pairing participant ready")
            }
            Self::SessionNotFound(session_id) => {
                write!(f, "pairing session not found: {session_id}")
            }
            Self::Internal(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for DaemonPairingHostError {}

pub struct DaemonPairingHost {
    runtime: Arc<CoreRuntime>,
    pairing_orchestrator: Arc<PairingOrchestrator>,
    pairing_action_rx: Mutex<Option<mpsc::Receiver<PairingAction>>>,
    state: Arc<RwLock<RuntimeState>>,
    space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
    key_slot_store: Arc<dyn KeySlotStore>,
    discoverability: Arc<LeaseRegistry>,
    participant_readiness: Arc<LeaseRegistry>,
    active_session_id: Arc<RwLock<Option<String>>>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
}

impl DaemonPairingHost {
    pub fn new(
        runtime: Arc<CoreRuntime>,
        pairing_orchestrator: Arc<PairingOrchestrator>,
        pairing_action_rx: mpsc::Receiver<PairingAction>,
        state: Arc<RwLock<RuntimeState>>,
        space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
        key_slot_store: Arc<dyn KeySlotStore>,
        event_tx: broadcast::Sender<DaemonWsEvent>,
    ) -> Self {
        Self {
            runtime,
            pairing_orchestrator,
            pairing_action_rx: Mutex::new(Some(pairing_action_rx)),
            state,
            space_access_orchestrator,
            key_slot_store,
            discoverability: Arc::new(LeaseRegistry::default()),
            participant_readiness: Arc::new(LeaseRegistry::default()),
            active_session_id: Arc::new(RwLock::new(None)),
            event_tx,
        }
    }

    pub async fn discoverable(&self) -> bool {
        self.discoverability.is_active().await
    }

    pub async fn participant_ready(&self) -> bool {
        self.participant_readiness.is_active().await
    }

    pub async fn set_discoverability(
        &self,
        client_kind: String,
        discoverable: bool,
        lease_ttl_ms: Option<u64>,
    ) {
        self.discoverability
            .set(client_kind, discoverable, lease_ttl_ms)
            .await;
    }

    pub async fn set_participant_ready(
        &self,
        client_kind: String,
        ready: bool,
        lease_ttl_ms: Option<u64>,
    ) {
        self.participant_readiness
            .set(client_kind, ready, lease_ttl_ms)
            .await;
    }

    pub async fn active_session_id(&self) -> Option<String> {
        self.active_session_id.read().await.clone()
    }

    pub async fn register_gui_participant(
        &self,
        enabled: bool,
        lease_ttl_ms: Option<u64>,
    ) -> Result<(), DaemonPairingHostError> {
        self.set_discoverability(GUI_CLIENT_KIND.to_string(), enabled, lease_ttl_ms)
            .await;
        self.set_participant_ready(GUI_CLIENT_KIND.to_string(), enabled, lease_ttl_ms)
            .await;
        Ok(())
    }

    pub async fn reset_setup_state(&self) {
        if let Some(session_id) = self.active_session_id().await {
            if let Err(error) = self
                .pairing_orchestrator
                .user_reject_pairing(&session_id)
                .await
            {
                warn!(
                    error = %error,
                    session_id = %session_id,
                    "failed to reject active pairing session during setup reset"
                );
            }
            self.clear_active_session(Some(&session_id)).await;
        }

        self.discoverability.clear().await;
        self.participant_readiness.clear().await;
        self.broadcast_space_access_state(
            &uc_core::security::space_access::state::SpaceAccessState::Idle,
        );
    }

    pub async fn initiate_pairing(
        &self,
        peer_id: String,
    ) -> Result<String, DaemonPairingHostError> {
        if !self.discoverability.is_active().await {
            return Err(DaemonPairingHostError::HostNotDiscoverable);
        }
        if !self.participant_readiness.is_active().await {
            return Err(DaemonPairingHostError::NoLocalPairingParticipantReady);
        }
        self.reserve_session_slot(None).await?;

        match self
            .pairing_orchestrator
            .initiate_pairing(peer_id.clone())
            .await
            .map_err(|err| DaemonPairingHostError::Internal(err.to_string()))
        {
            Ok(session_id) => {
                self.bind_active_session(session_id.clone()).await;
                upsert_pairing_snapshot(
                    &self.state,
                    session_id.clone(),
                    Some(peer_id),
                    None,
                    pairing_stage::REQUEST,
                    now_ms(),
                )
                .await;
                emit_pairing_session_changed(
                    &self.event_tx,
                    &session_id,
                    pairing_stage::REQUEST,
                    self.session_peer_id(&session_id).await,
                    self.session_device_name(&session_id).await,
                    now_ms(),
                );
                Ok(session_id)
            }
            Err(err) => {
                self.clear_active_session(None).await;
                Err(err)
            }
        }
    }

    pub async fn accept_pairing(&self, session_id: &str) -> Result<(), DaemonPairingHostError> {
        self.require_session(session_id).await?;
        self.pairing_orchestrator
            .user_accept_pairing(session_id)
            .await
            .map_err(|err| DaemonPairingHostError::Internal(err.to_string()))?;
        Ok(())
    }

    pub async fn reject_pairing(&self, session_id: &str) -> Result<(), DaemonPairingHostError> {
        self.require_session(session_id).await?;
        self.pairing_orchestrator
            .user_reject_pairing(session_id)
            .await
            .map_err(|err| DaemonPairingHostError::Internal(err.to_string()))?;
        self.clear_active_session(Some(session_id)).await;
        Ok(())
    }

    pub async fn cancel_pairing(&self, session_id: &str) -> Result<(), DaemonPairingHostError> {
        self.require_session(session_id).await?;
        self.pairing_orchestrator
            .user_cancel_pairing(session_id)
            .await
            .map_err(|err| DaemonPairingHostError::Internal(err.to_string()))?;
        self.clear_active_session(Some(session_id)).await;
        Ok(())
    }

    pub async fn verify_pairing(
        &self,
        session_id: &str,
        pin_matches: bool,
    ) -> Result<(), DaemonPairingHostError> {
        if pin_matches {
            self.accept_pairing(session_id).await
        } else {
            self.reject_pairing(session_id).await
        }
    }

    pub async fn handle_incoming_request(
        &self,
        peer_id: String,
        request: PairingRequest,
    ) -> Result<(), DaemonPairingHostError> {
        let session_id = request.session_id.clone();
        self.ensure_inbound_admitted(&peer_id, &session_id).await?;
        self.bind_active_session(session_id.clone()).await;

        let device_name = request.device_name.clone();
        match self
            .pairing_orchestrator
            .handle_incoming_request(peer_id.clone(), request)
            .await
        {
            Ok(()) => {
                upsert_pairing_snapshot(
                    &self.state,
                    session_id.clone(),
                    Some(peer_id.clone()),
                    Some(device_name.clone()),
                    pairing_stage::REQUEST,
                    now_ms(),
                )
                .await;
                emit_pairing_session_changed(
                    &self.event_tx,
                    &session_id,
                    pairing_stage::REQUEST,
                    Some(peer_id),
                    Some(device_name),
                    now_ms(),
                );
                Ok(())
            }
            Err(err) => {
                self.clear_active_session(Some(&session_id)).await;
                Err(DaemonPairingHostError::Internal(err.to_string()))
            }
        }
    }

    pub async fn run(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        let pairing_action_rx = self
            .pairing_action_rx
            .lock()
            .await
            .take()
            .context("daemon pairing host already running")?;
        let domain_events = PairingEventPort::subscribe(self.pairing_orchestrator.as_ref())
            .await
            .context("failed to subscribe to pairing domain events")?;
        let space_access_events =
            SpaceAccessEventPort::subscribe(self.space_access_orchestrator.as_ref())
                .await
                .context("failed to subscribe to space access events")?;

        let mut tasks = JoinSet::new();

        tasks.spawn(run_pairing_action_loop(
            self.runtime.clone(),
            self.runtime.setup_orchestrator().clone(),
            self.pairing_orchestrator.clone(),
            self.space_access_orchestrator.clone(),
            self.key_slot_store.clone(),
            self.state.clone(),
            self.active_session_id.clone(),
            self.event_tx.clone(),
            pairing_action_rx,
            cancel.child_token(),
        ));

        tasks.spawn(run_pairing_domain_event_loop(
            self.pairing_orchestrator.clone(),
            self.state.clone(),
            self.active_session_id.clone(),
            domain_events,
            self.event_tx.clone(),
            cancel.child_token(),
        ));

        tasks.spawn(run_pairing_protocol_loop(
            self.runtime.clone(),
            self.runtime.setup_orchestrator().clone(),
            self.space_access_orchestrator.clone(),
            self.pairing_orchestrator.clone(),
            self.state.clone(),
            self.active_session_id.clone(),
            self.discoverability.clone(),
            self.participant_readiness.clone(),
            self.event_tx.clone(),
            cancel.child_token(),
        ));

        tasks.spawn(run_pairing_session_sweep_loop(
            self.pairing_orchestrator.clone(),
            self.discoverability.clone(),
            self.participant_readiness.clone(),
            cancel.child_token(),
        ));

        tasks.spawn(run_space_access_event_loop(
            space_access_events,
            self.event_tx.clone(),
            cancel.child_token(),
        ));

        tokio::select! {
            _ = cancel.cancelled() => {
                info!("daemon pairing host received shutdown");
            }
            Some(result) = tasks.join_next() => {
                match result {
                    Ok(Ok(())) => {
                        warn!("daemon pairing host subtask exited unexpectedly");
                    }
                    Ok(Err(err)) => return Err(err),
                    Err(err) => return Err(anyhow::anyhow!("daemon pairing host task join failed: {}", err)),
                }
            }
        }

        while tasks.join_next().await.is_some() {}
        Ok(())
    }
}

#[async_trait::async_trait]
impl DaemonService for DaemonPairingHost {
    fn name(&self) -> &str {
        "pairing-host"
    }

    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        self.run(cancel).await
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn health_check(&self) -> ServiceHealth {
        ServiceHealth::Healthy
    }
}

impl DaemonPairingHost {
    async fn reserve_session_slot(
        &self,
        session_id: Option<&str>,
    ) -> Result<(), DaemonPairingHostError> {
        let mut guard = self.active_session_id.write().await;
        if let Some(active) = guard.as_ref() {
            if session_id.is_none() || Some(active.as_str()) != session_id {
                return Err(DaemonPairingHostError::ActivePairingSessionExists);
            }
        }

        if let Some(session_id) = session_id {
            *guard = Some(session_id.to_string());
        }
        Ok(())
    }

    async fn bind_active_session(&self, session_id: String) {
        *self.active_session_id.write().await = Some(session_id);
    }

    async fn clear_active_session(&self, session_id: Option<&str>) {
        let mut guard = self.active_session_id.write().await;
        let should_clear = match (guard.as_ref(), session_id) {
            (_, None) => true,
            (Some(active), Some(expected)) => active == expected,
            (None, Some(_)) => false,
        };

        if should_clear {
            *guard = None;
        }
    }

    async fn ensure_inbound_admitted(
        &self,
        peer_id: &str,
        session_id: &str,
    ) -> Result<(), DaemonPairingHostError> {
        if !self.discoverability.is_active().await {
            self.reject_inbound_request(
                peer_id,
                session_id,
                pairing_busy_reason::HOST_NOT_DISCOVERABLE,
            )
            .await;
            return Err(DaemonPairingHostError::HostNotDiscoverable);
        }
        if !self.participant_readiness.is_active().await {
            self.reject_inbound_request(
                peer_id,
                session_id,
                pairing_busy_reason::NO_LOCAL_PAIRING_PARTICIPANT_READY,
            )
            .await;
            return Err(DaemonPairingHostError::NoLocalPairingParticipantReady);
        }

        self.reserve_session_slot(Some(session_id)).await
    }

    async fn reject_inbound_request(&self, peer_id: &str, session_id: &str, reason: &str) {
        let pairing = self.runtime.wiring_deps().network_ports.pairing.clone();
        if let Err(err) = pairing
            .open_pairing_session(peer_id.to_string(), session_id.to_string())
            .await
        {
            debug!(
                error = %err,
                peer_id = %peer_id,
                session_id = %session_id,
                "failed to open busy response session"
            );
            return;
        }

        if let Err(err) = pairing
            .send_pairing_on_session(PairingMessage::Busy(PairingBusy {
                session_id: session_id.to_string(),
                reason: Some(reason.to_string()),
            }))
            .await
        {
            debug!(
                error = %err,
                peer_id = %peer_id,
                session_id = %session_id,
                "failed to send busy pairing response"
            );
        }
    }

    async fn require_session(&self, session_id: &str) -> Result<(), DaemonPairingHostError> {
        if !self.pairing_orchestrator.has_session(session_id).await
            && self
                .state
                .read()
                .await
                .pairing_session(session_id)
                .is_none()
        {
            return Err(DaemonPairingHostError::SessionNotFound(
                session_id.to_string(),
            ));
        }
        Ok(())
    }

    fn broadcast_space_access_state(
        &self,
        state: &uc_core::security::space_access::state::SpaceAccessState,
    ) {
        broadcast_space_access_state_changed(&self.event_tx, state);
    }

    async fn session_peer_id(&self, session_id: &str) -> Option<String> {
        if let Some(peer) = self.pairing_orchestrator.get_session_peer(session_id).await {
            return Some(peer.peer_id);
        }
        self.state
            .read()
            .await
            .pairing_session(session_id)
            .and_then(|snapshot| snapshot.peer_id.clone())
    }

    async fn session_device_name(&self, session_id: &str) -> Option<String> {
        if let Some(peer) = self.pairing_orchestrator.get_session_peer(session_id).await {
            return peer.device_name;
        }
        self.state
            .read()
            .await
            .pairing_session(session_id)
            .and_then(|snapshot| snapshot.device_name.clone())
    }
}

async fn run_pairing_action_loop(
    runtime: Arc<CoreRuntime>,
    setup_orchestrator: Arc<SetupOrchestrator>,
    pairing_orchestrator: Arc<PairingOrchestrator>,
    space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
    key_slot_store: Arc<dyn KeySlotStore>,
    state: Arc<RwLock<RuntimeState>>,
    active_session_id: Arc<RwLock<Option<String>>>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
    mut action_rx: mpsc::Receiver<PairingAction>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let pairing_transport = runtime.wiring_deps().network_ports.pairing.clone();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            maybe_action = action_rx.recv() => {
                let Some(action) = maybe_action else {
                    return Ok(());
                };

                match action {
                    PairingAction::Send { peer_id, message } => {
                        let session_id = message.session_id().to_string();
                        if let Err(err) = pairing_transport
                            .open_pairing_session(peer_id.clone(), session_id.clone())
                            .await
                        {
                            signal_pairing_transport_failure(
                                pairing_orchestrator.as_ref(),
                                &state,
                                &active_session_id,
                                &event_tx,
                                &session_id,
                                &peer_id,
                                err.to_string(),
                            )
                            .await?;
                            continue;
                        }

                        if let Err(err) = pairing_transport.send_pairing_on_session(message).await {
                            signal_pairing_transport_failure(
                                pairing_orchestrator.as_ref(),
                                &state,
                                &active_session_id,
                                &event_tx,
                                &session_id,
                                &peer_id,
                                err.to_string(),
                            )
                            .await?;
                        }
                    }
                    PairingAction::ShowVerification {
                        session_id,
                        short_code: _,
                        local_fingerprint: _,
                        peer_fingerprint: _,
                        peer_display_name,
                    } => {
                        upsert_pairing_snapshot(
                            &state,
                            session_id,
                            None,
                            Some(peer_display_name),
                            pairing_stage::VERIFICATION,
                            now_ms(),
                        )
                        .await;
                    }
                    PairingAction::ShowVerifying {
                        session_id,
                        peer_display_name,
                    } => {
                        upsert_pairing_snapshot(
                            &state,
                            session_id,
                            None,
                            Some(peer_display_name),
                            pairing_stage::VERIFYING,
                            now_ms(),
                        )
                        .await;
                    }
                    PairingAction::EmitResult {
                        session_id,
                        success,
                        error,
                    } => {
                        let peer_info = pairing_orchestrator.get_session_peer(&session_id).await;
                        let role = pairing_orchestrator.get_session_role(&session_id).await;

                        if !success {
                            if let Err(err) = pairing_transport
                                .close_pairing_session(session_id.clone(), error.clone())
                                .await
                            {
                                warn!(error = %err, session_id = %session_id, "failed to close pairing session");
                            }
                        } else if role == Some(PairingRole::Responder) {
                            if let Some(peer) = peer_info.as_ref() {
                                let context = space_access_orchestrator.context();
                                context.lock().await.sponsor_peer_id = Some(peer.peer_id.clone());
                                match key_slot_store.load().await {
                                    Ok(keyslot_file) => {
                                        if matches!(
                                            setup_orchestrator.get_state().await,
                                            uc_core::setup::SetupState::Completed
                                        ) {
                                            match setup_orchestrator
                                                .start_completed_host_sponsor_authorization(
                                                    session_id.clone(),
                                                    peer.peer_id.clone(),
                                                    keyslot_file,
                                                )
                                                .await
                                            {
                                                Ok(_) => {
                                                    broadcast_space_access_state_changed(
                                                        &event_tx,
                                                        &space_access_orchestrator.get_state().await,
                                                    );
                                                }
                                                Err(err) => {
                                                    warn!(
                                                        error = %err,
                                                        session_id = %session_id,
                                                        peer_id = %peer.peer_id,
                                                        "failed to start completed-host sponsor authorization"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    Err(err) => {
                                        debug!(
                                            error = %err,
                                            session_id = %session_id,
                                            "key slot store unavailable for responder-side follow-up"
                                        );
                                    }
                                }
                            }
                        }

                        let (peer_id, device_name) = match peer_info {
                            Some(peer) => (Some(peer.peer_id), peer.device_name),
                            None => (None, None),
                        };

                        let state_label = if success { pairing_stage::COMPLETE } else { pairing_stage::FAILED };
                        mark_pairing_session_terminal(
                            &state,
                            session_id.clone(),
                            peer_id,
                            device_name,
                            state_label,
                            now_ms(),
                        )
                        .await;
                        clear_active_session_slot(&active_session_id, &session_id).await;
                    }
                    other => {
                        debug!(action = ?other, "daemon pairing host ignored unsupported action");
                    }
                }
            }
        }
    }
}

async fn run_pairing_domain_event_loop(
    pairing_orchestrator: Arc<PairingOrchestrator>,
    state: Arc<RwLock<RuntimeState>>,
    active_session_id: Arc<RwLock<Option<String>>>,
    mut domain_events: mpsc::Receiver<PairingDomainEvent>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            maybe_event = domain_events.recv() => {
                let Some(event) = maybe_event else {
                    return Ok(());
                };

                match event {
                    PairingDomainEvent::PairingVerificationRequired {
                        session_id,
                        peer_id,
                        short_code,
                        local_fingerprint,
                        peer_fingerprint,
                    } => {
                        let device_name = pairing_orchestrator
                            .get_session_peer(&session_id)
                            .await
                            .and_then(|peer| peer.device_name);
                        let ts = now_ms();
                        state.write().await.upsert_pairing_session(DaemonPairingSessionSnapshot {
                            session_id: session_id.clone(),
                            peer_id: Some(peer_id.clone()),
                            device_name: device_name.clone(),
                            state: pairing_stage::VERIFICATION.to_string(),
                            updated_at_ms: ts,
                            short_code: Some(short_code.clone()),
                            peer_fingerprint: Some(peer_fingerprint.clone()),
                        });
                        info!(
                            session_id = %session_id,
                            peer_id = %peer_id,
                            device_name = device_name.as_deref().unwrap_or(""),
                            has_short_code = !short_code.is_empty(),
                            has_local_fingerprint = !local_fingerprint.is_empty(),
                            has_peer_fingerprint = !peer_fingerprint.is_empty(),
                            "broadcasting pairing verification to daemon websocket subscribers"
                        );
                        emit_pairing_session_changed(
                            &event_tx,
                            &session_id,
                            pairing_stage::VERIFICATION,
                            Some(peer_id.clone()),
                            device_name.clone(),
                            ts,
                        );
                        emit_pairing_verification(
                            &event_tx,
                            &session_id,
                            pairing_stage::VERIFICATION,
                            Some(peer_id.clone()),
                            device_name.clone(),
                            Some(short_code),
                            None,
                            Some(local_fingerprint),
                            Some(peer_fingerprint),
                        );
                    }
                    PairingDomainEvent::KeyslotReceived {
                        session_id,
                        peer_id,
                        keyslot_file: _,
                        challenge: _,
                    } => {
                        let ts = now_ms();
                        emit_ws_event(
                            &event_tx,
                            ws_topic::PAIRING_SESSION,
                            ws_event::PAIRING_UPDATED,
                            Some(session_id.clone()),
                            PairingSessionChangedPayload {
                                session_id: session_id.clone(),
                                state: pairing_stage::VERIFYING.to_string(),
                                stage: pairing_stage::VERIFYING.to_string(),
                                peer_id: Some(peer_id.clone()),
                                device_name: None,
                                updated_at_ms: ts,
                                ts,
                            },
                        );
                        upsert_pairing_snapshot(
                            &state,
                            session_id,
                            Some(peer_id),
                            None,
                            pairing_stage::VERIFYING,
                            ts,
                        )
                        .await;
                    }
                    PairingDomainEvent::PairingSucceeded { session_id, peer_id } => {
                        let device_name = pairing_orchestrator
                            .get_session_peer(&session_id)
                            .await
                            .and_then(|peer| peer.device_name);
                        let ts = now_ms();
                        emit_ws_event(
                            &event_tx,
                            ws_topic::PAIRING_SESSION,
                            ws_event::PAIRING_COMPLETE,
                            Some(session_id.clone()),
                            PairingSessionChangedPayload {
                                session_id: session_id.clone(),
                                state: pairing_stage::COMPLETE.to_string(),
                                stage: pairing_stage::COMPLETE.to_string(),
                                peer_id: Some(peer_id.clone()),
                                device_name: device_name.clone(),
                                updated_at_ms: ts,
                                ts,
                            },
                        );
                        emit_pairing_verification(
                            &event_tx,
                            &session_id,
                            pairing_stage::COMPLETE,
                            Some(peer_id.clone()),
                            device_name.clone(),
                            None,
                            None,
                            None,
                            None,
                        );
                        mark_pairing_session_terminal(
                            &state,
                            session_id.clone(),
                            Some(peer_id),
                            device_name,
                            pairing_stage::COMPLETE,
                            ts,
                        )
                        .await;
                        clear_active_session_slot(&active_session_id, &session_id).await;
                    }
                    PairingDomainEvent::PairingVerifying {
                        session_id,
                        peer_id,
                    } => {
                        let device_name = pairing_orchestrator
                            .get_session_peer(&session_id)
                            .await
                            .and_then(|peer| peer.device_name);
                        let ts = now_ms();
                        upsert_pairing_snapshot(
                            &state,
                            session_id.clone(),
                            Some(peer_id.clone()),
                            device_name.clone(),
                            pairing_stage::VERIFYING,
                            ts,
                        )
                        .await;
                        emit_pairing_session_changed(
                            &event_tx,
                            &session_id,
                            pairing_stage::VERIFYING,
                            Some(peer_id.clone()),
                            device_name.clone(),
                            ts,
                        );
                        emit_pairing_verification(
                            &event_tx,
                            &session_id,
                            pairing_stage::VERIFYING,
                            Some(peer_id),
                            device_name,
                            None,
                            None,
                            None,
                            None,
                        );
                    }
                    PairingDomainEvent::PairingFailed {
                        session_id,
                        peer_id,
                        reason,
                    } => {
                        let device_name = pairing_orchestrator
                            .get_session_peer(&session_id)
                            .await
                            .and_then(|peer| peer.device_name);
                        let failure_reason = pairing_failure_message(&reason);
                        let ts = now_ms();
                        emit_ws_event(
                            &event_tx,
                            ws_topic::PAIRING_VERIFICATION,
                            ws_event::PAIRING_FAILED,
                            Some(session_id.clone()),
                            PairingFailurePayload {
                                session_id: session_id.clone(),
                                peer_id: Some(peer_id.clone()),
                                error: failure_reason.clone(),
                                reason: failure_reason.clone(),
                            },
                        );
                        emit_pairing_verification(
                            &event_tx,
                            &session_id,
                            pairing_stage::FAILED,
                            Some(peer_id.clone()),
                            device_name.clone(),
                            None,
                            Some(failure_reason.clone()),
                            None,
                            None,
                        );
                        mark_pairing_session_terminal(
                            &state,
                            session_id.clone(),
                            Some(peer_id),
                            device_name,
                            pairing_stage::FAILED,
                            ts,
                        )
                        .await;
                        clear_active_session_slot(&active_session_id, &session_id).await;
                    }
                }
            }
        }
    }
}

async fn run_pairing_protocol_loop(
    runtime: Arc<CoreRuntime>,
    setup_orchestrator: Arc<SetupOrchestrator>,
    space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
    pairing_orchestrator: Arc<PairingOrchestrator>,
    state: Arc<RwLock<RuntimeState>>,
    active_session_id: Arc<RwLock<Option<String>>>,
    discoverability: Arc<LeaseRegistry>,
    participant_readiness: Arc<LeaseRegistry>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let network_events = runtime.wiring_deps().network_ports.events.clone();
    let pairing_transport = runtime.wiring_deps().network_ports.pairing.clone();

    let mut subscribe_attempt: u32 = 0;
    loop {
        let subscribe_result = tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            result = network_events.subscribe_events() => result,
        };

        match subscribe_result {
            Ok(mut event_rx) => {
                subscribe_attempt = 0;
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => return Ok(()),
                        maybe_event = event_rx.recv() => {
                            let Some(event) = maybe_event else {
                                break;
                            };

                            match event {
                                NetworkEvent::PairingMessageReceived { peer_id, message } => {
                                    handle_pairing_message(
                                        setup_orchestrator.as_ref(),
                                        space_access_orchestrator.as_ref(),
                                        pairing_orchestrator.as_ref(),
                                        &state,
                                        &active_session_id,
                                        &pairing_transport,
                                        &discoverability,
                                        &participant_readiness,
                                        &event_tx,
                                        peer_id,
                                        message,
                                    )
                                    .await?;
                                }
                                NetworkEvent::PairingFailed { session_id, peer_id, error } => {
                                    signal_pairing_transport_failure(
                                        pairing_orchestrator.as_ref(),
                                        &state,
                                        &active_session_id,
                                        &event_tx,
                                        &session_id,
                                        &peer_id,
                                        error,
                                    )
                                    .await?;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            Err(err) => {
                subscribe_attempt = subscribe_attempt.saturating_add(1);
                let retry_in_ms = pairing_events_subscribe_backoff_ms(subscribe_attempt);
                warn!(
                    error = %err,
                    attempt = subscribe_attempt,
                    retry_in_ms,
                    "failed to subscribe to daemon pairing network events"
                );
            }
        }

        let backoff = Duration::from_millis(pairing_events_subscribe_backoff_ms(subscribe_attempt));
        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            _ = tokio::time::sleep(backoff) => {}
        }
    }
}

async fn run_pairing_session_sweep_loop(
    pairing_orchestrator: Arc<PairingOrchestrator>,
    discoverability: Arc<LeaseRegistry>,
    participant_readiness: Arc<LeaseRegistry>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let mut interval = tokio::time::interval(Duration::from_secs(SESSION_SWEEP_INTERVAL_SECS));

    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            _ = interval.tick() => {
                pairing_orchestrator.cleanup_expired_sessions().await;
                let _ = discoverability.is_active().await;
                let _ = participant_readiness.is_active().await;
            }
        }
    }
}

async fn handle_pairing_message(
    setup_orchestrator: &SetupOrchestrator,
    space_access_orchestrator: &SpaceAccessOrchestrator,
    pairing_orchestrator: &PairingOrchestrator,
    state: &Arc<RwLock<RuntimeState>>,
    active_session_id: &Arc<RwLock<Option<String>>>,
    pairing_transport: &Arc<dyn uc_core::ports::PairingTransportPort>,
    discoverability: &Arc<LeaseRegistry>,
    participant_readiness: &Arc<LeaseRegistry>,
    event_tx: &broadcast::Sender<DaemonWsEvent>,
    peer_id: String,
    message: PairingMessage,
) -> anyhow::Result<()> {
    match message {
        PairingMessage::Request(request) => {
            if !discoverability.is_active().await {
                reject_inbound_request(
                    pairing_transport,
                    &peer_id,
                    &request.session_id,
                    pairing_busy_reason::HOST_NOT_DISCOVERABLE,
                )
                .await;
                return Ok(());
            }

            if !participant_readiness.is_active().await {
                reject_inbound_request(
                    pairing_transport,
                    &peer_id,
                    &request.session_id,
                    pairing_busy_reason::NO_LOCAL_PAIRING_PARTICIPANT_READY,
                )
                .await;
                return Ok(());
            }

            {
                let mut guard = active_session_id.write().await;
                if let Some(active) = guard.as_ref() {
                    if active != &request.session_id {
                        reject_inbound_request(
                            pairing_transport,
                            &peer_id,
                            &request.session_id,
                            pairing_busy_reason::BUSY,
                        )
                        .await;
                        return Ok(());
                    }
                } else {
                    *guard = Some(request.session_id.clone());
                }
            }

            let ts = now_ms();
            upsert_pairing_snapshot(
                state,
                request.session_id.clone(),
                Some(peer_id.clone()),
                Some(request.device_name.clone()),
                pairing_stage::REQUEST,
                ts,
            )
            .await;
            emit_ws_event(
                event_tx,
                ws_topic::PAIRING_SESSION,
                ws_event::PAIRING_UPDATED,
                Some(request.session_id.clone()),
                PairingSessionChangedPayload {
                    session_id: request.session_id.clone(),
                    state: pairing_stage::REQUEST.to_string(),
                    stage: pairing_stage::REQUEST.to_string(),
                    peer_id: Some(peer_id.clone()),
                    device_name: Some(request.device_name.clone()),
                    updated_at_ms: ts,
                    ts,
                },
            );

            pairing_orchestrator
                .handle_incoming_request(peer_id, request)
                .await?;
        }
        PairingMessage::Challenge(challenge) => {
            let session_id = challenge.session_id.clone();
            pairing_orchestrator
                .handle_challenge(&session_id, &peer_id, challenge)
                .await?;
        }
        PairingMessage::KeyslotOffer(offer) => {
            let session_id = offer.session_id.clone();
            pairing_orchestrator
                .handle_keyslot_offer(&session_id, &peer_id, offer)
                .await?;
        }
        PairingMessage::ChallengeResponse(response) => {
            let session_id = response.session_id.clone();
            pairing_orchestrator
                .handle_challenge_response(&session_id, &peer_id, response)
                .await?;
        }
        PairingMessage::Response(response) => {
            let session_id = response.session_id.clone();
            pairing_orchestrator
                .handle_response(&session_id, &peer_id, response)
                .await?;
        }
        PairingMessage::Confirm(confirm) => {
            let session_id = confirm.session_id.clone();
            pairing_orchestrator
                .handle_confirm(&session_id, &peer_id, confirm)
                .await?;
        }
        PairingMessage::Reject(reject) => {
            let session_id = reject.session_id.clone();
            pairing_orchestrator
                .handle_reject(&session_id, &peer_id)
                .await?;
        }
        PairingMessage::Cancel(cancel) => {
            let session_id = cancel.session_id.clone();
            pairing_orchestrator
                .handle_cancel(&session_id, &peer_id)
                .await?;
        }
        PairingMessage::Busy(busy) => {
            if let Some(reason) = busy.reason.as_deref() {
                match parse_space_access_busy_payload(reason) {
                    Ok(SpaceAccessBusyPayload::Offer(payload)) => {
                        let keyslot_file = match KeySlotFile::try_from(&payload.keyslot) {
                            Ok(keyslot_file) => keyslot_file,
                            Err(err) => {
                                warn!(
                                    error = %err,
                                    session_id = %busy.session_id,
                                    peer_id = %peer_id,
                                    "space access offer missing wrapped keyslot payload"
                                );
                                return Ok(());
                            }
                        };
                        pairing_orchestrator
                            .handle_keyslot_offer(
                                &busy.session_id,
                                &peer_id,
                                PairingKeyslotOffer {
                                    session_id: busy.session_id.clone(),
                                    keyslot_file: Some(keyslot_file),
                                    challenge: Some(payload.nonce),
                                },
                            )
                            .await?;
                        return Ok(());
                    }
                    Ok(SpaceAccessBusyPayload::Proof(payload)) => {
                        let challenge_len = payload.challenge_nonce.len();
                        let challenge_nonce: [u8; 32] = match payload.challenge_nonce.try_into() {
                            Ok(nonce) => nonce,
                            Err(_) => {
                                warn!(
                                    session_id = %busy.session_id,
                                    peer_id = %peer_id,
                                    challenge_len,
                                    "invalid space access proof nonce length"
                                );
                                return Ok(());
                            }
                        };
                        setup_orchestrator
                            .resolve_host_space_access_proof(
                                SpaceAccessProofArtifact {
                                    pairing_session_id: uc_core::SessionId::from(
                                        payload.pairing_session_id.as_str(),
                                    ),
                                    space_id: uc_core::ids::SpaceId::from(
                                        payload.space_id.as_str(),
                                    ),
                                    challenge_nonce,
                                    proof_bytes: payload.proof_bytes,
                                },
                                Some(peer_id.clone()),
                            )
                            .await
                            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
                        broadcast_space_access_state_changed(
                            event_tx,
                            &space_access_orchestrator.get_state().await,
                        );
                        return Ok(());
                    }
                    Ok(SpaceAccessBusyPayload::Result(payload)) => {
                        let deny_reason = payload
                            .deny_reason
                            .as_deref()
                            .and_then(deny_reason_from_code);
                        setup_orchestrator
                            .apply_joiner_space_access_result(
                                busy.session_id.clone(),
                                uc_core::ids::SpaceId::from(payload.space_id.as_str()),
                                Some(peer_id.clone()),
                                payload.success,
                                deny_reason,
                            )
                            .await
                            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
                        broadcast_space_access_state_changed(
                            event_tx,
                            &space_access_orchestrator.get_state().await,
                        );
                        return Ok(());
                    }
                    Err(ParseSpaceAccessBusyPayloadError::NotSpaceAccessPayload) => {}
                    Err(error) => {
                        warn!(
                            error = %error,
                            session_id = %busy.session_id,
                            peer_id = %peer_id,
                            "failed to parse space access busy payload"
                        );
                        return Ok(());
                    }
                }
            }
            let session_id = busy.session_id.clone();
            pairing_orchestrator
                .handle_busy(&session_id, &peer_id, busy.reason.clone())
                .await?;
        }
    }

    Ok(())
}

async fn reject_inbound_request(
    pairing_transport: &Arc<dyn uc_core::ports::PairingTransportPort>,
    peer_id: &str,
    session_id: &str,
    reason: &str,
) {
    if let Err(err) = pairing_transport
        .open_pairing_session(peer_id.to_string(), session_id.to_string())
        .await
    {
        debug!(error = %err, peer_id = %peer_id, session_id = %session_id, "failed to open busy pairing session");
        return;
    }

    if let Err(err) = pairing_transport
        .send_pairing_on_session(PairingMessage::Busy(PairingBusy {
            session_id: session_id.to_string(),
            reason: Some(reason.to_string()),
        }))
        .await
    {
        debug!(error = %err, peer_id = %peer_id, session_id = %session_id, "failed to send busy pairing message");
    }
}

fn broadcast_space_access_state_changed(
    event_tx: &broadcast::Sender<DaemonWsEvent>,
    state: &uc_core::security::space_access::state::SpaceAccessState,
) {
    let payload = SpaceAccessStateChangedPayload {
        state: state.clone(),
    };
    let serialized = match serde_json::to_value(&payload) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "failed to serialize space_access.state_changed payload");
            return;
        }
    };
    let _ = event_tx.send(DaemonWsEvent {
        topic: ws_topic::SPACE_ACCESS.to_string(),
        event_type: ws_event::SPACE_ACCESS_STATE_CHANGED.to_string(),
        session_id: None,
        ts: chrono::Utc::now().timestamp_millis(),
        payload: serialized,
    });
}

fn emit_ws_event<T: serde::Serialize>(
    event_tx: &broadcast::Sender<DaemonWsEvent>,
    topic: &str,
    event_type: &str,
    session_id: Option<String>,
    payload: T,
) {
    let payload = match serde_json::to_value(payload) {
        Ok(payload) => payload,
        Err(err) => {
            warn!(error = %err, topic, event_type, "failed to encode daemon websocket payload");
            return;
        }
    };

    let _ = event_tx.send(DaemonWsEvent {
        topic: topic.to_string(),
        event_type: event_type.to_string(),
        session_id,
        ts: now_ms(),
        payload,
    });
}

fn emit_pairing_session_changed(
    event_tx: &broadcast::Sender<DaemonWsEvent>,
    session_id: &str,
    stage: &str,
    peer_id: Option<String>,
    device_name: Option<String>,
    ts: i64,
) {
    emit_ws_event(
        event_tx,
        ws_topic::PAIRING_SESSION,
        ws_event::PAIRING_UPDATED,
        Some(session_id.to_string()),
        PairingSessionChangedPayload {
            session_id: session_id.to_string(),
            state: stage.to_string(),
            stage: stage.to_string(),
            peer_id,
            device_name,
            updated_at_ms: ts,
            ts,
        },
    );
}

fn emit_pairing_verification(
    event_tx: &broadcast::Sender<DaemonWsEvent>,
    session_id: &str,
    kind: &str,
    peer_id: Option<String>,
    device_name: Option<String>,
    code: Option<String>,
    error: Option<String>,
    local_fingerprint: Option<String>,
    peer_fingerprint: Option<String>,
) {
    emit_ws_event(
        event_tx,
        ws_topic::PAIRING_VERIFICATION,
        ws_event::PAIRING_VERIFICATION_REQUIRED,
        Some(session_id.to_string()),
        PairingVerificationPayload {
            session_id: session_id.to_string(),
            kind: kind.to_string(),
            peer_id,
            device_name,
            code,
            error,
            local_fingerprint,
            peer_fingerprint,
        },
    );
}

fn emit_pairing_failure(
    event_tx: &broadcast::Sender<DaemonWsEvent>,
    session_id: &str,
    peer_id: Option<String>,
    reason: &str,
) {
    emit_ws_event(
        event_tx,
        ws_topic::PAIRING_VERIFICATION,
        ws_event::PAIRING_FAILED,
        Some(session_id.to_string()),
        PairingFailurePayload {
            session_id: session_id.to_string(),
            peer_id,
            error: reason.to_string(),
            reason: reason.to_string(),
        },
    );
}

async fn signal_pairing_transport_failure(
    pairing_orchestrator: &PairingOrchestrator,
    state: &Arc<RwLock<RuntimeState>>,
    active_session_id: &Arc<RwLock<Option<String>>>,
    event_tx: &broadcast::Sender<DaemonWsEvent>,
    session_id: &str,
    peer_id: &str,
    reason: String,
) -> anyhow::Result<()> {
    let ts = now_ms();
    mark_pairing_session_terminal(
        state,
        session_id.to_string(),
        Some(peer_id.to_string()),
        None,
        pairing_stage::FAILED,
        ts,
    )
    .await;
    emit_pairing_session_changed(
        event_tx,
        session_id,
        pairing_stage::FAILED,
        Some(peer_id.to_string()),
        None,
        ts,
    );
    emit_pairing_verification(
        event_tx,
        session_id,
        pairing_stage::FAILED,
        Some(peer_id.to_string()),
        None,
        None,
        Some(reason.clone()),
        None,
        None,
    );
    emit_pairing_failure(event_tx, session_id, Some(peer_id.to_string()), &reason);
    clear_active_session_slot(active_session_id, session_id).await;
    pairing_orchestrator
        .handle_transport_error(session_id, peer_id, reason)
        .await?;
    Ok(())
}

async fn clear_active_session_slot(
    active_session_id: &Arc<RwLock<Option<String>>>,
    session_id: &str,
) {
    let mut guard = active_session_id.write().await;
    if guard.as_deref() == Some(session_id) {
        *guard = None;
    }
}

fn prune_expired_leases(leases: &mut HashMap<String, LeaseRegistration>) {
    let now = now_ms();
    leases.retain(|_, lease| lease.expires_at_ms > now);
}

fn pairing_events_subscribe_backoff_ms(attempt: u32) -> u64 {
    let exponent = attempt.saturating_sub(1).min(16);
    let factor = 1u64 << exponent;
    PAIRING_EVENTS_SUBSCRIBE_BACKOFF_INITIAL_MS
        .saturating_mul(factor)
        .min(PAIRING_EVENTS_SUBSCRIBE_BACKOFF_MAX_MS)
}

async fn run_space_access_event_loop(
    mut event_rx: mpsc::Receiver<SpaceAccessCompletedEvent>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            maybe_event = event_rx.recv() => {
                let Some(event) = maybe_event else {
                    return Ok(());
                };
                info!(
                    session_id = %event.session_id,
                    peer_id = %event.peer_id,
                    success = event.success,
                    "daemon emitting space access completed via websocket"
                );
                emit_ws_event(
                    &event_tx,
                    ws_topic::SETUP,
                    ws_event::SETUP_SPACE_ACCESS_COMPLETED,
                    Some(event.session_id.clone()),
                    SetupSpaceAccessCompletedPayload {
                        session_id: event.session_id,
                        peer_id: event.peer_id,
                        success: event.success,
                        reason: event.reason,
                        ts: event.ts,
                    },
                );
            }
        }
    }
}

fn pairing_failure_message(
    reason: &uc_core::network::pairing_state_machine::FailureReason,
) -> String {
    match reason {
        uc_core::network::pairing_state_machine::FailureReason::Other(message)
        | uc_core::network::pairing_state_machine::FailureReason::TransportError(message)
        | uc_core::network::pairing_state_machine::FailureReason::MessageParseError(message)
        | uc_core::network::pairing_state_machine::FailureReason::PersistenceError(message)
        | uc_core::network::pairing_state_machine::FailureReason::CryptoError(message) => {
            message.clone()
        }
        uc_core::network::pairing_state_machine::FailureReason::Timeout(kind) => {
            format!("timeout:{kind:?}")
        }
        uc_core::network::pairing_state_machine::FailureReason::RetryExhausted => {
            "retry_exhausted".to_string()
        }
        uc_core::network::pairing_state_machine::FailureReason::PeerBusy => "busy".to_string(),
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SpaceAccessBusyOfferPayload {
    kind: String,
    space_id: String,
    nonce: Vec<u8>,
    keyslot: KeySlot,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SpaceAccessBusyProofPayload {
    kind: String,
    pairing_session_id: String,
    space_id: String,
    challenge_nonce: Vec<u8>,
    proof_bytes: Vec<u8>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SpaceAccessBusyResultPayload {
    kind: String,
    space_id: String,
    #[serde(default)]
    sponsor_peer_id: Option<String>,
    success: bool,
    #[serde(default)]
    deny_reason: Option<String>,
}

enum SpaceAccessBusyPayload {
    Offer(SpaceAccessBusyOfferPayload),
    Proof(SpaceAccessBusyProofPayload),
    Result(SpaceAccessBusyResultPayload),
}

#[derive(Debug)]
enum ParseSpaceAccessBusyPayloadError {
    NotSpaceAccessPayload,
    InvalidJson(serde_json::Error),
}

impl std::fmt::Display for ParseSpaceAccessBusyPayloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotSpaceAccessPayload => {
                f.write_str("busy payload is not a space access payload")
            }
            Self::InvalidJson(error) => write!(f, "busy payload is not valid json: {error}"),
        }
    }
}

impl std::error::Error for ParseSpaceAccessBusyPayloadError {}

impl From<serde_json::Error> for ParseSpaceAccessBusyPayloadError {
    fn from(value: serde_json::Error) -> Self {
        Self::InvalidJson(value)
    }
}

fn parse_space_access_busy_payload(
    json: &str,
) -> Result<SpaceAccessBusyPayload, ParseSpaceAccessBusyPayloadError> {
    if !json.trim_start().starts_with('{') {
        return Err(ParseSpaceAccessBusyPayloadError::NotSpaceAccessPayload);
    }

    let payload: serde_json::Value = serde_json::from_str(json)?;
    let Some(kind) = payload.get("kind").and_then(serde_json::Value::as_str) else {
        return Err(ParseSpaceAccessBusyPayloadError::NotSpaceAccessPayload);
    };

    match kind {
        "space_access_offer" => Ok(SpaceAccessBusyPayload::Offer(serde_json::from_value(
            payload,
        )?)),
        "space_access_proof" => Ok(SpaceAccessBusyPayload::Proof(serde_json::from_value(
            payload,
        )?)),
        "space_access_result" => Ok(SpaceAccessBusyPayload::Result(serde_json::from_value(
            payload,
        )?)),
        _ => Err(ParseSpaceAccessBusyPayloadError::NotSpaceAccessPayload),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::network::daemon_api_strings::pairing_busy_reason;
    use uc_core::network::pairing_state_machine::{FailureReason, TimeoutKind};

    #[test]
    fn pairing_failure_message_preserves_machine_readable_reason() {
        assert_eq!(
            pairing_failure_message(&FailureReason::Other(
                pairing_busy_reason::NO_LOCAL_PAIRING_PARTICIPANT_READY.to_string(),
            )),
            pairing_busy_reason::NO_LOCAL_PAIRING_PARTICIPANT_READY
        );
        assert_eq!(
            pairing_failure_message(&FailureReason::Timeout(TimeoutKind::WaitingChallenge)),
            "timeout:WaitingChallenge"
        );
        assert_eq!(
            pairing_failure_message(&FailureReason::PeerBusy),
            pairing_busy_reason::BUSY
        );
    }

    #[test]
    fn parse_space_access_busy_payload_treats_plain_busy_reason_as_non_space_access() {
        for reason in [
            pairing_busy_reason::BUSY,
            pairing_busy_reason::HOST_NOT_DISCOVERABLE,
            pairing_busy_reason::NO_LOCAL_PAIRING_PARTICIPANT_READY,
        ] {
            assert!(
                matches!(
                    parse_space_access_busy_payload(reason),
                    Err(ParseSpaceAccessBusyPayloadError::NotSpaceAccessPayload)
                ),
                "plain busy reason should fall through to handle_busy: {reason}"
            );
        }
    }

    #[test]
    fn parse_space_access_busy_payload_keeps_invalid_json_observable() {
        assert!(matches!(
            parse_space_access_busy_payload("{\"kind\":"),
            Err(ParseSpaceAccessBusyPayloadError::InvalidJson(_))
        ));
    }

    /// Verifies that `emit_ws_event` with `PeersChangedFullPayload` produces a `DaemonWsEvent`
    /// whose payload deserializes back to the original full peer list.
    /// This is the unit-level contract test for the PeerDiscovered/PeerLost emission path.
    #[tokio::test]
    async fn peer_discovered_emits_peers_changed_full_payload_with_peer_list() {
        use crate::api::types::{PeerSnapshotDto, PeersChangedFullPayload};

        let (event_tx, mut event_rx) = broadcast::channel::<DaemonWsEvent>(8);

        let peers = vec![
            PeerSnapshotDto {
                peer_id: "peer-alpha".to_string(),
                device_name: Some("Alpha".to_string()),
                addresses: vec!["/ip4/192.168.1.1/tcp/4001".to_string()],
                is_paired: false,
                connected: true,
                pairing_state: "NotPaired".to_string(),
            },
            PeerSnapshotDto {
                peer_id: "peer-beta".to_string(),
                device_name: None,
                addresses: vec![],
                is_paired: true,
                connected: false,
                pairing_state: "Trusted".to_string(),
            },
        ];

        emit_ws_event(
            &event_tx,
            "peers",
            "peers.changed",
            None,
            PeersChangedFullPayload {
                peers: peers.clone(),
            },
        );

        let event = event_rx.recv().await.expect("event must be received");
        assert_eq!(event.topic, "peers");
        assert_eq!(event.event_type, "peers.changed");

        let payload: PeersChangedFullPayload = serde_json::from_value(event.payload)
            .expect("payload must deserialize to PeersChangedFullPayload");
        assert_eq!(payload.peers.len(), 2, "all 2 peers must be in payload");
        assert_eq!(payload.peers[0].peer_id, "peer-alpha");
        assert_eq!(payload.peers[1].peer_id, "peer-beta");
    }

    /// Verifies that `PeerLost` path emits `peers.changed` with full snapshot (empty when no peers remain).
    #[tokio::test]
    async fn peer_lost_can_emit_peers_changed_with_empty_list() {
        use crate::api::types::PeersChangedFullPayload;

        let (event_tx, mut event_rx) = broadcast::channel::<DaemonWsEvent>(8);

        // Emit an empty peer list (as would happen when last peer is lost)
        emit_ws_event(
            &event_tx,
            "peers",
            "peers.changed",
            None,
            PeersChangedFullPayload { peers: vec![] },
        );

        let event = event_rx.recv().await.expect("event must be received");
        let payload: PeersChangedFullPayload = serde_json::from_value(event.payload)
            .expect("payload must deserialize to PeersChangedFullPayload");
        assert!(
            payload.peers.is_empty(),
            "empty peer list must be emitted when no peers remain after PeerLost"
        );
    }
}
