//! P2P runtime module
//!
//! Manages all P2P-related components including NetworkManager, PairingManager, and P2pSync.

use anyhow::Result;
use chrono::Utc;
use libp2p::identity::Keypair;
use log::error;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, RwLock};

use crate::api::encryption::get_unified_encryption;
use crate::api::event::{
    P2PPairingCompleteEventData, P2PPairingFailedEventData, P2PPairingRequestEventData,
    P2PPeerConnectionEvent, P2PPinReadyEventData,
};
use crate::domain::pairing::PairedPeer;
use crate::infrastructure::p2p::pairing::{PairingCommand, PairingManager};
use crate::infrastructure::p2p::{ConnectedPeer, DiscoveredPeer, NetworkCommand};
use crate::infrastructure::storage::peer_storage::PeerStorage;
use crate::infrastructure::sync::Libp2pSync;

/// P2P runtime - manages all P2P components
pub struct P2PRuntime {
    /// Sender for network commands
    network_cmd_tx: mpsc::Sender<NetworkCommand>,
    /// Sender for pairing commands
    pairing_cmd_tx: mpsc::Sender<PairingCommand>,
    /// P2P sync instance (None if encryption not initialized)
    p2p_sync: Option<Arc<Libp2pSync>>,
    /// Peer storage for managing paired devices
    peer_storage: Arc<PeerStorage>,
    /// Local peer ID
    local_peer_id: String,
    /// Local device name
    device_name: String,
    /// Discovered peers (thread-safe for queries)
    discovered_peers: Arc<RwLock<HashMap<String, DiscoveredPeer>>>,
    /// Connected peers tracking (peer_id -> ConnectedPeer)
    connected_peers: Arc<RwLock<HashMap<String, ConnectedPeer>>>,
}

