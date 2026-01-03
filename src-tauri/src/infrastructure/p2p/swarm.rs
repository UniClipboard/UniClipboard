use chrono::Utc;
use futures::{AsyncReadExt, AsyncWriteExt, StreamExt};
use libp2p::{
    identify, mdns, ping,
    request_response::{self, ResponseChannel},
    swarm::{Stream, StreamProtocol, SwarmEvent},
    Multiaddr, PeerId, Swarm,
};
use libp2p_stream::Control;
use log::{debug, error, info, warn};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use super::behaviour::UniClipboardBehaviour;
use super::blob::{
    receiver::BlobReceiver, sender::BlobSender, Frame, FrameHandleResult, BLOBSTREAM_PROTOCOL,
};
use super::events::{ConnectedPeer, DiscoveredPeer, NetworkEvent, NetworkStatus};
use super::protocol::{ClipboardMessage, PairingMessage, ProtocolMessage};
use super::transport;
use super::{ReqPairingRequest, ReqPairingResponse};

/// Holds a pending pairing response channel with metadata
/// This is only available to the receiver of a request-response call
struct PendingPairingResponse {
    peer_id: PeerId,
    channel: ResponseChannel<ReqPairingResponse>,
    timestamp: Instant,
}

/// Tracks pending pairing requests waiting for Challenge response
/// Used by the initiator when waiting for the responder's Challenge
struct PendingChallengeRequest {
    peer_id: PeerId,
    session_id: String,
    timestamp: Instant,
}

/// Connection quality metrics for a peer
#[derive(Debug, Clone)]
struct ConnectionQuality {
    peer_id: PeerId,
    /// Last successful ping duration
    last_ping_latency: Option<Duration>,
    /// Number of successful pings
    successful_pings: u64,
    /// Number of failed pings
    failed_pings: u64,
    /// Connection uptime (since last established)
    connected_since: Option<Instant>,
    /// Last activity timestamp
    last_activity: Instant,
}

impl ConnectionQuality {
    fn new(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            last_ping_latency: None,
            successful_pings: 0,
            failed_pings: 0,
            connected_since: None,
            last_activity: Instant::now(),
        }
    }

    /// Calculate connection success rate (0.0 to 1.0)
    fn success_rate(&self) -> f64 {
        let total = self.successful_pings + self.failed_pings;
        if total == 0 {
            1.0 // No pings yet, assume healthy
        } else {
            self.successful_pings as f64 / total as f64
        }
    }

    /// Check if connection is healthy (success rate > 80%)
    fn is_healthy(&self) -> bool {
        self.success_rate() > 0.8
    }
}

/// Commands sent to NetworkManager
pub enum NetworkCommand {
    StartListening,
    StopListening,
    SendPairingRequest {
        peer_id: String,
        message: Vec<u8>,
    },
    /// Send a pairing challenge (PIN) as a response to an incoming pairing request.
    SendPairingChallenge {
        peer_id: String,
        session_id: String,
        pin: String,
        device_name: String,
        device_id: String,
        local_peer_id: String,
    },
    /// Send a pairing response with PIN hash (after initiator verifies PIN)
    SendPairingResponse {
        peer_id: String,
        session_id: String,
        pin_hash: Vec<u8>, // Argon2id-encoded {version||salt||hash}
        accepted: bool,
    },
    /// Reject a pairing request
    RejectPairing {
        peer_id: String,
        session_id: String,
    },
    /// Send pairing confirmation (after PIN verification on initiator side)
    SendPairingConfirm {
        peer_id: String,
        session_id: String,
        success: bool,
        device_name: String,
        device_id: String,
    },
    /// Send clipboard data to a specific peer via BlobStream.
    ///
    /// This command initiates a chunked stream transfer of clipboard content,
    /// which is more efficient than broadcasting for large data.
    SendClipboard {
        peer_id: String,
        data: Vec<u8>,
        respond_to: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    #[deprecated(
        note = "BroadcastClipboard is deprecated in v2.0.0. Use SendClipboard with BlobStream instead."
    )]
    BroadcastClipboard {
        message: ClipboardMessage,
    },
    #[allow(dead_code)]
    GetPeers,
    /// Force reconnection to all discovered peers (used after app resume from background).
    /// Also attempts to reconnect to paired peers using their last-known addresses
    /// as fallback when mDNS hasn't rediscovered them yet.
    ReconnectPeers {
        /// Paired peers with their last-known addresses (from vault).
        /// Used as fallback when peer is not in discovered_peers.
        /// Format: Vec<(peer_id, Vec<address>)>
        paired_peer_addresses: Vec<(String, Vec<String>)>,
    },
    /// Re-emit PeerDiscovered event for a specific peer
    #[allow(dead_code)]
    RefreshPeer {
        peer_id: String,
    },
    /// Broadcast device name to all peers on the network.
    AnnounceDeviceName {
        device_name: String,
    },
}

/// Network manager that handles libp2p swarm event loop
pub struct NetworkManager {
    swarm: Swarm<UniClipboardBehaviour>,
    command_rx: mpsc::Receiver<NetworkCommand>,
    event_tx: mpsc::Sender<NetworkEvent>,
    discovered_peers: HashMap<PeerId, DiscoveredPeer>,
    connected_peers: HashMap<PeerId, ConnectedPeer>,
    /// Maps session_id -> response channel
    /// Only populated for the receiver of a request-response call
    /// Using session_id as key provides stability across network reconnects
    pending_responses: HashMap<String, PendingPairingResponse>,
    /// Maps session_id -> pending challenge request info
    /// Used by the initiator to track outgoing PairingRequest that expects Challenge response
    pending_challenges: HashMap<String, PendingChallengeRequest>,
    /// Peers confirmed ready for broadcast (subscribed to clipboard topic).
    ready_peers: HashMap<PeerId, Instant>,
    /// Active BlobStream send sessions
    /// Maps session_id -> (peer_id, BlobSender)
    /// Each session represents an ongoing chunked data transfer to a peer
    active_blob_sends: HashMap<u32, (PeerId, BlobSender)>,
    /// Active BlobStream receive sessions
    /// Maps session_id -> BlobReceiver
    /// Each receiver handles incoming chunked data from a peer
    active_blob_receives: HashMap<u32, BlobReceiver>,
    /// Session ID counter for BlobStream transfers
    /// Incremented for each new BlobStream session
    next_session_id: u32,
    /// Stream control for BlobStream
    /// Used to open outgoing streams and accept incoming streams
    stream_control: Control,
    /// Current device name (updated when settings change)
    device_name: String,
    /// Our 6-digit device ID (from database)
    our_device_id: String,
    /// Connection statistics for diagnostics
    connections_established: u64,
    connections_failed_outgoing: u64,
    connections_failed_incoming: u64,
    /// Peers currently being dialed (prevent duplicate dials)
    dialing_peers: HashSet<PeerId>,
    /// Timestamp of last dial attempt per peer (for rate limiting)
    last_dial_attempt: HashMap<PeerId, Instant>,
    /// Peers with pending reconnection attempts
    reconnecting_peers: HashSet<PeerId>,
    /// Reconnection attempt count per peer (for exponential backoff)
    reconnect_attempts: HashMap<PeerId, u32>,
    /// Connection quality metrics per peer
    connection_quality: HashMap<PeerId, ConnectionQuality>,
}

