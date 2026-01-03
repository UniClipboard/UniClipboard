//! Device pairing module for UniClipboard P2P
//!
//! Handles secure device pairing using:
//! - PIN-based verification to prevent MITM attacks
//! - Unified encryption (all devices use the same master key)
//! - Actor pattern for thread safety
//!
//! Note: ECDH key exchange has been removed. All devices now use the same
//! master key derived from the user's encryption password.

use anyhow::{anyhow, Result};
use chrono::Utc;
use rand::Rng;
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};

use super::events::NetworkEvent;
use super::pin_hash;
use super::protocol::{PairingConfirm, PairingRequest, PairingResponse};
use super::swarm::NetworkCommand;

/// PIN length for pairing verification
const PIN_LENGTH: usize = 6;

/// Commands for the pairing actor
pub enum PairingCommand {
    /// Initiate pairing with a peer
    Initiate {
        peer_id: String,
        device_name: String,
        local_peer_id: String,
        respond_to: oneshot::Sender<Result<String>>,
    },
    /// Handle incoming pairing request
    HandleRequest {
        peer_id: String,
        request: PairingRequest,
    },
    /// Handle incoming PIN ready (Challenge)
    HandlePinReady {
        session_id: String,
        pin: String,
        peer_device_name: String,
        peer_device_id: String,
    },
    /// Verify PIN
    VerifyPin {
        session_id: String,
        pin_match: bool,
        respond_to: oneshot::Sender<Result<()>>,
    },
    /// Handle pairing response
    HandleResponse {
        session_id: String,
        response: PairingResponse,
        peer_device_name: String,
        respond_to: oneshot::Sender<Result<bool>>,
    },
    /// Handle pairing confirm
    HandleConfirm {
        session_id: String,
        confirm: PairingConfirm,
        peer_id: String,
        respond_to: oneshot::Sender<Result<()>>,
    },
    /// Reject pairing
    Reject {
        session_id: String,
        peer_id: String,
        respond_to: oneshot::Sender<Result<()>>,
    },
    /// Accept pairing request (responder side)
    AcceptPairing {
        session_id: String,
        respond_to: oneshot::Sender<Result<()>>,
    },
}

/// Active pairing session state
pub struct PairingSession {
    pub session_id: String,
    pub peer_id: String,
    /// Local device name (this device's name)
    pub local_device_name: String,
    /// Peer device name (the other device's name)
    pub peer_device_name: String,
    pub created_at: chrono::DateTime<Utc>,
    pub is_initiator: bool,
    /// Peer's 6-digit device ID (from PairingRequest or PairingChallenge)
    pub peer_device_id: Option<String>,
    /// PIN stored in memory only for hash computation (initiator side)
    /// This is cleared after verification to minimize exposure
    pin: Option<String>,
}

impl PairingSession {
    pub fn new(
        session_id: String,
        peer_id: String,
        local_device_name: String,
        peer_device_name: String,
        is_initiator: bool,
    ) -> Self {
        Self {
            session_id,
            peer_id,
            local_device_name,
            peer_device_name,
            created_at: Utc::now(),
            is_initiator,
            peer_device_id: None,
            pin: None,
        }
    }

    /// Set the PIN for this session (initiator side only)
    pub fn set_pin(&mut self, pin: String) {
        self.pin = Some(pin);
    }

    /// Get the PIN for this session
    pub fn get_pin(&self) -> Option<&str> {
        self.pin.as_deref()
    }

    /// Clear the PIN from memory after use
    pub fn clear_pin(&mut self) {
        self.pin = None;
    }

    /// Check if session is expired (5 minutes)
    pub fn is_expired(&self) -> bool {
        let elapsed = Utc::now() - self.created_at;
        elapsed.num_seconds() > 300 // 5 minutes
    }
}

/// Pairing manager that handles the device pairing flow
pub struct PairingManager {
    /// Active pairing sessions keyed by session_id
    sessions: HashMap<String, PairingSession>,
    /// Channel to send commands to NetworkManager
    network_command_tx: mpsc::Sender<NetworkCommand>,
    /// Channel to emit pairing events
    event_tx: mpsc::Sender<NetworkEvent>,
    /// Channel to receive commands
    command_rx: mpsc::Receiver<PairingCommand>,
    /// local device name
    device_name: String,
    /// Local device's 6-digit ID (from database)
    our_device_id: String,
    /// Local libp2p PeerId (network layer, changes each restart)
    local_peer_id: String,
}

