//! Pairing session lifecycle management
//!
//! Manages creation, lookup, cleanup, and timeout of pairing sessions.
//! Owns the sessions map (`HashMap<SessionId, PairingSessionContext>`) and
//! peer information map.

use anyhow::{Context as AnyhowContext, Result};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug_span, Instrument};

use uc_core::network::pairing_state_machine::{
    PairingEvent, PairingPolicy, PairingRole, PairingStateMachine, SessionId, TimeoutKind,
};

use super::orchestrator::PairingConfig;

/// Pairing session context, holding a state machine and its metadata.
pub(crate) struct PairingSessionContext {
    /// State machine for this session
    pub(crate) state_machine: PairingStateMachine,
    /// When the session was created
    pub(crate) created_at: DateTime<Utc>,
    /// Timer abort handles for this session
    pub(crate) timers: Mutex<HashMap<TimeoutKind, tokio::task::AbortHandle>>,
}

/// Peer information for a pairing session.
#[derive(Debug, Clone)]
pub struct PairingPeerInfo {
    pub peer_id: String,
    pub device_name: Option<String>,
}

/// Local device identity information.
#[derive(Clone)]
pub(crate) struct LocalDeviceInfo {
    /// Device name
    pub(crate) device_name: String,
    /// Device ID
    pub(crate) device_id: String,
    /// Identity public key
    pub(crate) identity_pubkey: Vec<u8>,
    /// Local PeerID
    pub(crate) peer_id: String,
}

/// Manages pairing session lifecycle: creation, lookup, cleanup, timeout.
#[derive(Clone)]
pub(crate) struct PairingSessionManager {
    /// Configuration
    config: PairingConfig,
    /// Active pairing sessions (session_id -> state machine context)
    sessions: Arc<RwLock<HashMap<SessionId, PairingSessionContext>>>,
    /// Peer info for each session
    session_peers: Arc<RwLock<HashMap<SessionId, PairingPeerInfo>>>,
    /// Local device identity
    local_identity: LocalDeviceInfo,
}

impl PairingSessionManager {
    /// Create a new session manager.
    pub(crate) fn new(config: PairingConfig, local_identity: LocalDeviceInfo) -> Self {
        Self {
            config,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_peers: Arc::new(RwLock::new(HashMap::new())),
            local_identity,
        }
    }

    /// Get a reference to the sessions map (for action executor).
    pub(crate) fn sessions(&self) -> &Arc<RwLock<HashMap<SessionId, PairingSessionContext>>> {
        &self.sessions
    }

    /// Get a reference to the session peers map (for action executor).
    pub(crate) fn session_peers(&self) -> &Arc<RwLock<HashMap<SessionId, PairingPeerInfo>>> {
        &self.session_peers
    }

    /// Build a PairingPolicy from the current config.
    pub(crate) fn build_policy(&self) -> PairingPolicy {
        PairingPolicy {
            step_timeout_secs: self.config.step_timeout_secs,
            user_verification_timeout_secs: self.config.user_verification_timeout_secs,
            max_retries: self.config.max_retries,
            protocol_version: self.config.protocol_version.clone(),
        }
    }

    /// Create a new state machine with local identity and policy.
    pub(crate) fn new_state_machine(&self) -> PairingStateMachine {
        PairingStateMachine::new_with_local_identity_and_policy(
            self.local_identity.device_name.clone(),
            self.local_identity.device_id.clone(),
            self.local_identity.identity_pubkey.clone(),
            self.build_policy(),
        )
    }

    /// Get the local peer ID.
    pub(crate) fn local_peer_id(&self) -> &str {
        &self.local_identity.peer_id
    }

    /// Record peer info for a session.
    pub(crate) async fn record_session_peer(
        &self,
        session_id: &str,
        peer_id: String,
        device_name: Option<String>,
    ) {
        let mut peers = self.session_peers.write().await;
        let entry = peers
            .entry(session_id.to_string())
            .or_insert_with(|| PairingPeerInfo {
                peer_id: peer_id.clone(),
                device_name: None,
            });
        entry.peer_id = peer_id;
        if device_name.is_some() {
            entry.device_name = device_name;
        }
    }