impl NetworkManager {
    pub async fn new(
        command_rx: mpsc::Receiver<NetworkCommand>,
        event_tx: mpsc::Sender<NetworkEvent>,
        local_key: libp2p::identity::Keypair,
        device_name: String,
        our_device_id: String,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let local_peer_id = PeerId::from(local_key.public());
        info!("Local peer ID: {}", local_peer_id);

        // Create swarm with transport configuration module
        // Supports both TCP (fallback) and QUIC for better performance
        let swarm = libp2p::SwarmBuilder::with_existing_identity(local_key.clone())
            .with_tokio()
            // NOTE: 暂时禁用 tcp，专注 quic 协议调试
            // .with_tcp(
            //     transport::build_tcp_config(),
            //     transport::build_noise_config,
            //     transport::build_yamux_config,
            // )?
            .with_quic_config(transport::configure_quic)
            .with_behaviour(|_key| {
                UniClipboardBehaviour::new(local_peer_id, &local_key, &device_name, &our_device_id)
                    .expect("Failed to create behaviour")
            })?
            .with_swarm_config(|cfg| {
                // Set connection idle timeout to 300s (5 minutes)
                // This MUST be >= QUIC's max_idle_timeout to prevent Swarm from closing
                // connections before the QUIC layer. Clipboard is a low-frequency application,
                // so users may not copy anything for several minutes.
                cfg.with_idle_connection_timeout(Duration::from_secs(300))
            })
            .build();

        // ⚠️ Critical: Must get control AFTER swarm is built
        let stream_control = swarm.behaviour().stream.new_control();

        Ok(Self {
            swarm,
            command_rx,
            event_tx,
            discovered_peers: HashMap::new(),
            connected_peers: HashMap::new(),
            pending_responses: HashMap::new(),
            pending_challenges: HashMap::new(),
            ready_peers: HashMap::new(),
            active_blob_sends: HashMap::new(),
            active_blob_receives: HashMap::new(),
            next_session_id: 0,
            stream_control,
            device_name,
            our_device_id,
            connections_established: 0,
            connections_failed_outgoing: 0,
            connections_failed_incoming: 0,
            dialing_peers: HashSet::new(),
            last_dial_attempt: HashMap::new(),
            reconnecting_peers: HashSet::new(),
            reconnect_attempts: HashMap::new(),
            connection_quality: HashMap::new(),
        })
    }

    /// Clean up expired pending response channels (older than 5 minutes)
    /// This should be called periodically to prevent memory leaks from abandoned sessions
    fn cleanup_expired_channels(&mut self) {
        let timeout = Duration::from_secs(300);
        let now = Instant::now();

        self.pending_responses.retain(|session_id, pending| {
            if now.duration_since(pending.timestamp) > timeout {
                debug!(
                    "Removing expired pending response for session {} (peer: {})",
                    session_id, pending.peer_id
                );
                false
            } else {
                true
            }
        });

        self.pending_challenges.retain(|session_id, pending| {
            if now.duration_since(pending.timestamp) > timeout {
                debug!(
                    "Removing expired pending challenge for session {} (peer: {})",
                    session_id, pending.peer_id
                );
                false
            } else {
                true
            }
        });
    }

    /// Calculate exponential backoff delay for reconnection attempts
    ///
    /// Formula: min(2^attempt * BASE_DELAY, MAX_DELAY)
    /// - attempt 0: 5s
    /// - attempt 1: 10s
    /// - attempt 2: 20s
    /// - attempt 3: 40s
    /// - attempt 4+: 60s (capped)
    fn calculate_backoff_delay(attempt: u32) -> Duration {
        const BASE_DELAY: Duration = Duration::from_secs(5);
        const MAX_DELAY: Duration = Duration::from_secs(60);

        let delay = BASE_DELAY * 2u32.pow(attempt.min(4)); // Cap at 2^4 = 16
        std::cmp::min(delay, MAX_DELAY)
    }

    /// Handle incoming BlobStream connection
    ///
    /// This is a static method that runs in a separate task for each incoming stream.
    /// It reads length-prefixed frames and processes them with a BlobReceiver.
    ///
    /// # Arguments
    ///
    /// * `peer` - The peer ID of the sender
    /// * `stream` - The incoming stream
    /// * `event_tx` - Channel to send network events to
    async fn handle_incoming_blob(
        peer: PeerId,
        mut stream: Stream,
        event_tx: mpsc::Sender<NetworkEvent>,
    ) -> Result<(), String> {
        info!("[BlobRecv] Incoming BlobStream from {}", peer);

        // Create a receiver with a placeholder session_id (will be set from first frame)
        let mut receiver = BlobReceiver::new(0);
        info!("[BlobRecv] BlobReceiver created with session_id=0");

        loop {
            info!("[BlobRecv] Waiting for frame...");
            let frame_bytes = read_len_prefixed(&mut stream).await.map_err(|e| {
                error!("[BlobRecv] read_len_prefixed failed: {}", e);
                e.to_string()
            })?;
            info!("[BlobRecv] Read {} bytes", frame_bytes.len());

            let frame = Frame::from_bytes(&frame_bytes).map_err(|e| {
                error!("[BlobRecv] Frame::from_bytes failed: {}", e);
                e.to_string()
            })?;

            let session_id = frame.session_id();

            // Update receiver session_id on first frame
            if receiver.session_id() == 0 && session_id != 0 {
                receiver = BlobReceiver::new(session_id);
                debug!(
                    "BlobStream receiver initialized with session_id={}",
                    session_id
                );
            }

            match receiver.handle_frame(frame).map_err(|e| e.to_string())? {
                FrameHandleResult::MetadataReceived => {
                    info!(
                        "BlobStream metadata received: session_id={}, peer={}",
                        session_id, peer
                    );
                    // TODO: Emit progress event to UI if needed
                }
                FrameHandleResult::DataReceived { complete } => {
                    if complete {
                        debug!(
                            "BlobStream all data received: session_id={}, peer={}",
                            session_id, peer
                        );
                    }
                }
                FrameHandleResult::TransferComplete => {
                    info!(
                        "BlobStream transfer complete: session_id={}, peer={}",
                        session_id, peer
                    );

                    // Assemble the received data
                    let data = receiver.assemble().map_err(|e| e.to_string())?;

                    // Create a ClipboardMessage from the received data
                    // Note: BlobStream transfers raw encrypted clipboard content
                    let clipboard_message = super::protocol::ClipboardMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        content_hash: blake3::hash(&data).to_hex().to_string(),
                        encrypted_content: data,
                        timestamp: Utc::now(),
                        origin_device_id: String::new(), // Not available at this layer
                        origin_device_name: String::new(), // Not available at this layer
                    };

                    // Send clipboard received event
                    let _ = event_tx
                        .send(NetworkEvent::ClipboardReceived(clipboard_message))
                        .await;

                    break;
                }
                FrameHandleResult::InvalidSession => {
                    warn!(
                        "BlobStream invalid session: expected {}, got {}",
                        receiver.session_id(),
                        session_id
                    );
                    // Continue anyway, might be a different session
                }
                FrameHandleResult::UnknownFrame => {
                    warn!("BlobStream unknown frame type from {}", peer);
                }
                FrameHandleResult::HashMismatch => {
                    warn!("BlobStream hash mismatch for chunk from {}", peer);
                    // Continue anyway, let higher layers handle corruption
                }
            }
        }