impl P2PRuntime {
    /// Creates and starts a new P2P runtime that manages network and pairing components, discovered and connected peer tracking, and optional encrypted P2P sync.
    ///
    /// The runtime spawns background tasks for the NetworkManager and PairingManager, initializes peer storage, and begins processing network events that update internal state and emit frontend events. Encryption-based sync (Libp2pSync) is enabled only if unified encryption is available.
    ///
    /// # Returns
    ///
    /// `Ok(Self)` with the initialized P2PRuntime on success, or an error if initialization fails.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tauri::AppHandle;
    /// # async fn example(app_handle: AppHandle) -> anyhow::Result<()> {
    /// let runtime = crate::infrastructure::p2p::P2PRuntime::new("My Device".to_string(), app_handle).await?;
    /// // runtime is now running with background tasks handling network and pairing events
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(device_name: String, app_handle: AppHandle) -> Result<Self> {
        // Create channels for network communication
        let (network_cmd_tx, network_cmd_rx) = mpsc::channel(100);
        let (network_event_tx, mut network_event_rx) = mpsc::channel(100);

        // Generate libp2p keypair
        let local_key = Keypair::generate_ed25519();
        let local_peer_id = libp2p::PeerId::from(local_key.public()).to_string();

        // Get our 6-digit device ID from database
        let device_manager = crate::application::device_service::get_device_manager();
        let our_device_id = match device_manager.get_current_device() {
            Ok(Some(device)) => device.id,
            Ok(None) => {
                log::warn!("No current device found in database, using fallback ID");
                // Fallback: generate a temporary ID (this shouldn't happen in normal operation)
                "000000".to_string()
            }
            Err(e) => {
                log::error!("Failed to get current device: {}", e);
                "000000".to_string()
            }
        };

        // Create channels for pairing manager
        let (pairing_cmd_tx, pairing_cmd_rx) = mpsc::channel(100);

        // Clone app_handle for the event loop
        let app_handle_for_events = app_handle.clone();

        // Spawn NetworkManager task
        let network_event_tx_clone = network_event_tx.clone();
        let device_name_for_network = device_name.clone();
        let our_device_id_for_network = our_device_id.clone();
        tokio::spawn(async move {
            let mut network_manager = crate::infrastructure::p2p::NetworkManager::new(
                network_cmd_rx,
                network_event_tx_clone,
                local_key,
                device_name_for_network,
                our_device_id_for_network,
            )
            .await
            .expect("Failed to create NetworkManager");

            log::info!("Starting P2P NetworkManager");
            network_manager.run().await;
        });

        // Spawn PairingManager actor
        let pairing_network_cmd_tx = network_cmd_tx.clone();
        let pairing_event_tx = network_event_tx.clone();
        let pairing_device_name = device_name.clone();
        let pairing_local_peer_id = local_peer_id.clone();
        tokio::spawn(async move {
            // Get our 6-digit device ID from database
            let device_manager = crate::application::device_service::get_device_manager();
            let our_device_id = match device_manager.get_current_device() {
                Ok(Some(device)) => device.id,
                Ok(None) => {
                    log::warn!("No current device found in database, using fallback ID");
                    // Fallback: generate a temporary ID (this shouldn't happen in normal operation)
                    "000000".to_string()
                }
                Err(e) => {
                    log::error!("Failed to get current device: {}", e);
                    "000000".to_string()
                }
            };

            let pairing_manager = PairingManager::new(
                pairing_network_cmd_tx,
                pairing_event_tx,
                pairing_cmd_rx,
                pairing_device_name,
                our_device_id,
                pairing_local_peer_id,
            );
            pairing_manager.run().await;
        });

        // Create PeerStorage (kept separate for pairing management)
        let peer_storage = Arc::new(PeerStorage::new().expect("Failed to create PeerStorage"));

        // Initialize peer tracking before creating Libp2pSync (avoids circular dependency)
        let discovered_peers = Arc::new(RwLock::new(HashMap::new()));
        let connected_peers = Arc::new(RwLock::new(HashMap::new()));

        // Try to get the unified encryption (optional for first-time users)
        let p2p_sync = match get_unified_encryption().await {
            Some(encryptor) => {
                log::info!("Unified encryptor initialized, P2P sync enabled");
                let device_name_for_sync = device_name.clone();
                Some(Arc::new(Libp2pSync::new(
                    network_cmd_tx.clone(),
                    device_name_for_sync,
                    our_device_id.clone(), // Use 6-digit device ID instead of PeerId
                    encryptor,
                    connected_peers.clone(), // Pass connected_peers reference for broadcasting
                )))
            }
            None => {
                log::info!(
                    "Unified encryptor not initialized, P2P sync disabled. \
                     Please set encryption password to enable P2P features."
                );
                None
            }
        };

        // Spawn event monitoring loop
        let pairing_cmd_tx_clone = pairing_cmd_tx.clone();
        let peer_storage_clone = peer_storage.clone();
        let p2p_sync_for_events = p2p_sync.clone();
        let discovered_peers_clone = discovered_peers.clone();
        let connected_peers_clone = connected_peers.clone();

        tokio::spawn(async move {
            use crate::infrastructure::p2p::NetworkEvent;

            while let Some(event) = network_event_rx.recv().await {
                match event {
                    NetworkEvent::PeerDiscovered(peer) => {
                        let mut peers = discovered_peers_clone.write().await;
                        peers.insert(peer.peer_id.clone(), peer);
                    }
                    NetworkEvent::PeerLost(peer_id) => {
                        let mut peers = discovered_peers_clone.write().await;
                        peers.remove(&peer_id);
                    }
                    NetworkEvent::PairingRequestReceived {
                        session_id,
                        peer_id,
                        request,
                    } => {
                        let device_name = request.device_name.clone();

                        // Send to pairing manager
                        let _ = pairing_cmd_tx_clone
                            .send(PairingCommand::HandleRequest {
                                peer_id: peer_id.clone(),
                                request,
                            })
                            .await;

                        // Emit event to frontend
                        let event_data = P2PPairingRequestEventData {
                            session_id,
                            peer_id,
                            device_name: Some(device_name),
                        };
                        if let Err(e) =
                            app_handle_for_events.emit("p2p-pairing-request", event_data)
                        {
                            log::error!("Failed to emit p2p-pairing-request event: {:?}", e);
                        }
                    }
                    NetworkEvent::PairingPinReady {
                        session_id,
                        pin,
                        peer_device_name,
                        peer_device_id,
                    } => {
                        // Send to pairing manager
                        let _ = pairing_cmd_tx_clone
                            .send(PairingCommand::HandlePinReady {
                                session_id: session_id.clone(),
                                pin: pin.clone(),
                                peer_device_name: peer_device_name.clone(),
                                peer_device_id: peer_device_id.clone(),
                            })
                            .await;

                        // Emit event to frontend
                        let event_data = P2PPinReadyEventData {
                            session_id,
                            pin,
                            peer_device_name,
                        };
                        if let Err(e) = app_handle_for_events.emit("p2p-pin-ready", event_data) {
                            log::error!("Failed to emit p2p-pin-ready event: {:?}", e);
                        }
                    }
                    NetworkEvent::PairingResponseReceived {
                        session_id,
                        peer_id,
                        response,
                    } => {
                        // Send to pairing manager for verification
                        let (tx, _rx) = tokio::sync::oneshot::channel();
                        let _ = pairing_cmd_tx_clone
                            .send(PairingCommand::HandleResponse {
                                session_id: session_id.clone(),
                                response,
                                peer_device_name: peer_id.clone(), // We'll use peer_id as device name placeholder
                                respond_to: tx,
                            })
                            .await;
                    }
                    NetworkEvent::PairingComplete {
                        session_id,
                        peer_id,
                        peer_device_id,
                        peer_device_name,
                    } => {
                        // Save paired peer to independent PeerStorage
                        let paired_peer = PairedPeer {
                            peer_id: peer_id.clone(),
                            device_name: peer_device_name.clone(),
                            shared_secret: vec![], // Empty - no longer using ECDH shared secret
                            paired_at: Utc::now(),
                            last_seen: Some(Utc::now()),
                            last_known_addresses: vec![],
                        };
                        if let Err(e) = peer_storage_clone.save_peer(paired_peer) {
                            log::error!("Failed to save paired peer {}: {}", peer_id, e);
                        } else {
                            log::info!(
                                "Saved paired peer: {} (device_id: {}, device: {})",
                                peer_id,
                                peer_device_id,
                                peer_device_name
                            );
                        }

                        // Also save to DeviceManager (database) for clipboard sync
                        let device_manager = crate::application::device_service::get_device_manager();
                        match device_manager.get(&peer_device_id) {
                            Ok(Some(mut existing_device)) => {
                                // Update existing device with new peer_id and info
                                existing_device.peer_id = Some(peer_id.clone());
                                existing_device.device_name = Some(peer_device_name.clone());
                                existing_device.is_paired = true;
                                existing_device.last_seen = Some(Utc::now().timestamp() as i32);
                                if let Err(e) = device_manager.add(existing_device) {
                                    log::error!("Failed to update paired device {}: {}", peer_device_id, e);
                                }
                            }
                            Ok(None) => {
                                // Create new device entry
                                use crate::domain::device::Device;
                                let mut new_device = Device::new(
                                    peer_device_id.clone(),
                                    None, // IP unknown
                                    None,
                                    None,
                                );
                                new_device.peer_id = Some(peer_id.clone());
                                new_device.device_name = Some(peer_device_name.clone());
                                new_device.is_paired = true;
                                new_device.last_seen = Some(Utc::now().timestamp() as i32);
                                if let Err(e) = device_manager.add(new_device) {
                                    log::error!("Failed to add paired device {}: {}", peer_device_id, e);
                                }
                            }
                            Err(e) => {
                                log::error!("Failed to check device existence: {}", e);
                            }
                        }

                        // Emit event to frontend
                        let event_data = P2PPairingCompleteEventData {
                            session_id,
                            peer_id,
                            device_name: peer_device_name,
                        };
                        if let Err(e) =
                            app_handle_for_events.emit("p2p-pairing-complete", event_data)
                        {
                            log::error!("Failed to emit p2p-pairing-complete event: {:?}", e);
                        }
                    }
                    NetworkEvent::PairingFailed { session_id, error } => {
                        // Emit event to frontend
                        let event_data = P2PPairingFailedEventData { session_id, error };
                        if let Err(e) = app_handle_for_events.emit("p2p-pairing-failed", event_data)
                        {
                            log::error!("Failed to emit p2p-pairing-failed event: {:?}", e);
                        }
                    }
                    NetworkEvent::ClipboardReceived(msg) => {
                        log::info!(
                            "Handling incoming clipboard message from device '{}' (device-id: {})",
                            msg.origin_device_name,
                            msg.origin_device_id
                        );
                        // Forward to P2pSync if initialized
                        if let Some(p2p_sync) = &p2p_sync_for_events {
                            if let Err(e) = p2p_sync.handle_incoming_message(msg).await {
                                log::warn!("Failed to handle incoming clipboard message: {}", e);
                            }
                        } else {
                            log::debug!("Received clipboard message but P2P sync is not enabled (encryption not set up)");
                        }
                    }
                    NetworkEvent::PeerConnected(connected) => {
                        // Update connected peers tracking
                        let mut peers = connected_peers_clone.write().await;
                        peers.insert(connected.peer_id.clone(), connected.clone());
                        log::info!(
                            "Peer connected: {} ({})",
                            connected.peer_id,
                            connected.device_name
                        );

                        // Emit event to frontend
                        let event_data = P2PPeerConnectionEvent {
                            peer_id: connected.peer_id.clone(),
                            device_name: Some(connected.device_name.clone()),
                            connected: true,
                        };
                        if let Err(e) =
                            app_handle_for_events.emit("p2p-peer-connection-changed", event_data)
                        {
                            log::error!(
                                "Failed to emit p2p-peer-connection-changed event: {:?}",
                                e
                            );
                        }
                    }
                    NetworkEvent::PeerDisconnected(peer_id) => {
                        // Get device name from connected peers before removing
                        let device_name = {
                            let mut peers = connected_peers_clone.write().await;
                            peers.remove(&peer_id).map(|p| p.device_name)
                        };
                        log::info!("Peer disconnected: {}", peer_id);

                        // Emit event to frontend
                        let event_data = P2PPeerConnectionEvent {
                            peer_id: peer_id.clone(),
                            device_name,
                            connected: false,
                        };
                        if let Err(e) =
                            app_handle_for_events.emit("p2p-peer-connection-changed", event_data)
                        {
                            log::error!(
                                "Failed to emit p2p-peer-connection-changed event: {:?}",
                                e
                            );
                        }
                    }
                    _ => {}
                }
            }
        });

        Ok(Self {
            network_cmd_tx,
            pairing_cmd_tx,
            p2p_sync,
            peer_storage,
            local_peer_id,
            device_name,
            discovered_peers,
            connected_peers,
        })
    }

