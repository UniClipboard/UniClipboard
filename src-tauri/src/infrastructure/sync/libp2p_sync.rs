//! P2P clipboard synchronization using libp2p
//!
//! This module implements the `RemoteClipboardSync` trait using libp2p for
//! peer-to-peer clipboard sharing over local networks.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::Utc;
use futures::future;
use log::{debug, error, info, warn};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

use crate::domain::transfer_message::ClipboardTransferMessage;
use crate::infrastructure::p2p::{ClipboardMessage, ConnectedPeer, NetworkCommand};
use crate::infrastructure::security::unified_encryption::UnifiedEncryption;
use crate::interface::RemoteClipboardSync;

/// P2P-based clipboard synchronization
#[derive(Clone)]
pub struct Libp2pSync {
    /// Sender for network commands
    network_command_tx: tokio::sync::mpsc::Sender<NetworkCommand>,
    /// Device name for this peer
    device_name: String,
    /// Device ID (PeerId)
    device_id: String,
    /// Unified encryption for encrypting/decrypting clipboard content
    encryptor: Arc<UnifiedEncryption>,
    /// Sender for clipboard pull requests (to application)
    clipboard_tx: tokio::sync::mpsc::Sender<ClipboardTransferMessage>,
    /// Receiver for clipboard pull (used internally)
    clipboard_rx: Arc<Mutex<tokio::sync::mpsc::Receiver<ClipboardTransferMessage>>>,
    /// Whether sync is running
    running: Arc<Mutex<bool>>,
    /// Connected peers reference for broadcasting clipboard content
    connected_peers: Arc<RwLock<HashMap<String, ConnectedPeer>>>,
}

impl Libp2pSync {
    /// Creates a new Libp2pSync instance wired to the provided network sender and encryption provider.
    ///
    /// The returned instance is initialized with an internal clipboard channel (capacity 100)
    /// and a stopped (`running = false`) state.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::sync::Arc;
    /// use std::collections::HashMap;
    /// use tokio::sync::{mpsc, RwLock};
    ///
    /// // Placeholder types; replace with real implementations from the crate.
    /// // let network_command_tx: mpsc::Sender<NetworkCommand> = ...;
    /// // let encryptor: Arc<UnifiedEncryption> = Arc::new(...);
    /// // let connected_peers: Arc<RwLock<HashMap<String, ConnectedPeer>>> = ...;
    ///
    /// let (network_command_tx, _rx) = mpsc::channel::<NetworkCommand>(8);
    /// let device_name = "my-device".to_string();
    /// let device_id = "peer-id-123".to_string();
    /// let encryptor = Arc::new(unimplemented!()); // replace with `UnifiedEncryption` instance
    /// let connected_peers = Arc::new(RwLock::new(HashMap::new()));
    ///
    /// let sync = Libp2pSync::new(network_command_tx, device_name, device_id, encryptor, connected_peers);
    /// ```
    pub fn new(
        network_command_tx: tokio::sync::mpsc::Sender<NetworkCommand>,
        device_name: String,
        device_id: String,
        encryptor: Arc<UnifiedEncryption>,
        connected_peers: Arc<RwLock<HashMap<String, ConnectedPeer>>>,
    ) -> Self {
        let (clipboard_tx, clipboard_rx) = tokio::sync::mpsc::channel(100);

        Self {
            network_command_tx,
            device_name,
            device_id,
            encryptor,
            clipboard_tx,
            clipboard_rx: Arc::new(Mutex::new(clipboard_rx)),
            running: Arc::new(Mutex::new(false)),
            connected_peers,
        }
    }

    /// Get a channel sender for receiving clipboard events
    /// The NetworkManager/Swarm loop will send received messages here
    pub fn get_clipboard_tx(&self) -> tokio::sync::mpsc::Sender<ClipboardTransferMessage> {
        self.clipboard_tx.clone()
    }

    /// Compute content hash for deduplication
    fn compute_content_hash(content: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content);
        format!("{:x}", hasher.finalize())
    }

    /// Encrypt content using the unified encryptor
    /// All paired devices share the same master key, so we encrypt once and broadcast to all
    async fn encrypt_content(&self, content: &[u8]) -> Result<Vec<u8>> {
        // Check if encryptor is ready
        if !self.encryptor.is_ready().await {
            return Err(anyhow!(
                "Unified encryptor not initialized. Please set encryption password first."
            ));
        }

        self.encryptor
            .encrypt(content)
            .await
            .map_err(|e| anyhow!("Failed to encrypt content: {}", e))
    }

    /// Decrypt content using the unified encryptor
    /// All devices use the same master key, so we don't need to know who sent it
    async fn decrypt_content(&self, content: &[u8]) -> Result<Vec<u8>> {
        // Check if encryptor is ready
        if !self.encryptor.is_ready().await {
            return Err(anyhow!(
                "Unified encryptor not initialized. Please set encryption password first."
            ));
        }

        self.encryptor
            .decrypt(content)
            .await
            .map_err(|e| anyhow!("Failed to decrypt content: {}", e))
    }

    /// Handle an incoming encrypted clipboard message from the network
    /// This is called when NetworkManager receives a message
    pub async fn handle_incoming_message(&self, msg: ClipboardMessage) -> Result<()> {
        info!(
            "Decrypting clipboard message from '{}' (encrypted size: {} bytes)",
            msg.origin_device_name,
            msg.encrypted_content.len()
        );
        // Decrypt using unified encryptor (no longer need origin_device_id)
        let decrypted = self
            .decrypt_content(&msg.encrypted_content)
            .await
            .map_err(|e| {
                anyhow!(
                    "Failed to decrypt clipboard message from {}: {}",
                    msg.origin_device_id,
                    e
                )
            })?;

        info!("Clipboard message decrypted successfully");
        let transfer_msg: ClipboardTransferMessage = serde_json::from_slice(&decrypted)
            .map_err(|e| anyhow!("Failed to deserialize clipboard message: {}", e))?;

        info!(
            "Deserialized clipboard transfer message: type={:?}, size={} bytes",
            transfer_msg.metadata.get_content_type(),
            transfer_msg.metadata.get_size()
        );

        self.clipboard_tx
            .send(transfer_msg)
            .await
            .map_err(|e| anyhow!("Failed to send to clipboard channel: {}", e))?;

        info!("Sent clipboard message to internal channel");
        Ok(())
    }
}