        Ok(())
    }

    pub async fn run(&mut self) {
        // Start accepting incoming BlobStream connections
        // This must be polled continuously, otherwise streams will be dropped
        let mut incoming = self
            .stream_control
            .clone()
            .accept(StreamProtocol::new(BLOBSTREAM_PROTOCOL))
            .expect("blob protocol already registered?");

        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            while let Some((peer, stream)) = incoming.next().await {
                let event_tx = event_tx.clone();
                // Spawn each stream in its own task to avoid blocking the accept loop
                tokio::spawn(async move {
                    if let Err(e) = Self::handle_incoming_blob(peer, stream, event_tx).await {
                        warn!("blob recv error from {}: {}", peer, e);
                    }
                });
            }
        });

        // Get physical LAN IP for QUIC listening
        // QUIC cannot bind to 0.0.0.0 reliably when multiple interfaces exist (e.g., VPN/tunnel)
        // Must explicitly bind to a physical LAN IP to avoid handshake issues
        let local_ip = crate::utils::helpers::get_physical_lan_ip().unwrap_or_else(|| {
            warn!("No physical LAN IP found, falling back to 0.0.0.0 (QUIC may fail)");
            "0.0.0.0".to_string()
        });
        info!("P2P NetworkManager binding to {}:31773 (QUIC)", local_ip);

        // Start listening on TCP
        // Use a fixed port (31773) so cached addresses remain valid across app restarts.
        // let tcp_addr: Multiaddr = format!("/ip4/{}/tcp/31773", local_ip)
        //     .parse()
        //     .expect("Invalid TCP listen address");
        // if let Err(e) = self.swarm.listen_on(tcp_addr) {
        //     error!("Failed to start listening on TCP: {}", e);
        //     let _ = self
        //         .event_tx
        //         .send(NetworkEvent::StatusChanged(NetworkStatus::Error(
        //             e.to_string(),
        //         )))
        //         .await;
        //     return;
        // }

        // Start listening on QUIC (UDP)
        // Same port number for consistency, using QUIC protocol
        let quic_addr: Multiaddr = format!("/ip4/{}/udp/31773/quic-v1", local_ip)
            .parse()
            .expect("Invalid QUIC listen address");
        if let Err(e) = self.swarm.listen_on(quic_addr) {
            warn!("Failed to start listening on QUIC: {}", e);
            // QUIC is optional, continue with TCP only
        } else {
            info!("QUIC listener started successfully");
        }

        let _ = self
            .event_tx
            .send(NetworkEvent::StatusChanged(NetworkStatus::Connecting))
            .await;

        // Create interval for periodic cleanup (every 60 seconds)
        let mut cleanup_interval = tokio::time::interval(Duration::from_secs(60));

        loop {
            tokio::select! {
                // Handle swarm events
                event = self.swarm.select_next_some() => {
                    self.handle_swarm_event(event).await;
                }

                // Handle commands
                Some(command) = self.command_rx.recv() => {
                    self.handle_command(command).await;
                }

                // Periodic cleanup of expired channels
                _ = cleanup_interval.tick() => {
                    self.cleanup_expired_channels();
                }
            }
        }
    }

    async fn handle_swarm_event(
        &mut self,
        event: SwarmEvent<super::behaviour::UniClipboardBehaviourEvent>,
    ) {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on {}", address);
                let _ = self
                    .event_tx
                    .send(NetworkEvent::StatusChanged(NetworkStatus::Connected))
                    .await;
            }

            SwarmEvent::Behaviour(super::behaviour::UniClipboardBehaviourEvent::Mdns(event)) => {
                match event {
                    mdns::Event::Discovered(peers) => {
                        for (peer_id, addr) in peers {
                            debug!("mDNS discovered: {} at {}", peer_id, addr);

                            // Check if already connected
                            if self.connected_peers.contains_key(&peer_id) {
                                debug!("Skipping dial: already connected to {}", peer_id);
                                // Still track discovered peer for UI updates
                                let discovered = DiscoveredPeer {
                                    peer_id: peer_id.to_string(),
                                    device_name: None,
                                    device_id: None,
                                    addresses: vec![addr.to_string()],
                                    discovered_at: Utc::now(),
                                    is_paired: false,
                                };
                                self.discovered_peers.insert(peer_id, discovered.clone());
                                let _ = self
                                    .event_tx
                                    .send(NetworkEvent::PeerDiscovered(discovered))
                                    .await;
                                continue;
                            }

                            // Check if already dialing
                            if self.dialing_peers.contains(&peer_id) {
                                debug!("Skipping dial: already dialing {}", peer_id);
                                continue;
                            }

                            // Check if recent dial attempt (avoid rapid retry)
                            if let Some(last_attempt) = self.last_dial_attempt.get(&peer_id) {
                                if last_attempt.elapsed() < Duration::from_secs(30) {
                                    debug!("Skipping dial: recent attempt to {}", peer_id);
                                    continue;
                                }
                            }

                            // Mark as dialing
                            self.dialing_peers.insert(peer_id);
                            self.last_dial_attempt.insert(peer_id, Instant::now());

                            if let Err(e) = self.swarm.dial(addr.clone()) {
                                warn!("Failed to dial {}: {}", peer_id, e);
                                self.dialing_peers.remove(&peer_id);
                            }

                            // Track discovered peer
                            let discovered = DiscoveredPeer {
                                peer_id: peer_id.to_string(),
                                device_name: None,
                                device_id: None, // Will be filled in by Identify event
                                addresses: vec![addr.to_string()],
                                discovered_at: Utc::now(),
                                is_paired: false,
                            };

                            self.discovered_peers.insert(peer_id, discovered.clone());
                            let _ = self
                                .event_tx
                                .send(NetworkEvent::PeerDiscovered(discovered))
                                .await;
                        }
                    }
                    mdns::Event::Expired(peers) => {
                        for (peer_id, _) in peers {
                            debug!("mDNS peer expired: {}", peer_id);
                            self.discovered_peers.remove(&peer_id);
                            let _ = self
                                .event_tx
                                .send(NetworkEvent::PeerLost(peer_id.to_string()))
                                .await;
                        }
                    }
                }
            }

            SwarmEvent::Behaviour(
                super::behaviour::UniClipboardBehaviourEvent::RequestResponse(event),
            ) => {
                match event {
                    request_response::Event::Message {
                        peer,
                        connection_id: _connection_id,
                        message,
                    } => {
                        match message {
                            request_response::Message::Request {
                                request, channel, ..
                            } => {
                                debug!("Received pairing message from {}", peer);

                                if let Ok(protocol_msg) =
                                    ProtocolMessage::from_bytes(&request.message)
                                {
                                    match protocol_msg {
                                        ProtocolMessage::Pairing(PairingMessage::Request(req)) => {
                                            // Remove any existing pending response for this session
                                            self.pending_responses.remove(&req.session_id);

                                            // Store with session_id as key for stability across reconnects
                                            self.pending_responses.insert(
                                                req.session_id.clone(),
                                                PendingPairingResponse {
                                                    peer_id: peer,
                                                    channel,
                                                    timestamp: Instant::now(),
                                                },
                                            );

                                            let _ = self
                                                .event_tx
                                                .send(NetworkEvent::PairingRequestReceived {
                                                    session_id: req.session_id.clone(),
                                                    peer_id: peer.to_string(),
                                                    request: req,
                                                })
                                                .await;
                                        }
                                        // Handle Challenge message sent as a NEW request (not response)
                                        // This allows the responder to send Challenge to the initiator
                                        ProtocolMessage::Pairing(PairingMessage::Challenge(
                                            challenge,
                                        )) => {
                                            // Store the channel for responding with ChallengeResponse
                                            self.pending_responses.insert(
                                                challenge.session_id.clone(),
                                                PendingPairingResponse {
                                                    peer_id: peer,
                                                    channel,
                                                    timestamp: Instant::now(),
                                                },
                                            );

                                            let _ = self
                                                .event_tx
                                                .send(NetworkEvent::PairingPinReady {
                                                    session_id: challenge.session_id,
                                                    pin: challenge.pin,
                                                    peer_device_name: challenge.device_name,
                                                    peer_device_id: challenge.device_id,
                                                })
                                                .await;
                                        }
                                        // Handle Response message sent as a NEW request
                                        // This allows the initiator to send Response (PIN hash) to the responder
                                        ProtocolMessage::Pairing(PairingMessage::Response(
                                            response,
                                        )) => {
                                            // Store the channel for responding with Confirm
                                            self.pending_responses.insert(
                                                response.session_id.clone(),
                                                PendingPairingResponse {
                                                    peer_id: peer,
                                                    channel,
                                                    timestamp: Instant::now(),
                                                },
                                            );

                                            let _ = self
                                                .event_tx
                                                .send(NetworkEvent::PairingResponseReceived {
                                                    session_id: response.session_id.clone(),
                                                    peer_id: peer.to_string(),
                                                    response,
                                                })
                                                .await;
                                        }
                                        ProtocolMessage::Pairing(PairingMessage::Confirm(
                                            confirm,
                                        )) => {
                                            // Get initiator's device name from the confirm message
                                            let initiator_device_name =
                                                confirm.sender_device_name.clone();

                                            if confirm.success {
                                                // Send ACK with responder's own device name and device ID
                                                let ack = super::protocol::PairingConfirm {
                                                    session_id: confirm.session_id.clone(),
                                                    success: true,
                                                    error: None,
                                                    sender_device_name: self.device_name.clone(),
                                                    device_id: self.our_device_id.clone(),
                                                };
                                                let ack_msg = ProtocolMessage::Pairing(
                                                    PairingMessage::Confirm(ack),
                                                );
                                                if let Ok(message) = ack_msg.to_bytes() {
                                                    let response = ReqPairingResponse { message };
                                                    let _ = self
                                                        .swarm
                                                        .behaviour_mut()
                                                        .request_response
                                                        .send_response(channel, response);
                                                }

                                                let _ = self
                                                    .event_tx
                                                    .send(NetworkEvent::PairingComplete {
                                                        session_id: confirm.session_id,
                                                        peer_id: peer.to_string(),
                                                        peer_device_id: confirm.device_id,
                                                        peer_device_name: initiator_device_name,
                                                    })
                                                    .await;
                                            } else {
                                                let ack = super::protocol::PairingConfirm {
                                                    session_id: confirm.session_id.clone(),
                                                    success: false,
                                                    error: confirm.error.clone(),
                                                    sender_device_name: self.device_name.clone(),
                                                    device_id: String::new(), // Empty for failure case
                                                };
                                                let ack_msg = ProtocolMessage::Pairing(
                                                    PairingMessage::Confirm(ack),
                                                );
                                                if let Ok(message) = ack_msg.to_bytes() {
                                                    let response = ReqPairingResponse { message };
                                                    let _ = self
                                                        .swarm
                                                        .behaviour_mut()
                                                        .request_response
                                                        .send_response(channel, response);
                                                }

                                                let _ = self
                                                    .event_tx
                                                    .send(NetworkEvent::PairingFailed {
                                                        session_id: confirm.session_id,
                                                        error: confirm.error.unwrap_or_else(|| {
                                                            "Pairing cancelled".to_string()
                                                        }),
                                                    })
                                                    .await;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            request_response::Message::Response { response, .. } => {
                                // Note: Challenge and Response are now sent as requests, not responses
                                // This branch handles the Confirm ACK (response to Confirm request)
                                if let Ok(ProtocolMessage::Pairing(pairing_msg)) =
                                    ProtocolMessage::from_bytes(&response.message)
                                {
                                    match pairing_msg {
                                        PairingMessage::Confirm(confirm) => {
                                            if confirm.success {
                                                // Get responder's device info from the ACK
                                                let responder_device_name =
                                                    confirm.sender_device_name.clone();
                                                let responder_device_id = confirm.device_id.clone();

                                                let _ = self
                                                    .event_tx
                                                    .send(NetworkEvent::PairingComplete {
                                                        session_id: confirm.session_id,
                                                        peer_id: peer.to_string(),
                                                        peer_device_id: responder_device_id,
                                                        peer_device_name: responder_device_name,
                                                    })
                                                    .await;
                                            } else {
                                                let _ = self
                                                    .event_tx
                                                    .send(NetworkEvent::PairingFailed {
                                                        session_id: confirm.session_id,
                                                        error: confirm.error.unwrap_or_else(|| {
                                                            "Unknown error".to_string()
                                                        }),
                                                    })
                                                    .await;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            SwarmEvent::Behaviour(super::behaviour::UniClipboardBehaviourEvent::Identify(
                identify::Event::Received { peer_id, info, .. },
            )) => {
                debug!("Identified peer {}: {}", peer_id, info.agent_version);

                // Parse device_id and device_name from agent_version
                // New format: "uniclipboard/<version>/<device_id>/<device_name>"
                // Old format: "uniclipboard/<version>/<device_name>" (for backward compatibility)
                let (device_id, device_name) = if info.agent_version.starts_with("uniclipboard/") {
                    let parts: Vec<&str> = info.agent_version.splitn(4, '/').collect();
                    if parts.len() >= 4 {
                        // New format with device_id
                        (Some(parts[2].to_string()), Some(parts[3].to_string()))
                    } else if parts.len() >= 3 {
                        // Old format without device_id - treat parts[2] as device_name
                        (None, Some(parts[2].to_string()))
                    } else {
                        (None, Some(info.agent_version.clone()))
                    }
                } else {
                    (None, Some(info.agent_version.clone()))
                };

                if let Some(discovered) = self.discovered_peers.get_mut(&peer_id) {
                    let old_name = discovered.device_name.clone();
                    discovered.device_name = device_name.clone();
                    discovered.device_id = device_id.clone(); // Store the 6-digit device ID

                    if old_name != discovered.device_name {
                        let _ = self
                            .event_tx
                            .send(NetworkEvent::PeerDiscovered(discovered.clone()))
                            .await;
                    }
                }

                // Update device in database when we get both device_id and peer_id
                if let (Some(device_id), Some(device_name)) = (device_id, device_name) {
                    let device_manager = crate::application::device_service::get_device_manager();
                    if let Err(e) = device_manager.update_by_peer_id(
                        &peer_id.to_string(),
                        &device_id,
                        Some(device_name),
                    ) {
                        warn!(
                            "Failed to update device {} with peer_id {}: {}",
                            device_id, peer_id, e
                        );
                    }
                }
            }

            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                // Clean up dialing state
                self.dialing_peers.remove(&peer_id);
                self.last_dial_attempt.remove(&peer_id);
                self.reconnecting_peers.remove(&peer_id); // Clear reconnection state
                self.reconnect_attempts.remove(&peer_id); // Reset reconnection counter

                self.connections_established += 1;
                info!("╔══════════════════════════════════════════╗");
                info!("║     Connection Established               ║");
                info!("╚══════════════════════════════════════════╝");
                info!("Peer ID: {}", peer_id);
                info!("Endpoint: {:?}", endpoint);
                info!(
                    "Total established: {} | Active connections: {}",
                    self.connections_established,
                    self.connected_peers.len()
                );
                debug!("Connection established with {}", peer_id);

                // Note: Incoming BlobStream connections are handled by the accept loop in run()
                // via libp2p-stream::Control::accept(), not here.

                let connected = ConnectedPeer {
                    peer_id: peer_id.to_string(),
                    device_name: self
                        .discovered_peers
                        .get(&peer_id)
                        .and_then(|p| p.device_name.clone())
                        .unwrap_or_else(|| "Unknown".to_string()),
                    connected_at: Utc::now(),
                };
                self.connected_peers.insert(peer_id, connected.clone());
                let _ = self
                    .event_tx
                    .send(NetworkEvent::PeerConnected(connected))
                    .await;

                // Initialize connection quality metrics
                let quality = self.connection_quality
                    .entry(peer_id)
                    .or_insert_with(|| ConnectionQuality::new(peer_id));
                quality.connected_since = Some(Instant::now());
                quality.successful_pings = 0;
                quality.failed_pings = 0;
            }

            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                // Clean up dialing state
                self.dialing_peers.remove(&peer_id);

                debug!("Connection closed with {}", peer_id);

                // Note: active_blob_receives now tracks by session_id, not peer_id.
                // Incoming streams are handled in self-contained tasks that complete independently.
                // If we want to track receives per peer, we would need to store (session_id, peer_id) pairs.

                // Clean up any active BlobStream send sessions to this peer
                let sessions_to_remove: Vec<u32> = self
                    .active_blob_sends
                    .iter()
                    .filter(|(_, (p, _))| *p == peer_id)
                    .map(|(session_id, _)| *session_id)
                    .collect();

                for session_id in sessions_to_remove {
                    if self.active_blob_sends.remove(&session_id).is_some() {
                        debug!(
                            "Removed BlobStream send session {} for peer {} on connection close",
                            session_id, peer_id
                        );
                    }
                }

                if self.ready_peers.remove(&peer_id).is_some() {
                    let _ = self
                        .event_tx
                        .send(NetworkEvent::PeerNotReady {
                            peer_id: peer_id.to_string(),
                        })
                        .await;
                }

                self.connected_peers.remove(&peer_id);
                let _ = self
                    .event_tx
                    .send(NetworkEvent::PeerDisconnected(peer_id.to_string()))
                    .await;

                // Auto-reconnect logic for paired peers
                if let Some(discovered) = self.discovered_peers.get(&peer_id) {
                    // Only reconnect to paired devices (has device_id)
                    if discovered.device_id.is_some() {
                        info!("Initiating auto-reconnect to paired peer {}", peer_id);

                        // Check if we have cached addresses
                        if let Some(addr_str) = discovered.addresses.first() {
                            if let Ok(addr) = addr_str.parse::<Multiaddr>() {
                                // Check rate limiting (avoid rapid retry within 10s)
                                if let Some(last_attempt) = self.last_dial_attempt.get(&peer_id) {
                                    if last_attempt.elapsed() < Duration::from_secs(10) {
                                        debug!("Skipping reconnect: recent attempt to {}", peer_id);
                                        return;
                                    }
                                }

                                // Check if already dialing
                                if !self.dialing_peers.contains(&peer_id) {
                                    // Calculate backoff delay
                                    let attempt = self.reconnect_attempts.entry(peer_id).or_insert(0);
                                    let backoff_delay = Self::calculate_backoff_delay(*attempt);

                                    info!(
                                        "Scheduling reconnection to {} in {:?} (attempt {})",
                                        peer_id, backoff_delay, attempt
                                    );

                                    // Mark as dialing and increment attempt
                                    self.dialing_peers.insert(peer_id);
                                    self.last_dial_attempt.insert(peer_id, Instant::now());
                                    *attempt += 1;

                                    // Attempt reconnection immediately if no backoff
                                    if backoff_delay.is_zero() {
                                        if let Err(e) = self.swarm.dial(addr) {
                                            warn!("Failed to dial {} for reconnection: {}", peer_id, e);
                                            self.dialing_peers.remove(&peer_id);
                                        } else {
                                            info!("Reconnection attempt initiated to {}", peer_id);
                                        }
                                    } else {
                                        // Spawn delayed reconnection task
                                        tokio::spawn(async move {
                                            tokio::time::sleep(backoff_delay).await;
                                            debug!("Backoff delay elapsed for {}, ready for reconnection", peer_id);
                                            // Note: mDNS or ReconnectPeers command will handle actual reconnection
                                        });
                                    }
                                }
                            }
                        } else {
                            // No cached addresses - mDNS will rediscover the peer
                            debug!("No cached addresses for {}, waiting for mDNS rediscovery", peer_id);
                        }
                    } else {
                        debug!("Not reconnecting to unpaired peer {}", peer_id);
                    }
                }
            }

            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                self.connections_failed_outgoing += 1;
                error!("╔══════════════════════════════════════════╗");
                error!("║     Outgoing Connection Failed           ║");
                error!("╚══════════════════════════════════════════╝");
                error!("Peer ID: {:?}", peer_id);
                error!("Error Source: {:?}", std::error::Error::source(&error));
                error!("Error Message: {}", error);
                error!(
                    "Total failed outgoing: {}",
                    self.connections_failed_outgoing
                );
                warn!("Outgoing connection error to {:?}: {}", peer_id, error);
            }

            SwarmEvent::IncomingConnectionError {
                local_addr,
                send_back_addr,
                error,
                ..
            } => {
                self.connections_failed_incoming += 1;
                error!("╔══════════════════════════════════════════╗");
                error!("║     Incoming Connection Failed           ║");
                error!("╚══════════════════════════════════════════╝");
                error!("Local addr: {}", local_addr);
                error!("Send back addr: {}", send_back_addr);
                error!("Error Source: {:?}", std::error::Error::source(&error));
                error!("Error Message: {}", error);
                error!(
                    "Total failed incoming: {}",
                    self.connections_failed_incoming
                );
                warn!("Incoming connection error: {}", error);
            }

            SwarmEvent::Dialing { peer_id, .. } => {
                debug!("Dialing peer: {:?}", peer_id);
            }

            SwarmEvent::Behaviour(super::behaviour::UniClipboardBehaviourEvent::Ping(event)) => {
                // Ping events are used for keepalive and connection health monitoring
                // Since libp2p 0.56 ping::Event has ambiguous enum variants,
                // we use a simple debug log for now and match on specific patterns

                // Try to extract peer and result from the event
                // The ping event structure varies by libp2p version, so we use a safe approach
                let peer_id = format!("{:?}", event);
                debug!("Ping event: {}", peer_id);

                // For detailed quality monitoring, we would need to match the specific
                // ping::Event variants, but due to API ambiguity we use diagnostic logging
                // The QUIC layer and Swarm idle timeout will handle connection stability
            }

            _ => {}
        }
    }

    async fn handle_command(&mut self, command: NetworkCommand) {
        match command {
            NetworkCommand::SendClipboard {
                peer_id,
                data,
                respond_to,
            } => {
                info!(
                    "[SendClipboard] Received command for peer {}, data size: {} bytes",
                    peer_id,
                    data.len()
                );

                // Parse peer_id
                let peer = match peer_id.parse::<PeerId>() {
                    Ok(p) => p,
                    Err(e) => {
                        warn!("[SendClipboard] Invalid peer_id '{}': {}", peer_id, e);
                        let _ = self
                            .event_tx
                            .send(NetworkEvent::Error(format!(
                                "Failed to send clipboard: invalid peer_id '{}': {}",
                                peer_id, e
                            )))
                            .await;
                        let _ = respond_to.send(Err(format!("Invalid peer_id: {}", e)));
                        return;
                    }
                };

                info!("[SendClipboard] Peer ID parsed: {}", peer);

                // Allocate session_id
                let session_id = self.next_session_id;
                self.next_session_id = self.next_session_id.wrapping_add(1);
                info!("[SendClipboard] Allocated session_id: {}", session_id);

                // Clone control and event_tx for the spawned task
                let mut control = self.stream_control.clone();
                let _event_tx = self.event_tx.clone();

                // Spawn the send task to avoid blocking the swarm event loop
                tokio::spawn(async move {
                    info!(
                        "[SendClipboard] Task started for peer {}, session_id={}",
                        peer, session_id
                    );

                    let result: Result<(), String> = async {
                        info!("[SendClipboard] Opening stream to peer {}...", peer);
                        // Open stream to peer
                        let mut stream = control
                            .open_stream(peer, StreamProtocol::new(BLOBSTREAM_PROTOCOL))
                            .await
                            .map_err(|e| {
                                error!("[SendClipboard] open_stream failed: {}", e);
                                format!("open_stream failed: {}", e)
                            })?;

                        info!("[SendClipboard] Stream opened successfully");

                        // Create sender
                        let mut sender = BlobSender::new(data, session_id);
                        let total_frames = sender.total_frames();

                        info!(
                            "[SendClipboard] BlobSender created: session_id={}, total_frames={}, data_size={}",
                            session_id, total_frames, sender.metadata().size
                        );

                        // Send metadata frame
                        let frame = sender
                            .make_metadata_frame()
                            .map_err(|e| {
                                error!("[SendClipboard] make_metadata_frame failed: {}", e);
                                format!("make_metadata_frame failed: {}", e)
                            })?;
                        let frame_bytes = frame
                            .to_bytes()
                            .map_err(|e| {
                                error!("[SendClipboard] frame serialization failed: {}", e);
                                format!("frame serialization failed: {}", e)
                            })?;

                        info!("[SendClipboard] Metadata frame size: {} bytes", frame_bytes.len());
                        write_len_prefixed(&mut stream, &frame_bytes).await.map_err(|e| {
                            error!("[SendClipboard] write_len_prefixed (metadata) failed: {}", e);
                            format!("write_len_prefixed failed: {}", e)
                        })?;
                        info!("[SendClipboard] Metadata frame sent");

                        // Send data frames
                        let mut frame_count = 0;
                        while let Some(frame) = sender
                            .next_frame()
                            .map_err(|e| {
                                error!("[SendClipboard] next_frame failed: {}", e);
                                format!("next_frame failed: {}", e)
                            })?
                        {
                            let frame_bytes = frame.to_bytes().map_err(|e| {
                                error!("[SendClipboard] frame serialization failed: {}", e);
                                format!("frame serialization failed: {}", e)
                            })?;
                            frame_count += 1;
                            info!("[SendClipboard] Sending data frame {}: {} bytes", frame_count, frame_bytes.len());
                            write_len_prefixed(&mut stream, &frame_bytes).await.map_err(|e| {
                                error!("[SendClipboard] write_len_prefixed (data frame {}) failed: {}", frame_count, e);
                                format!("write_len_prefixed failed: {}", e)
                            })?;
                        }

                        // Send complete frame
                        let frame = sender.make_complete_frame();
                        let frame_bytes = frame
                            .to_bytes()
                            .map_err(|e| {
                                error!("[SendClipboard] frame serialization failed: {}", e);
                                format!("frame serialization failed: {}", e)
                            })?;

                        info!("[SendClipboard] Complete frame size: {} bytes", frame_bytes.len());
                        write_len_prefixed(&mut stream, &frame_bytes).await.map_err(|e| {
                            error!("[SendClipboard] write_len_prefixed (complete) failed: {}", e);
                            format!("write_len_prefixed failed: {}", e)
                        })?;
                        info!("[SendClipboard] Complete frame sent");

                        info!(
                            "[SendClipboard] BlobStream send complete: session_id={}, frames_sent={}, peer={}",
                            session_id, frame_count + 2, peer
                        );

                        Ok(())
                    }
                    .await;

                    info!(
                        "[SendClipboard] Task finished for peer {}, session_id={}, result: {:?}",
                        peer, session_id, result
                    );

                    // Respond to caller
                    let _ = respond_to.send(result.clone());

                    result
                });
            }

            NetworkCommand::BroadcastClipboard { message } => {
                // GossipSub has been removed, clipboard broadcast is now handled by BlobStream
                warn!(
                    "BroadcastClipboard command is deprecated in v2.0.0 (use SendClipboard with BlobStream instead)"
                );
            }

            NetworkCommand::SendPairingRequest { peer_id, message } => {
                match peer_id.parse::<PeerId>() {
                    Ok(peer) => {
                        let request = ReqPairingRequest { message };
                        self.swarm
                            .behaviour_mut()
                            .request_response
                            .send_request(&peer, request);
                        debug!("Sent pairing request to {}", peer_id);
                    }
                    Err(e) => {
                        warn!("Invalid peer_id '{}': {}", peer_id, e);
                        let _ = self
                            .event_tx
                            .send(NetworkEvent::Error(format!(
                                "Failed to send pairing request: invalid peer_id '{}': {}",
                                peer_id, e
                            )))
                            .await;
                    }
                }
            }

            NetworkCommand::SendPairingChallenge {
                peer_id,
                session_id,
                pin,
                device_name,
                device_id,
                local_peer_id,
            } => {
                // CRITICAL FIX: Send Challenge as a NEW request, not as a response
                // This works because the initiator (A) doesn't have a response channel
                // The responder (B) initiates a new request to send the Challenge to A
                match peer_id.parse::<PeerId>() {
                    Ok(peer) => {
                        let challenge = super::protocol::PairingChallenge {
                            session_id: session_id.clone(),
                            pin,
                            device_name,
                            device_id: device_id.clone(),
                        };
                        let protocol_msg =
                            ProtocolMessage::Pairing(PairingMessage::Challenge(challenge));
                        if let Ok(message) = protocol_msg.to_bytes() {
                            let request = ReqPairingRequest { message };
                            self.swarm
                                .behaviour_mut()
                                .request_response
                                .send_request(&peer, request);
                            debug!(
                                "Sent pairing challenge to {} for session {} (device_id: {}, peer_id: {})",
                                peer_id, session_id, device_id, local_peer_id
                            );
                        }
                    }
                    Err(e) => {
                        warn!("Invalid peer_id '{}': {}", peer_id, e);
                        let _ = self
                            .event_tx
                            .send(NetworkEvent::Error(format!(
                                "Failed to send pairing challenge: invalid peer_id '{}': {}",
                                peer_id, e
                            )))
                            .await;
                    }
                }
            }

            NetworkCommand::SendPairingResponse {
                peer_id,
                session_id,
                pin_hash,
                accepted,
            } => {
                // CRITICAL FIX: Send Response as a NEW request
                // Since Challenge is now sent as a request, Response must also be a request
                match peer_id.parse::<PeerId>() {
                    Ok(peer) => {
                        let response = super::protocol::PairingResponse {
                            session_id: session_id.clone(),
                            pin_hash,
                            accepted,
                        };
                        let protocol_msg =
                            ProtocolMessage::Pairing(PairingMessage::Response(response));
                        if let Ok(message) = protocol_msg.to_bytes() {
                            let request = ReqPairingRequest { message };
                            self.swarm
                                .behaviour_mut()
                                .request_response
                                .send_request(&peer, request);
                            debug!(
                                "Sent pairing response to {} for session {}",
                                peer_id, session_id
                            );
                        }
                    }
                    Err(e) => {
                        warn!("Invalid peer_id '{}': {}", peer_id, e);
                        let _ = self
                            .event_tx
                            .send(NetworkEvent::Error(format!(
                                "Failed to send pairing response: invalid peer_id '{}': {}",
                                peer_id, e
                            )))
                            .await;
                    }
                }
            }

            NetworkCommand::RejectPairing {
                peer_id,
                session_id,
            } => {
                // CRITICAL FIX: Send rejection as a NEW request
                // The responder sends a rejection confirm to the initiator
                match peer_id.parse::<PeerId>() {
                    Ok(peer) => {
                        let confirm = super::protocol::PairingConfirm {
                            session_id: session_id.clone(),
                            success: false,
                            error: Some("Pairing rejected by user".to_string()),
                            sender_device_name: self.device_name.clone(),
                            device_id: String::new(), // Empty for rejection
                        };
                        let protocol_msg =
                            ProtocolMessage::Pairing(PairingMessage::Confirm(confirm));
                        if let Ok(message) = protocol_msg.to_bytes() {
                            let request = ReqPairingRequest { message };
                            self.swarm
                                .behaviour_mut()
                                .request_response
                                .send_request(&peer, request);
                            debug!(
                                "Sent pairing rejection to {} for session {}",
                                peer_id, session_id
                            );
                        }
                    }
                    Err(e) => {
                        warn!("Invalid peer_id '{}': {}", peer_id, e);
                        let _ = self
                            .event_tx
                            .send(NetworkEvent::Error(format!(
                                "Failed to send pairing rejection: invalid peer_id '{}': {}",
                                peer_id, e
                            )))
                            .await;
                    }
                }
            }

            NetworkCommand::SendPairingConfirm {
                peer_id,
                session_id,
                success,
                device_name,
                device_id,
            } => match peer_id.parse::<PeerId>() {
                Ok(peer) => {
                    let confirm = super::protocol::PairingConfirm {
                        session_id,
                        success,
                        error: None,
                        sender_device_name: device_name,
                        device_id,
                    };
                    let protocol_msg = ProtocolMessage::Pairing(PairingMessage::Confirm(confirm));
                    if let Ok(message) = protocol_msg.to_bytes() {
                        let request = ReqPairingRequest { message };
                        self.swarm
                            .behaviour_mut()
                            .request_response
                            .send_request(&peer, request);
                    }
                }
                Err(e) => {
                    warn!("Invalid peer_id '{}': {}", peer_id, e);
                    let _ = self
                        .event_tx
                        .send(NetworkEvent::Error(format!(
                            "Failed to send pairing confirm: invalid peer_id '{}': {}",
                            peer_id, e
                        )))
                        .await;
                }
            },

            NetworkCommand::ReconnectPeers {
                paired_peer_addresses,
            } => {
                info!("Reconnecting to peers");

                let mut dialed_peers: std::collections::HashSet<PeerId> =
                    std::collections::HashSet::new();

                for (peer_id, peer) in &self.discovered_peers {
                    if self.connected_peers.contains_key(peer_id) {
                        continue;
                    }
                    if let Some(addr_str) = peer.addresses.first() {
                        if let Ok(addr) = addr_str.parse::<Multiaddr>() {
                            if let Err(e) = self.swarm.dial(addr) {
                                warn!("Failed to reconnect to {}: {}", peer_id, e);
                            }
                            dialed_peers.insert(*peer_id);
                        }
                    }
                }

                for (peer_id_str, addresses) in &paired_peer_addresses {
                    if let Ok(peer_id) = peer_id_str.parse::<PeerId>() {
                        if self.connected_peers.contains_key(&peer_id)
                            || dialed_peers.contains(&peer_id)
                        {
                            continue;
                        }

                        for addr_str in addresses {
                            if let Ok(addr) = addr_str.parse::<Multiaddr>() {
                                if let Err(e) = self.swarm.dial(addr.clone()) {
                                    warn!(
                                        "Failed to dial paired peer {} at {}: {}",
                                        peer_id, addr, e
                                    );
                                } else {
                                    dialed_peers.insert(peer_id);
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            NetworkCommand::RefreshPeer { peer_id } => match peer_id.parse::<PeerId>() {
                Ok(pid) => {
                    if let Some(peer) = self.discovered_peers.get(&pid) {
                        let _ = self
                            .event_tx
                            .send(NetworkEvent::PeerDiscovered(peer.clone()))
                            .await;
                    }
                }
                Err(e) => {
                    warn!("Invalid peer_id '{}': {}", peer_id, e);
                    let _ = self
                        .event_tx
                        .send(NetworkEvent::Error(format!(
                            "Failed to refresh peer: invalid peer_id '{}': {}",
                            peer_id, e
                        )))
                        .await;
                }
            },

            NetworkCommand::AnnounceDeviceName { device_name } => {
                self.device_name = device_name.clone();
                debug!("Device name updated to: {}", device_name);
                // Device announcement is now handled via Identify protocol
            }

            NetworkCommand::StartListening
            | NetworkCommand::StopListening
            | NetworkCommand::GetPeers => {}
        }
    }
}

/// 写入长度前缀的帧（u32 BE + payload）
///
/// # Arguments
///
/// * `stream` - 可写流
/// * `payload` - 要写入的数据
async fn write_len_prefixed(stream: &mut Stream, payload: &[u8]) -> Result<(), String> {
    let len = payload.len() as u32;
    stream
        .write_all(&len.to_be_bytes())
        .await
        .map_err(|e| e.to_string())?;
    stream.write_all(payload).await.map_err(|e| e.to_string())?;
    Ok(())
}

/// 读取长度前缀的帧（u32 BE + payload）
///
/// # Arguments
///
/// * `stream` - 可读流
async fn read_len_prefixed(stream: &mut Stream) -> Result<Vec<u8>, String> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(len_buf) as usize;

    // 限制单帧大小为 1MB，防止恶意数据
    const MAX_FRAME_SIZE: usize = 1024 * 1024;
    if len > MAX_FRAME_SIZE {
        return Err(format!(
            "Frame too large: {} bytes (max {})",
            len, MAX_FRAME_SIZE
        ));
    }

    let mut buf = vec![0u8; len];
    stream
        .read_exact(&mut buf)
        .await
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

impl std::fmt::Debug for NetworkCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SendClipboard { peer_id, .. } => f
                .debug_struct("SendClipboard")
                .field("peer_id", peer_id)
                .field("data_size", &"[REDACTED]")
                .finish(),
            Self::SendPairingChallenge {
                peer_id,
                session_id,
                ..
            } => f
                .debug_struct("SendPairingChallenge")
                .field("peer_id", peer_id)
                .field("session_id", session_id)
                .field("pin", &"[REDACTED]")
                .field("public_key", &"[REDACTED]")
                .finish(),
            Self::SendPairingConfirm {
                peer_id,
                session_id,
                success,
                ..
            } => f
                .debug_struct("SendPairingConfirm")
                .field("peer_id", peer_id)
                .field("session_id", session_id)
                .field("success", success)
                .field("shared_secret", &"[REDACTED]")
                .finish(),
            Self::SendPairingRequest { peer_id, .. } => f
                .debug_struct("SendPairingRequest")
                .field("peer_id", peer_id)
                .field("message", &"[REDACTED]")
                .finish(),
            _ => {
                // For non-sensitive variants, use discriminant
                write!(f, "{:?}", std::mem::discriminant(self))
            }
        }
    }
}