    /// Get peer info for a session.
    pub(crate) async fn get_session_peer(&self, session_id: &str) -> Option<PairingPeerInfo> {
        let peers = self.session_peers.read().await;
        peers.get(session_id).cloned()
    }

    /// Get the role for a session.
    pub(crate) async fn get_session_role(&self, session_id: &str) -> Option<PairingRole> {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .and_then(|ctx| ctx.state_machine.role())
    }

    /// Return whether a session exists in the manager.
    pub(crate) async fn has_session(&self, session_id: &str) -> bool {
        self.sessions.read().await.contains_key(session_id)
    }

    /// Return whether a session exists **and** is still in a non-terminal state
    /// (i.e. not `Failed`, `Cancelled`, or `Paired`).
    pub(crate) async fn has_active_session(&self, session_id: &str) -> bool {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .map(|ctx| !ctx.state_machine.state().is_terminal())
            .unwrap_or(false)
    }

    /// Insert a session context into the sessions map.
    pub(crate) async fn insert_session(
        &self,
        session_id: SessionId,
        context: PairingSessionContext,
    ) {
        self.sessions.write().await.insert(session_id, context);
    }

    /// Process a state machine event for an existing session.
    /// Returns the resulting actions. The session must already exist.
    pub(crate) async fn process_event(
        &self,
        session_id: &str,
        event: PairingEvent,
    ) -> Result<Vec<uc_core::network::pairing_state_machine::PairingAction>> {
        let mut sessions = self.sessions.write().await;
        let context = sessions.get_mut(session_id).context("Session not found")?;
        let (_state, actions) = context.state_machine.handle_event(event, Utc::now());
        Ok(actions)
    }

    /// Process a state machine event for a session that may not exist.
    /// Returns empty actions if session not found.
    pub(crate) async fn process_event_if_exists(
        &self,
        session_id: &str,
        event: PairingEvent,
    ) -> Vec<uc_core::network::pairing_state_machine::PairingAction> {
        let mut sessions = self.sessions.write().await;
        if let Some(context) = sessions.get_mut(session_id) {
            let (_state, actions) = context.state_machine.handle_event(event, Utc::now());
            actions
        } else {
            vec![]
        }
    }

    /// Cleanup expired sessions.
    pub(crate) async fn cleanup_expired_sessions(&self) {
        let span = debug_span!("pairing.cleanup_expired_sessions");
        async {
            let mut sessions = self.sessions.write().await;
            let now = Utc::now();
            let timeout = Duration::seconds(self.config.session_timeout_secs);
            let expired_ids: Vec<SessionId> = sessions
                .iter()
                .filter_map(|(session_id, context)| {
                    let expired = now.signed_duration_since(context.created_at).num_seconds()
                        >= timeout.num_seconds();
                    if expired {
                        Some(session_id.clone())
                    } else {
                        None
                    }
                })
                .collect();

            let mut expired_contexts = Vec::with_capacity(expired_ids.len());
            for session_id in &expired_ids {
                if let Some(context) = sessions.remove(session_id) {
                    expired_contexts.push((session_id.clone(), context));
                }
            }
            drop(sessions);

            if !expired_ids.is_empty() {
                tracing::info!(
                    count = expired_ids.len(),
                    "Cleaned up expired pairing sessions"
                );
                let mut peers = self.session_peers.write().await;
                for session_id in &expired_ids {
                    peers.remove(session_id);
                }
            }

            for (_session_id, context) in expired_contexts {
                let mut timers = context.timers.lock().await;
                for (_kind, handle) in timers.drain() {
                    handle.abort();
                }
            }
        }
        .instrument(span)
        .await
    }
}