impl PairingManager {
    pub fn new(
        network_command_tx: mpsc::Sender<NetworkCommand>,
        event_tx: mpsc::Sender<NetworkEvent>,
        command_rx: mpsc::Receiver<PairingCommand>,
        device_name: String,
        our_device_id: String,
        local_peer_id: String,
    ) -> Self {
        Self {
            sessions: HashMap::new(),
            network_command_tx,
            event_tx,
            command_rx,
            device_name,
            our_device_id,
            local_peer_id,
        }
    }

    /// Run the pairing manager actor
    pub async fn run(mut self) {
        log::info!("PairingManager actor started");
        loop {
            tokio::select! {
                Some(cmd) = self.command_rx.recv() => {
                    if let Err(e) = self.handle_command(cmd).await {
                        log::error!("Error handling pairing command: {}", e);
                    }
                }
                // Periodic cleanup every minute
                _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {
                    self.cleanup_expired_sessions();
                }
            }
        }
    }

    /// Handle a pairing command
    async fn handle_command(&mut self, cmd: PairingCommand) -> Result<()> {
        match cmd {
            PairingCommand::Initiate {
                peer_id,
                device_name,
                local_peer_id,
                respond_to,
            } => {
                let result = self
                    .initiate_pairing(peer_id, device_name, local_peer_id)
                    .await;
                let _ = respond_to.send(result);
            }
            PairingCommand::VerifyPin {
                session_id,
                pin_match,
                respond_to,
            } => {
                let result = self.verify_pin(&session_id, pin_match).await;
                let _ = respond_to.send(result);
            }
            PairingCommand::HandleRequest { peer_id, request } => {
                // This command is internal/network triggered.
                // We don't have a respond_to channel here usually, passing Result is fine.
                self.handle_pairing_request(peer_id, request).await?;
            }
            PairingCommand::HandlePinReady {
                session_id,
                pin,
                peer_device_name,
                peer_device_id,
            } => {
                self.handle_pin_ready(&session_id, pin, peer_device_name, peer_device_id)
                    .await?;
            }
            PairingCommand::HandleResponse {
                session_id,
                response,
                peer_device_name,
                respond_to,
            } => {
                let result = self
                    .handle_pairing_response(&session_id, response, peer_device_name)
                    .await;
                let _ = respond_to.send(result);
            }
            PairingCommand::HandleConfirm {
                session_id,
                confirm,
                peer_id,
                respond_to,
            } => {
                let result = self
                    .handle_pairing_confirm(&session_id, confirm, peer_id)
                    .await;
                let _ = respond_to.send(result);
            }
            PairingCommand::Reject {
                session_id,
                peer_id,
                respond_to,
            } => {
                let result = self.reject_pairing(&session_id, peer_id).await;
                let _ = respond_to.send(result);
            }
            PairingCommand::AcceptPairing {
                session_id,
                respond_to,
            } => {
                let result = self.accept_pairing(&session_id).await;
                let _ = respond_to.send(result);
            }
        }
        Ok(())
    }