    /// Unpair a device
    pub fn unpair_peer(&self, peer_id: &str) -> Result<()> {
        // Use the independent peer_storage instead of p2p_sync.peer_storage()
        self.peer_storage.remove_peer(peer_id)?;
        Ok(())
    }

    /// Get all paired peers
    pub fn get_paired_peers(&self) -> Result<Vec<PairedPeer>> {
        self.peer_storage.get_all_peers()
    }

    /// Get the local peer ID
    pub fn local_peer_id(&self) -> &str {
        &self.local_peer_id
    }

    /// Get the local device name
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Get discovered peers
    pub async fn discovered_peers(&self) -> Vec<DiscoveredPeer> {
        self.discovered_peers
            .read()
            .await
            .values()
            .cloned()
            .collect()
    }

    /// Get connected peers
    pub async fn connected_peers(&self) -> HashMap<String, ConnectedPeer> {
        self.connected_peers.read().await.clone()
    }

    /// Check if a specific peer is connected
    pub async fn is_peer_connected(&self, peer_id: &str) -> bool {
        self.connected_peers.read().await.contains_key(peer_id)
    }

    /// Get the P2P command sender
    pub fn network_cmd_tx(&self) -> mpsc::Sender<NetworkCommand> {
        self.network_cmd_tx.clone()
    }

    /// Get the pairing command sender
    pub fn pairing_cmd_tx(&self) -> mpsc::Sender<PairingCommand> {
        self.pairing_cmd_tx.clone()
    }

    /// Get the P2pSync instance
    ///
    /// Returns None if encryption is not initialized (first-time users)
    pub fn p2p_sync(&self) -> Option<Arc<Libp2pSync>> {
        self.p2p_sync.clone()
    }
}