#[async_trait]
impl RemoteClipboardSync for Libp2pSync {
    /// Push clipboard content to all connected peers via BlobStream
    async fn push(&self, message: ClipboardTransferMessage) -> Result<()> {
        let content_bytes = serde_json::to_vec(&message)
            .map_err(|e| anyhow!("Failed to serialize clipboard message: {}", e))?;

        // Encrypt once using the unified encryptor (all devices share the same key)
        let encrypted_content = self.encrypt_content(&content_bytes).await.map_err(|e| {
            error!("Failed to encrypt clipboard content: {}", e);
            anyhow!("Failed to encrypt clipboard content: {}", e)
        })?;

        // Get all connected peers
        let peers = self.connected_peers.read().await;

        if peers.is_empty() {
            debug!("No connected peers, skipping clipboard broadcast");
            return Ok(());
        }

        // Concurrently send to all connected peers using BlobStream
        let mut send_tasks = Vec::new();
        for (peer_id, _peer_info) in peers.iter() {
            let cmd_tx = self.network_command_tx.clone();
            let peer_id = peer_id.clone();
            let encrypted_data = encrypted_content.clone();

            send_tasks.push(tokio::spawn(async move {
                let (respond_to, rx) = tokio::sync::oneshot::channel();
                let peer_id_for_log = peer_id.clone();

                if let Err(e) = cmd_tx.send(NetworkCommand::SendClipboard {
                    peer_id,
                    data: encrypted_data,
                    respond_to,
                }).await {
                    return Err(format!("Failed to send SendClipboard command: {}", e));
                }

                match rx.await {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(e)) => Err(format!("SendClipboard failed for peer {}: {}", peer_id_for_log, e)),
                    Err(e) => Err(format!("SendClipboard response channel closed for peer {}: {}", peer_id_for_log, e)),
                }
            }));
        }

        // Wait for all send tasks to complete
        let results: Vec<_> = future::join_all(send_tasks).await;

        // Count successes and failures
        let success_count = results.iter().filter(|r| r.is_ok() && r.as_ref().unwrap().is_ok()).count();
        let fail_count = results.len() - success_count;

        if fail_count > 0 {
            warn!("Clipboard broadcast partially failed: {} succeeded, {} failed",
                  success_count, fail_count);
        } else {
            debug!("Clipboard message sent successfully to {} peers", success_count);
        }

        // At least one success is considered success
        if success_count == 0 {
            Err(anyhow!("Failed to broadcast clipboard to any peer (0/{} succeeded)", results.len()))
        } else {
            Ok(())
        }
    }

    /// Pull clipboard content from P2P network
    async fn pull(&self, _timeout: Option<Duration>) -> Result<ClipboardTransferMessage> {
        info!("Pulling clipboard message from P2P internal channel");
        // We just wait for messages on the channel
        let mut rx = self.clipboard_rx.lock().await;
        let msg = rx.recv()
            .await
            .ok_or_else(|| anyhow!("Clipboard channel closed"))?;
        info!("Received clipboard message from internal channel: type={:?}, size={} bytes",
            msg.metadata.get_content_type(),
            msg.metadata.get_size()
        );
        Ok(msg)
    }

    /// Sync is a no-op for P2P (continuous sync via gossipsub)
    async fn sync(&self) -> Result<()> {
        Ok(())
    }

    /// Start P2P synchronization
    async fn start(&self) -> Result<()> {
        let mut running = self.running.lock().await;
        if *running {
            return Ok(());
        }
        *running = true;
        info!("Starting P2P clipboard synchronization");
        Ok(())
    }

    /// Stop P2P synchronization
    async fn stop(&self) -> Result<()> {
        let mut running = self.running.lock().await;
        *running = false;
        info!("Stopped P2P clipboard synchronization");
        Ok(())
    }

    /// Pause P2P synchronization
    async fn pause(&self) -> Result<()> {
        self.stop().await
    }

    /// Resume P2P synchronization
    async fn resume(&self) -> Result<()> {
        self.start().await
    }
}