    /// Generate a new session ID
    fn generate_session_id(&self) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time is before UNIX_EPOCH")
            .as_millis();
        let mut rng = rand::rng();
        format!("{}-{}", timestamp, rng.random::<u32>())
    }

    /// Generate a random PIN for pairing verification
    fn generate_pin(&self) -> String {
        let mut rng = rand::rng();
        (0..PIN_LENGTH)
            .map(|_| rng.random_range(0..10).to_string())
            .collect()
    }

    /// Initiate pairing as the initiator
    pub async fn initiate_pairing(
        &mut self,
        peer_id: String,
        device_name: String,
        local_peer_id: String,
    ) -> Result<String> {
        let session_id = self.generate_session_id();

        // Create session with local device name and "Unknown" for peer (will be updated later)
        let session = PairingSession::new(
            session_id.clone(),
            peer_id.clone(),
            device_name.clone(),   // local_device_name (this device)
            "Unknown".to_string(), // peer_device_name (will be updated in handle_pin_ready)
            true,                  // is_initiator
        );

        self.sessions.insert(session_id.clone(), session);

        let request = PairingRequest {
            session_id: session_id.clone(),
            device_name,
            device_id: self.our_device_id.clone(), // Our 6-digit stable ID
            peer_id: local_peer_id,               // Our current libp2p PeerId
        };

        let message = super::protocol::ProtocolMessage::Pairing(
            super::protocol::PairingMessage::Request(request),
        );

        let message_bytes = message
            .to_bytes()
            .map_err(|e| anyhow!("Failed to serialize pairing request: {}", e))?;

        self.network_command_tx
            .send(NetworkCommand::SendPairingRequest {
                peer_id,
                message: message_bytes,
            })
            .await
            .map_err(|e| anyhow!("Failed to send pairing request: {}", e))?;

        Ok(session_id)
    }

    /// Handle incoming pairing request (responder side)
    /// Only creates session and notifies frontend - PIN generation is deferred until accept_pairing
    pub async fn handle_pairing_request(
        &mut self,
        peer_id: String,
        request: PairingRequest,
    ) -> Result<()> {
        log::info!(
            "Received pairing request from peer {} (device: {}, device_id: {})",
            peer_id,
            request.device_name,
            request.device_id
        );

        // Create session with local device name and peer device name from request
        let mut session = PairingSession::new(
            request.session_id.clone(),
            peer_id.clone(),
            self.device_name.clone(), // local_device_name (responder's device)
            request.device_name.clone(), // peer_device_name (initiator's device)
            false,                    // is_initiator
        );

        // Store the peer's 6-digit device ID from the request
        session.peer_device_id = Some(request.device_id.clone());

        self.sessions.insert(request.session_id.clone(), session);

        Ok(())
    }

    /// Accept pairing request (responder side) - generates PIN and sends challenge
    pub async fn accept_pairing(&mut self, session_id: &str) -> Result<()> {
        // Clone needed values before borrowing to avoid borrow conflicts
        let (peer_id, peer_device_name, peer_device_id) = {
            let session = self
                .sessions
                .get_mut(session_id)
                .ok_or_else(|| anyhow!("Session not found"))?;

            if session.is_expired() {
                return Err(anyhow!("Pairing session expired"));
            }

            (
                session.peer_id.clone(),
                session.peer_device_name.clone(),
                session.peer_device_id.clone().ok_or_else(|| anyhow!("Peer device ID not found in session"))?,
            )
        };

        // Generate PIN for user verification
        let pin = self.generate_pin();

        log::info!(
            "Accepted pairing request {}, generated PIN: {}",
            session_id,
            pin
        );

        // Store PIN for later hash verification when initiator responds
        {
            let session = self
                .sessions
                .get_mut(session_id)
                .ok_or_else(|| anyhow!("Session not found"))?;
            session.set_pin(pin.clone());
        }

        // Send p2p-pin-ready event to frontend (responder needs to see the PIN)
        let _ = self
            .event_tx
            .send(NetworkEvent::PairingPinReady {
                session_id: session_id.to_string(),
                pin: pin.clone(),
                peer_device_name: peer_device_name.clone(),
                peer_device_id: peer_device_id.clone(),
            })
            .await;

        // Send pairing challenge with PIN to the initiator
        self.network_command_tx
            .send(NetworkCommand::SendPairingChallenge {
                peer_id,
                session_id: session_id.to_string(),
                pin,
                device_name: self.device_name.clone(),
                device_id: self.our_device_id.clone(),
                local_peer_id: self.local_peer_id.clone(),
            })
            .await
            .map_err(|e| anyhow!("Failed to send pairing challenge: {}", e))?;

        Ok(())
    }

    /// Handle incoming PIN ready / Challenge (initiator side)
    pub async fn handle_pin_ready(
        &mut self,
        session_id: &str,
        pin: String,
        peer_device_name: String,
        peer_device_id: String,
    ) -> Result<()> {
        log::info!(
            "Received PIN challenge for session {}, peer device: {}, peer_device_id: {}",
            session_id,
            peer_device_name,
            peer_device_id
        );

        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| anyhow!("Session not found"))?;

        // Update peer device name when we receive the challenge
        session.peer_device_name = peer_device_name;
        // Store peer's 6-digit device ID
        session.peer_device_id = Some(peer_device_id);
        // Store PIN for later hash computation when user confirms
        session.set_pin(pin);

        Ok(())
    }

    /// Handle PIN verification result (initiator side)
    /// Sends PairingResponse with Argon2id-derived PIN hash to responder
    pub async fn verify_pin(&mut self, session_id: &str, pin_match: bool) -> Result<()> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| anyhow!("Session not found"))?;

        if session.is_expired() {
            return Err(anyhow!("Pairing session expired"));
        }

        let peer_id = session.peer_id.clone();

        if !pin_match {
            log::warn!("PIN verification failed for session {}", session_id);
            // Send rejection response
            self.network_command_tx
                .send(NetworkCommand::SendPairingResponse {
                    peer_id,
                    session_id: session_id.to_string(),
                    pin_hash: Vec::new(), // Empty hash for rejection
                    accepted: false,
                })
                .await
                .map_err(|e| anyhow!("Failed to send pairing response: {}", e))?;

            self.sessions.remove(session_id);
            return Ok(());
        }

        // Get the stored PIN and compute Argon2id hash
        let pin = session
            .get_pin()
            .ok_or_else(|| anyhow!("PIN not found in session"))?;

        log::info!(
            "Computing Argon2id hash for PIN verification, session: {}, peer: {}",
            session_id,
            peer_id
        );

        let pin_hash =
            pin_hash::hash_pin(pin).map_err(|e| anyhow!("Failed to compute PIN hash: {}", e))?;

        // Clear PIN from memory after use
        session.clear_pin();

        log::info!(
            "Sending PairingResponse with Argon2id hash for session {}, peer: {}",
            session_id,
            peer_id
        );

        // Send PairingResponse with PIN hash
        self.network_command_tx
            .send(NetworkCommand::SendPairingResponse {
                peer_id,
                session_id: session_id.to_string(),
                pin_hash,
                accepted: true,
            })
            .await
            .map_err(|e| anyhow!("Failed to send pairing response: {}", e))?;

        // Note: We don't remove the session yet; we wait for PairingConfirm from responder
        Ok(())
    }

    /// Handle pairing response (responder side)
    /// Verifies the Argon2id PIN hash and sends PairingConfirm
    pub async fn handle_pairing_response(
        &mut self,
        session_id: &str,
        response: PairingResponse,
        _peer_device_name: String,
    ) -> Result<bool> {
        // Clone needed values before borrowing to avoid borrow conflicts
        let (peer_id, local_device_name, is_expired, pin) = {
            let session = self
                .sessions
                .get(session_id)
                .ok_or_else(|| anyhow!("Session not found"))?;

            (
                session.peer_id.clone(),
                session.local_device_name.clone(),
                session.is_expired(),
                session.get_pin().map(|p| p.to_string()),
            )
        };

        if is_expired {
            return Err(anyhow!("Pairing session expired"));
        }

        if !response.accepted {
            log::warn!(
                "Pairing rejected by initiator for session {}, peer: {}",
                session_id,
                peer_id
            );
            self.sessions.remove(session_id);

            // Send rejection confirm
            let _ = self
                .network_command_tx
                .send(NetworkCommand::SendPairingConfirm {
                    peer_id,
                    session_id: session_id.to_string(),
                    success: false,
                    device_name: local_device_name,
                    device_id: String::new(), // Empty for rejection
                })
                .await;

            return Ok(false);
        }

        // Get the stored PIN for verification
        let pin = pin.ok_or_else(|| anyhow!("PIN not found in session"))?;

        log::info!(
            "Verifying Argon2id PIN hash for session {}, peer: {}",
            session_id,
            peer_id
        );

        // Verify the PIN hash using constant-time comparison
        let verified = pin_hash::verify_pin(&pin, &response.pin_hash)
            .map_err(|e| anyhow!("Failed to verify PIN hash: {}", e))?;

        if !verified {
            log::warn!(
                "PIN hash verification failed for session {}, peer: {}",
                session_id,
                peer_id
            );

            self.sessions.remove(session_id);

            // Send failure confirm
            let _ = self
                .network_command_tx
                .send(NetworkCommand::SendPairingConfirm {
                    peer_id,
                    session_id: session_id.to_string(),
                    success: false,
                    device_name: local_device_name,
                    device_id: String::new(), // Empty for failure
                })
                .await;

            return Ok(false);
        }

        log::info!(
            "PIN hash verified successfully for session {}, peer: {}",
            session_id,
            peer_id
        );

        log::info!(
            "Sending PairingConfirm with success=true for session {}",
            session_id
        );

        // Send success confirm (no shared_secret needed anymore)
        self.network_command_tx
            .send(NetworkCommand::SendPairingConfirm {
                peer_id,
                session_id: session_id.to_string(),
                success: true,
                device_name: local_device_name,
                device_id: self.our_device_id.clone(), // Our 6-digit device ID
            })
            .await
            .map_err(|e| anyhow!("Failed to send pairing confirm: {}", e))?;

        self.sessions.remove(session_id);
        Ok(true)
    }

    /// Handle pairing confirmation
    /// Called by both initiator (receiving confirmation from responder)
    /// and responder (when pairing is complete)
    pub async fn handle_pairing_confirm(
        &mut self,
        session_id: &str,
        confirm: PairingConfirm,
        peer_id: String,
    ) -> Result<()> {
        let session = self
            .sessions
            .get(session_id)
            .ok_or_else(|| anyhow!("Session not found"))?;

        if session.is_expired() {
            return Err(anyhow!("Pairing session expired"));
        }

        if confirm.success {
            log::info!(
                "Pairing completed successfully for session {}, peer: {}",
                session_id,
                peer_id
            );
        } else {
            log::warn!(
                "Pairing failed for session {}: {}",
                session_id,
                confirm.error.as_deref().unwrap_or("unknown error")
            );
        }

        self.sessions.remove(session_id);
        Ok(())
    }

    /// Reject an incoming pairing request
    pub async fn reject_pairing(&mut self, session_id: &str, peer_id: String) -> Result<()> {
        log::info!(
            "Rejected pairing request {} from peer {}",
            session_id,
            peer_id
        );

        self.network_command_tx
            .send(NetworkCommand::RejectPairing {
                peer_id,
                session_id: session_id.to_string(),
            })
            .await
            .map_err(|e| anyhow!("Failed to send pairing rejection: {}", e))?;

        self.sessions.remove(session_id);
        Ok(())
    }

    /// Clean up expired sessions
    pub fn cleanup_expired_sessions(&mut self) {
        self.sessions.retain(|_, session| !session.is_expired());
    }

    /// Get a session by ID
    pub fn get_session(&self, session_id: &str) -> Option<&PairingSession> {
        self.sessions.get(session_id)
    }

    /// Get a mutable session by ID
    pub fn get_session_mut(&mut self, session_id: &str) -> Option<&mut PairingSession> {
        self.sessions.get_mut(session_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_pin() {
        let manager = create_test_manager();
        let pin = manager.generate_pin();
        assert_eq!(pin.len(), PIN_LENGTH);
        assert!(pin.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_session_expiration() {
        let session = PairingSession::new(
            "test-session".to_string(),
            "peer-123".to_string(),
            "Test Device".to_string(),
            "Peer Device".to_string(),
            true,
        );
        assert!(!session.is_expired());
    }

    fn create_test_manager() -> PairingManager {
        let (cmd_tx, _) = mpsc::channel(100);
        let (event_tx, _) = mpsc::channel(100);
        let (_, command_rx) = mpsc::channel(100);
        PairingManager::new(
            cmd_tx,
            event_tx,
            command_rx,
            "Test Device".to_string(),
            "test-device-id".to_string(),
            "test-peer-id".to_string(),
        )
    }
}

impl std::fmt::Debug for PairingCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HandlePinReady {
                session_id,
                peer_device_name,
                ..
            } => f
                .debug_struct("HandlePinReady")
                .field("session_id", session_id)
                .field("peer_device_name", peer_device_name)
                .field("pin", &"[REDACTED]")
                .finish(),
            Self::HandleResponse {
                session_id,
                peer_device_name,
                ..
            } => f
                .debug_struct("HandleResponse")
                .field("session_id", session_id)
                .field("peer_device_name", peer_device_name)
                .field("response", &"[REDACTED]")
                .finish(),
            Self::HandleConfirm {
                session_id,
                peer_id,
                ..
            } => f
                .debug_struct("HandleConfirm")
                .field("session_id", session_id)
                .field("peer_id", peer_id)
                .field("confirm", &"[REDACTED]")
                .finish(),
            Self::HandleRequest { peer_id, .. } => f
                .debug_struct("HandleRequest")
                .field("peer_id", peer_id)
                .field("request", &"[REDACTED]")
                .finish(),
            _ => {
                // For non-sensitive variants, use discriminant
                write!(f, "{:?}", std::mem::discriminant(self))
            }
        }
    }
}
