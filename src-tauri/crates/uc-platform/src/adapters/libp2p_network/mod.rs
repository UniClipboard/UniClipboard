mod behaviour;
mod business_command;
mod business_stream;
mod dial_strategy;
mod discovery;
pub(crate) mod peer_cache;
mod stream_handler;
mod swarm_event_loop;
#[cfg(test)]
#[allow(deprecated)]
mod tests;

use crate::ports::IdentityStorePort;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use libp2p::{identity, noise, tcp, yamux, Multiaddr, PeerId, SwarmBuilder};
use libp2p_stream as stream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};
use tokio::time::{sleep, timeout};
use tracing::{debug, error, info, warn};
use uc_core::network::{
    ClipboardMessage, ConnectedPeer, DiscoveredPeer, NetworkEvent, PairingMessage, PairingState,
    ProtocolDirection, ProtocolId, ProtocolKind, ProtocolMessage, ResolvedConnectionPolicy,
};
use uc_core::ports::{
    ClipboardTransportPort, ConnectionPolicyResolverPort, EncryptionSessionPort,
    NetworkControlPort, NetworkEventPort, PairingTransportPort, PeerDirectoryPort,
    TransferPayloadDecryptorPort, TransferPayloadEncryptorPort,
};

use super::file_transfer::service::{FileTransferConfig, FileTransferService};
use super::network::PairingRuntimeOwner;
use super::pairing_stream::service::{PairingStreamConfig, PairingStreamService};
use crate::identity_store::load_or_create_identity;

// Re-export submodule types used throughout this module.
use behaviour::{build_mdns_config, start_state_name, Libp2pBehaviour};
use business_command::notify_enqueue_failure;
use dial_strategy::{
    chosen_dial_addr_for_log, dial_decision_for_snapshot, infer_chosen_dial_addr_resolution,
    preferred_candidate_transport, transport_label,
};
use peer_cache::{snapshot_peer_addresses, PeerAddressSnapshot, PeerCaches};
use stream_handler::{
    emit_protocol_denied, handle_pairing_open_error, spawn_business_stream_handler,
};
use swarm_event_loop::run_swarm;

const BUSINESS_PROTOCOL_ID: &str = ProtocolId::Business.as_str();
const BUSINESS_PAYLOAD_MAX_BYTES: u64 = 300 * 1024 * 1024;
/// Network I/O chunk size for writing outbound payloads (256 KB).
const NETWORK_CHUNK_SIZE: usize = 256 * 1024;
/// Maximum allowed ciphertext length per chunk (plaintext chunk + encryption overhead).
const MAX_CHUNK_CIPHERTEXT_SIZE: usize = NETWORK_CHUNK_SIZE + 256;
const BUSINESS_READ_TIMEOUT: Duration = Duration::from_secs(120);
const BUSINESS_STREAM_OPEN_TIMEOUT: Duration = Duration::from_secs(10);
const PAIRING_STREAM_OPEN_TIMEOUT: Duration = Duration::from_secs(10);
const PAIRING_OPEN_SUCCESS_OBSERVATION_POLL_INTERVAL: Duration = Duration::from_millis(10);
const PAIRING_OPEN_SUCCESS_OBSERVATION_POLL_ATTEMPTS: usize = 5;
const BUSINESS_STREAM_WRITE_TIMEOUT: Duration = Duration::from_secs(120);
const BUSINESS_STREAM_CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
const BUSINESS_COMMAND_ENQUEUE_TIMEOUT: Duration = Duration::from_secs(5);
const BUSINESS_SEND_COMMAND_RESULT_TIMEOUT: Duration = Duration::from_secs(150);
const BUSINESS_ENSURE_COMMAND_RESULT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_IN_FLIGHT_BUSINESS_COMMANDS: usize = 16;
const START_STATE_IDLE: u8 = 0;
const START_STATE_STARTING: u8 = 1;
const START_STATE_STARTED: u8 = 2;
const START_STATE_FAILED: u8 = 3;

#[derive(Debug)]
enum BusinessCommand {
    SendClipboard {
        peer_id: uc_core::PeerId,
        data: Arc<[u8]>,
        result_tx: oneshot::Sender<Result<()>>,
    },
    EnsureBusinessPath {
        peer_id: uc_core::PeerId,
        result_tx: oneshot::Sender<Result<()>>,
    },
    AnnounceDeviceName {
        device_name: String,
    },
    UnpairPeer {
        peer_id: uc_core::PeerId,
        result_tx: oneshot::Sender<Result<()>>,
    },
}

/// A request from a business stream to pre-dial a peer with explicit addresses
/// before opening a stream.  Handled by the swarm event loop.
#[derive(Debug)]
pub(super) struct DialRequest {
    pub peer: PeerId,
    pub addresses: Vec<Multiaddr>,
    pub result_tx: oneshot::Sender<Result<()>>,
}

/// Maximum JSON header size (64KB). Streams with larger headers are discarded.
const MAX_JSON_HEADER_SIZE: usize = 64 * 1024;

/// Result of processing a single inbound business stream message.
enum ProcessedMessage {
    /// Clipboard with pre-decoded plaintext from transport-level streaming decode.
    StreamingClipboard(ClipboardMessage, Vec<u8>),
    /// All other messages (DeviceAnnounce, Heartbeat, Pairing).
    Standard(ProtocolMessage),
}

pub struct Libp2pNetworkAdapter {
    local_peer_id: String,
    local_identity_pubkey: Vec<u8>,
    caches: Arc<RwLock<PeerCaches>>,
    event_tx: mpsc::Sender<NetworkEvent>,
    event_ingress_rx: Mutex<Option<mpsc::Receiver<NetworkEvent>>>,
    event_bus_tx: broadcast::Sender<NetworkEvent>,
    event_fanout_started: AtomicBool,
    clipboard_tx: mpsc::Sender<(ClipboardMessage, Option<Vec<u8>>)>,
    clipboard_rx: Mutex<Option<mpsc::Receiver<(ClipboardMessage, Option<Vec<u8>>)>>>,
    business_tx: mpsc::Sender<BusinessCommand>,
    business_rx: Mutex<Option<mpsc::Receiver<BusinessCommand>>>,
    dial_tx: mpsc::Sender<DialRequest>,
    dial_rx: Mutex<Option<mpsc::Receiver<DialRequest>>>,
    keypair: Mutex<identity::Keypair>,
    start_state: AtomicU8,
    policy_resolver: Arc<dyn ConnectionPolicyResolverPort>,
    encryption_session: Arc<dyn EncryptionSessionPort>,
    transfer_decryptor: Arc<dyn TransferPayloadDecryptorPort>,
    _transfer_encryptor: Arc<dyn TransferPayloadEncryptorPort>,
    stream_control: Mutex<Option<stream::Control>>,
    pairing_runtime_owner: PairingRuntimeOwner,
    pairing_service: Mutex<Option<PairingStreamService>>,
    file_transfer_service: Mutex<Option<FileTransferService>>,
    file_cache_dir: PathBuf,
}

impl Libp2pNetworkAdapter {
    pub fn new(
        identity_store: Arc<dyn IdentityStorePort>,
        policy_resolver: Arc<dyn ConnectionPolicyResolverPort>,
        encryption_session: Arc<dyn EncryptionSessionPort>,
        transfer_decryptor: Arc<dyn TransferPayloadDecryptorPort>,
        transfer_encryptor: Arc<dyn TransferPayloadEncryptorPort>,
        file_cache_dir: PathBuf,
        pairing_runtime_owner: PairingRuntimeOwner,
    ) -> Result<Self> {
        let keypair = load_or_create_identity(identity_store.as_ref())
            .map_err(|e| anyhow!("failed to load libp2p identity: {e}"))?;
        let local_peer_id = PeerId::from(keypair.public()).to_string();
        let local_identity_pubkey = keypair
            .public()
            .try_into_ed25519()
            .map_err(|err| anyhow!("failed to extract ed25519 public key: {err}"))?
            .to_bytes()
            .to_vec();
        let (event_tx, event_ingress_rx) = mpsc::channel(64);
        let (event_bus_tx, _) = broadcast::channel(64);
        let (clipboard_tx, clipboard_rx) = mpsc::channel(64);
        let (business_tx, business_rx) = mpsc::channel(64);
        let (dial_tx, dial_rx) = mpsc::channel(32);
        let pairing_service = Mutex::new(None);

        Ok(Self {
            local_peer_id,
            local_identity_pubkey,
            caches: Arc::new(RwLock::new(PeerCaches::new())),
            event_tx,
            event_ingress_rx: Mutex::new(Some(event_ingress_rx)),
            event_bus_tx,
            event_fanout_started: AtomicBool::new(false),
            clipboard_tx,
            clipboard_rx: Mutex::new(Some(clipboard_rx)),
            business_tx,
            business_rx: Mutex::new(Some(business_rx)),
            dial_tx,
            dial_rx: Mutex::new(Some(dial_rx)),
            keypair: Mutex::new(keypair),
            start_state: AtomicU8::new(START_STATE_IDLE),
            policy_resolver,
            encryption_session,
            transfer_decryptor,
            _transfer_encryptor: transfer_encryptor,
            stream_control: Mutex::new(None),
            pairing_runtime_owner,
            pairing_service,
            file_transfer_service: Mutex::new(None),
            file_cache_dir,
        })
    }

    pub fn local_identity_pubkey(&self) -> Vec<u8> {
        self.local_identity_pubkey.clone()
    }

    pub fn pairing_runtime_owner(&self) -> PairingRuntimeOwner {
        self.pairing_runtime_owner
    }

    async fn ensure_event_fanout_started(&self) -> Result<()> {
        if self
            .event_fanout_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(());
        }

        let mut ingress_rx = self
            .event_ingress_rx
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| anyhow!("network event ingress receiver missing"))?;
        let event_bus_tx = self.event_bus_tx.clone();

        tokio::spawn(async move {
            while let Some(event) = ingress_rx.recv().await {
                let _ = event_bus_tx.send(event);
            }
        });

        Ok(())
    }

    pub fn spawn_swarm(&self) -> Result<()> {
        let mdns_config = build_mdns_config();
        info!(
            query_interval_secs = mdns_config.query_interval.as_secs(),
            ttl_secs = mdns_config.ttl.as_secs(),
            enable_ipv6 = mdns_config.enable_ipv6,
            local_peer_id = %self.local_peer_id,
            "preparing libp2p swarm"
        );
        let keypair = self.take_keypair()?;
        let local_peer_id = PeerId::from(keypair.public());
        let behaviour = Libp2pBehaviour::new(local_peer_id)
            .map_err(|e| anyhow!("failed to create libp2p behaviour: {e}"))?;

        let mut swarm = SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default().nodelay(true),
                noise::Config::new,
                yamux::Config::default,
            )
            .map_err(|e| anyhow!("failed to configure tcp transport: {e}"))?
            .with_quic()
            .with_behaviour(move |_| behaviour)
            .map_err(|e| anyhow!("failed to attach libp2p behaviour: {e}"))?
            .build();

        let stream_control = swarm.behaviour().stream.new_control();
        {
            let mut guard = self
                .stream_control
                .lock()
                .map_err(|_| anyhow!("stream control mutex poisoned"))?;
            *guard = Some(stream_control.clone());
        }
        if self.pairing_runtime_owner == PairingRuntimeOwner::CurrentProcess {
            // CurrentProcess owns local pairing protocol registration and accept loop startup.
            let pairing_service = PairingStreamService::new(
                stream_control.clone(),
                self.event_tx.clone(),
                PairingStreamConfig::default(),
            );
            pairing_service.spawn_accept_loop();
            let mut guard = self
                .pairing_service
                .lock()
                .map_err(|_| anyhow!("pairing service mutex poisoned"))?;
            *guard = Some(pairing_service);
        } else {
            info!(
                local_peer_id = %self.local_peer_id,
                "skip local pairing runtime initialization and pairing protocol registration; external daemon owns pairing runtime"
            );
        }

        // Construct FileTransferService and spawn accept loop
        let file_transfer_service = FileTransferService::new(
            stream_control.clone(),
            self.event_tx.clone(),
            Arc::new(uc_core::ports::transfer_progress::NoopTransferProgressPort),
            FileTransferConfig::new(self.file_cache_dir.clone()),
        );
        file_transfer_service.spawn_accept_loop();
        {
            let mut guard = self
                .file_transfer_service
                .lock()
                .map_err(|_| anyhow!("file transfer service mutex poisoned"))?;
            *guard = Some(file_transfer_service);
        }

        spawn_business_stream_handler(
            stream_control.clone(),
            self.caches.clone(),
            self.event_tx.clone(),
            self.clipboard_tx.clone(),
            self.policy_resolver.clone(),
            self.encryption_session.clone(),
            self.transfer_decryptor.clone(),
        );

        let listen_ip = match crate::net_utils::get_physical_lan_ip() {
            Some(ip) => ip.to_string(),
            None => {
                warn!(
                    local_peer_id = %self.local_peer_id,
                    "no physical LAN IP detected, fallback to 0.0.0.0"
                );
                "0.0.0.0".to_string()
            }
        };
        let quic_addr_str = format!("/ip4/{listen_ip}/udp/0/quic-v1");
        let tcp_addr_str = format!("/ip4/{listen_ip}/tcp/0");
        info!(
            event = "network.listen_addresses_selected",
            listen_ip = %listen_ip,
            quic_address = %quic_addr_str,
            tcp_address = %tcp_addr_str,
            "selected listen addresses"
        );

        let quic_addr: Multiaddr = quic_addr_str
            .parse()
            .map_err(|e| anyhow!("failed to parse quic listen address: {e}"))?;
        let tcp_addr: Multiaddr = tcp_addr_str
            .parse()
            .map_err(|e| anyhow!("failed to parse tcp listen address: {e}"))?;

        // Partial startup is acceptable: if at least one transport binds,
        // the node can operate. Individual transport failures are logged as
        // warnings by listen_on_swarm but do not emit error events.
        let quic_ok = listen_on_swarm(&mut swarm, quic_addr).is_ok();
        let tcp_ok = listen_on_swarm(&mut swarm, tcp_addr).is_ok();

        if !quic_ok && !tcp_ok {
            return Err(anyhow!(
                "failed to listen on any transport (tried QUIC and TCP)"
            ));
        }

        let caches = self.caches.clone();
        let event_tx = self.event_tx.clone();
        let policy_resolver = self.policy_resolver.clone();
        let business_rx = Self::take_receiver(&self.business_rx, "business command")?;
        let dial_rx = Self::take_receiver(&self.dial_rx, "dial request")?;
        let dial_tx = self.dial_tx.clone();
        let local_peer_id = self.local_peer_id.clone();
        tokio::spawn(async move {
            run_swarm(
                swarm,
                caches,
                event_tx,
                policy_resolver,
                business_rx,
                dial_rx,
                dial_tx,
                local_peer_id,
            )
            .await;
        });
        Ok(())
    }

    fn take_keypair(&self) -> Result<identity::Keypair> {
        let guard = self
            .keypair
            .lock()
            .map_err(|_| anyhow!("libp2p keypair mutex poisoned"))?;
        Ok(guard.clone())
    }

    fn take_receiver<T>(
        mutex: &Mutex<Option<mpsc::Receiver<T>>>,
        name: &str,
    ) -> Result<mpsc::Receiver<T>> {
        let mut guard = mutex
            .lock()
            .map_err(|_| anyhow!("{name} receiver mutex poisoned"))?;
        match guard.take() {
            Some(rx) => {
                tracing::info!("{name} receiver taken successfully");
                Ok(rx)
            }
            None => {
                let bt = std::backtrace::Backtrace::force_capture();
                tracing::error!("{name} receiver already taken — backtrace:\n{bt}");
                Err(anyhow!("{name} receiver already taken"))
            }
        }
    }
}

#[async_trait]
impl ClipboardTransportPort for Libp2pNetworkAdapter {
    async fn send_clipboard(&self, _peer_id: &str, _encrypted_data: Arc<[u8]>) -> Result<()> {
        if _peer_id == self.local_peer_id {
            warn!(peer_id = _peer_id, "skip send_clipboard to local peer");
            return Err(anyhow!("send_clipboard target is local peer_id"));
        }
        let peer = uc_core::PeerId::from(_peer_id);
        let (result_tx, result_rx) = oneshot::channel();
        let command = BusinessCommand::SendClipboard {
            peer_id: peer,
            data: _encrypted_data,
            result_tx,
        };
        let enqueue_result = timeout(
            BUSINESS_COMMAND_ENQUEUE_TIMEOUT,
            self.business_tx.send(command),
        )
        .await;
        match enqueue_result {
            Ok(Ok(())) => {}
            Ok(Err(tokio::sync::mpsc::error::SendError(command))) => {
                let message = "failed to queue business stream: business command channel closed";
                error!(
                    peer_id = _peer_id,
                    error = message,
                    "business command enqueue failed"
                );
                notify_enqueue_failure(command, message, "clipboard", _peer_id);
                return Err(anyhow!(message));
            }
            Err(_) => {
                // Cancelling the send future drops the unsent command and closes its result_tx.
                let message = "timed out queueing business stream command";
                error!(
                    peer_id = _peer_id,
                    timeout_ms = BUSINESS_COMMAND_ENQUEUE_TIMEOUT.as_millis() as u64,
                    error = message,
                    "business command enqueue timed out"
                );
                return Err(anyhow!(message));
            }
        }
        match timeout(BUSINESS_SEND_COMMAND_RESULT_TIMEOUT, result_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(err)) => Err(anyhow!("failed to receive business stream result: {err}")),
            Err(_) => Err(anyhow!("timed out waiting for business command result")),
        }
    }

    async fn broadcast_clipboard(&self, _encrypted_data: Arc<[u8]>) -> Result<()> {
        Err(anyhow!(
            "ClipboardTransportPort::broadcast_clipboard not implemented yet"
        ))
    }

    async fn subscribe_clipboard(
        &self,
    ) -> Result<mpsc::Receiver<(ClipboardMessage, Option<Vec<u8>>)>> {
        if self.clipboard_tx.is_closed() {
            warn!("clipboard channel sender is closed");
        }
        Self::take_receiver(&self.clipboard_rx, "clipboard")
    }

    async fn ensure_business_path(&self, peer_id: &str) -> Result<()> {
        let peer = uc_core::PeerId::from(peer_id);
        let (result_tx, result_rx) = oneshot::channel();
        let command = BusinessCommand::EnsureBusinessPath {
            peer_id: peer,
            result_tx,
        };
        let enqueue_result = timeout(
            BUSINESS_COMMAND_ENQUEUE_TIMEOUT,
            self.business_tx.send(command),
        )
        .await;
        match enqueue_result {
            Ok(Ok(())) => {}
            Ok(Err(tokio::sync::mpsc::error::SendError(command))) => {
                let message =
                    "failed to queue ensure business path command: business command channel closed";
                error!(
                    peer_id = peer_id,
                    error = message,
                    "business command enqueue failed"
                );
                notify_enqueue_failure(command, message, "ensure", peer_id);
                return Err(anyhow!(message));
            }
            Err(_) => {
                // Cancelling the send future drops the unsent command and closes its result_tx.
                let message = "timed out queueing ensure business path command";
                error!(
                    peer_id = peer_id,
                    timeout_ms = BUSINESS_COMMAND_ENQUEUE_TIMEOUT.as_millis() as u64,
                    error = message,
                    "business command enqueue timed out"
                );
                return Err(anyhow!(message));
            }
        }

        match timeout(BUSINESS_ENSURE_COMMAND_RESULT_TIMEOUT, result_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(err)) => Err(anyhow!(
                "failed to receive ensure business path result: {err}"
            )),
            Err(_) => Err(anyhow!("timed out waiting for business command result")),
        }
    }
}

#[async_trait]
impl PeerDirectoryPort for Libp2pNetworkAdapter {
    async fn get_discovered_peers(&self) -> Result<Vec<DiscoveredPeer>> {
        let caches = self.caches.read().await;
        let local_id = &self.local_peer_id;
        let peers: Vec<DiscoveredPeer> = caches
            .discovered_peers
            .values()
            .filter(|p| p.peer_id != *local_id)
            .cloned()
            .collect();
        debug!(
            discovered_peer_count = peers.len(),
            reachable_peer_count = caches.reachable_peers.len(),
            "snapshot discovered peers (local_peer_id filtered)"
        );
        Ok(peers)
    }

    async fn get_connected_peers(&self) -> Result<Vec<ConnectedPeer>> {
        let caches = self.caches.read().await;
        let mut peers = Vec::new();
        for peer_id in caches.reachable_peers.iter() {
            let connected_at = caches
                .connected_at
                .get(peer_id)
                .cloned()
                .unwrap_or_else(Utc::now);
            let device_name = caches
                .discovered_peers
                .get(peer_id)
                .and_then(|peer| peer.device_name.clone())
                .unwrap_or_else(|| "Unknown Device".to_string());
            peers.push(ConnectedPeer {
                peer_id: peer_id.clone(),
                device_name,
                connected_at,
            });
        }
        Ok(peers)
    }

    async fn list_sendable_peers(&self) -> Result<Vec<DiscoveredPeer>> {
        let discovered: Vec<DiscoveredPeer> = {
            let caches = self.caches.read().await;
            caches.discovered_peers.values().cloned().collect()
        };

        let mut sendable = Vec::new();
        for mut peer in discovered {
            if peer.peer_id == self.local_peer_id {
                debug!(peer_id = %peer.peer_id, "skip local peer in sendable peer list");
                continue;
            }
            let policy = match self
                .policy_resolver
                .resolve_for_peer(&uc_core::PeerId::from(peer.peer_id.as_str()))
                .await
            {
                Ok(policy) => policy,
                Err(err) => {
                    warn!(
                        peer_id = %peer.peer_id,
                        error = %err,
                        "failed to resolve connection policy while listing sendable peers"
                    );
                    continue;
                }
            };

            if policy.allowed.allows(ProtocolKind::Business) {
                peer.is_paired = matches!(policy.pairing_state, PairingState::Trusted);
                sendable.push(peer);
            }
        }
        Ok(sendable)
    }

    fn local_peer_id(&self) -> String {
        self.local_peer_id.clone()
    }

    async fn announce_device_name(&self, device_name: String) -> Result<()> {
        match timeout(
            BUSINESS_COMMAND_ENQUEUE_TIMEOUT,
            self.business_tx
                .send(BusinessCommand::AnnounceDeviceName { device_name }),
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(anyhow!("failed to queue device announce: {err}")),
            Err(_) => Err(anyhow!(
                "timed out queueing device announce command after {} ms",
                BUSINESS_COMMAND_ENQUEUE_TIMEOUT.as_millis()
            )),
        }
    }
}

#[async_trait]
impl PairingTransportPort for Libp2pNetworkAdapter {
    async fn open_pairing_session(&self, peer_id: String, session_id: String) -> Result<()> {
        let service = {
            let guard = self
                .pairing_service
                .lock()
                .map_err(|_| anyhow!("pairing service mutex poisoned"))?;
            guard
                .as_ref()
                .cloned()
                .ok_or_else(|| anyhow!("pairing service not initialized"))?
        };
        if service.has_session(&session_id).await {
            info!(
                event = "pairing_stream.open_skipped",
                peer_id = %peer_id,
                session_id = %session_id,
                skip_reason = "session_already_open",
                "pairing stream open skipped because session already exists"
            );
            return Ok(());
        }
        let attempt_started_at = Utc::now();
        let attempt_snapshot = {
            let caches = self.caches.read().await;
            snapshot_peer_addresses(&caches, &peer_id, attempt_started_at)
        };
        let dial_decision = dial_decision_for_snapshot(&attempt_snapshot);
        info!(
            event = "pairing_stream.open_attempt",
            peer_id = %peer_id,
            session_id = %session_id,
            dial_decision,
            peer_marked_reachable = attempt_snapshot.peer_marked_reachable,
            candidate_address_count = attempt_snapshot.candidate_addresses.len(),
            preferred_candidate_transport = preferred_candidate_transport(&attempt_snapshot),
            connected_age_ms = ?attempt_snapshot.connected_age_ms,
            discovered_age_ms = ?attempt_snapshot.discovered_age_ms,
            last_seen_age_ms = ?attempt_snapshot.last_seen_age_ms,
            "attempting pairing stream open"
        );
        match timeout(
            PAIRING_STREAM_OPEN_TIMEOUT,
            service.open_pairing_session(peer_id.clone(), session_id.clone()),
        )
        .await
        {
            Ok(Ok(())) => {
                let success_snapshot = snapshot_pairing_open_success(
                    &self.caches,
                    &peer_id,
                    dial_decision,
                    attempt_started_at,
                )
                .await;
                let chosen_dial_addr =
                    chosen_dial_addr_for_log(&success_snapshot, dial_decision, attempt_started_at);
                let chosen_dial_addr_resolution = infer_chosen_dial_addr_resolution(
                    &success_snapshot,
                    dial_decision,
                    attempt_started_at,
                );
                info!(
                    event = "pairing_stream.open_succeeded",
                    peer_id = %peer_id,
                    session_id = %session_id,
                    dial_decision,
                    candidate_address_count = success_snapshot.candidate_addresses.len(),
                    chosen_dial_addr = %chosen_dial_addr.unwrap_or("-"),
                    chosen_dial_addr_resolution,
                    dial_attempt_address_count = success_snapshot.dial_attempt_address_count,
                    dial_attempt_addresses = ?success_snapshot.dial_attempt_addresses,
                    last_dial_outcome = success_snapshot.last_dial_outcome.unwrap_or("unknown"),
                    last_dial_age_ms = ?success_snapshot.last_dial_age_ms,
                    "pairing stream open succeeded"
                );
                Ok(())
            }
            Ok(Err(err)) => {
                let failure_snapshot = {
                    let caches = self.caches.read().await;
                    snapshot_peer_addresses(&caches, &peer_id, Utc::now())
                };
                let chosen_dial_addr =
                    chosen_dial_addr_for_log(&failure_snapshot, dial_decision, attempt_started_at);
                let chosen_dial_addr_resolution = infer_chosen_dial_addr_resolution(
                    &failure_snapshot,
                    dial_decision,
                    attempt_started_at,
                );
                warn!(
                    event = "pairing_stream.open_failed",
                    peer_id = %peer_id,
                    session_id = %session_id,
                    dial_decision,
                    candidate_address_count = failure_snapshot.candidate_addresses.len(),
                    candidate_addresses = ?failure_snapshot.candidate_addresses,
                    chosen_dial_addr = %chosen_dial_addr.unwrap_or("-"),
                    chosen_dial_addr_resolution,
                    dial_attempt_address_count = failure_snapshot.dial_attempt_address_count,
                    dial_attempt_addresses = ?failure_snapshot.dial_attempt_addresses,
                    last_dial_outcome = failure_snapshot.last_dial_outcome.unwrap_or("unknown"),
                    last_dial_age_ms = ?failure_snapshot.last_dial_age_ms,
                    error = %err,
                    "pairing stream open failed"
                );
                handle_pairing_open_error(&self.policy_resolver, &self.event_tx, &peer_id, &err)
                    .await;
                Err(err)
            }
            Err(_) => {
                let timeout_snapshot = {
                    let caches = self.caches.read().await;
                    snapshot_peer_addresses(&caches, &peer_id, Utc::now())
                };
                let chosen_dial_addr =
                    chosen_dial_addr_for_log(&timeout_snapshot, dial_decision, attempt_started_at);
                let chosen_dial_addr_resolution = infer_chosen_dial_addr_resolution(
                    &timeout_snapshot,
                    dial_decision,
                    attempt_started_at,
                );
                warn!(
                    event = "pairing_stream.open_timeout",
                    peer_id = %peer_id,
                    session_id = %session_id,
                    dial_decision,
                    candidate_address_count = timeout_snapshot.candidate_addresses.len(),
                    candidate_addresses = ?timeout_snapshot.candidate_addresses,
                    chosen_dial_addr = %chosen_dial_addr.unwrap_or("-"),
                    chosen_dial_addr_resolution,
                    dial_attempt_address_count = timeout_snapshot.dial_attempt_address_count,
                    dial_attempt_addresses = ?timeout_snapshot.dial_attempt_addresses,
                    last_dial_outcome = timeout_snapshot.last_dial_outcome.unwrap_or("unknown"),
                    last_dial_age_ms = ?timeout_snapshot.last_dial_age_ms,
                    timeout_ms = PAIRING_STREAM_OPEN_TIMEOUT.as_millis() as u64,
                    "pairing stream open timed out"
                );
                Err(anyhow!("pairing stream open timed out"))
            }
        }
    }

    async fn send_pairing_on_session(&self, message: PairingMessage) -> Result<()> {
        let service = {
            let guard = self
                .pairing_service
                .lock()
                .map_err(|_| anyhow!("pairing service mutex poisoned"))?;
            guard
                .as_ref()
                .cloned()
                .ok_or_else(|| anyhow!("pairing service not initialized"))?
        };
        service.send_pairing_on_session(message).await
    }

    async fn close_pairing_session(
        &self,
        session_id: String,
        reason: Option<String>,
    ) -> Result<()> {
        let service = {
            let guard = self
                .pairing_service
                .lock()
                .map_err(|_| anyhow!("pairing service mutex poisoned"))?;
            guard
                .as_ref()
                .cloned()
                .ok_or_else(|| anyhow!("pairing service not initialized"))?
        };
        service.close_pairing_session(session_id, reason).await
    }

    async fn unpair_device(&self, peer_id: String) -> Result<()> {
        peer_id
            .parse::<PeerId>()
            .map_err(|err| anyhow!("invalid peer id for unpair_device: {err}"))?;
        if peer_id == self.local_peer_id {
            return Err(anyhow!("cannot unpair local peer id"));
        }

        let service = {
            let guard = self
                .pairing_service
                .lock()
                .map_err(|_| anyhow!("pairing service mutex poisoned"))?;
            guard
                .as_ref()
                .cloned()
                .ok_or_else(|| anyhow!("pairing service not initialized"))?
        };
        service.close_sessions_for_peer(&peer_id).await?;

        let (result_tx, result_rx) = oneshot::channel();
        let command = BusinessCommand::UnpairPeer {
            peer_id: uc_core::PeerId::from(peer_id.as_str()),
            result_tx,
        };
        match timeout(
            BUSINESS_COMMAND_ENQUEUE_TIMEOUT,
            self.business_tx.send(command),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(tokio::sync::mpsc::error::SendError(command))) => {
                let message = "failed to queue unpair command: business command channel closed";
                notify_enqueue_failure(command, message, "unpair", &peer_id);
                return Err(anyhow!(message));
            }
            Err(_) => {
                return Err(anyhow!(
                    "timed out queueing unpair command after {} ms",
                    BUSINESS_COMMAND_ENQUEUE_TIMEOUT.as_millis()
                ));
            }
        }
        let unpair_result = match timeout(BUSINESS_ENSURE_COMMAND_RESULT_TIMEOUT, result_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(err)) => Err(anyhow!("failed to receive unpair command result: {err}")),
            Err(_) => Err(anyhow!("timed out waiting for unpair command result")),
        };
        if let Err(err) = unpair_result {
            error!(
                peer_id = %peer_id,
                error = %err,
                "unpair command failed; skipping peer cache mutation and peer-lost event"
            );
            return Err(err);
        }

        let event = {
            let mut caches = self.caches.write().await;
            caches
                .remove_discovered(&peer_id)
                .map(|_| NetworkEvent::PeerLost(peer_id.clone()))
        };
        if let Some(event) = event {
            if let Err(err) = self.event_tx.send(event).await {
                warn!(
                    peer_id = %peer_id,
                    error = %err,
                    "failed to publish peer lost event after unpair"
                );
            }
        }
        Ok(())
    }
}

#[async_trait]
impl NetworkEventPort for Libp2pNetworkAdapter {
    async fn subscribe_events(&self) -> Result<mpsc::Receiver<NetworkEvent>> {
        self.ensure_event_fanout_started().await?;

        let mut broadcast_rx = self.event_bus_tx.subscribe();
        let (event_tx, event_rx) = mpsc::channel(64);

        tokio::spawn(async move {
            loop {
                match broadcast_rx.recv().await {
                    Ok(event) => {
                        if event_tx.send(event).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!(
                            skipped,
                            "network event subscriber lagged behind fanout channel"
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        Ok(event_rx)
    }
}

#[async_trait]
impl NetworkControlPort for Libp2pNetworkAdapter {
    async fn start_network(&self) -> Result<()> {
        let mut state = self.start_state.load(Ordering::Acquire);
        info!(
            state = start_state_name(state),
            local_peer_id = %self.local_peer_id,
            "start_network requested"
        );
        loop {
            match state {
                START_STATE_IDLE | START_STATE_FAILED => {
                    match self.start_state.compare_exchange(
                        state,
                        START_STATE_STARTING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => {
                            info!(
                                previous_state = start_state_name(state),
                                next_state = start_state_name(START_STATE_STARTING),
                                local_peer_id = %self.local_peer_id,
                                "network start state transition"
                            );
                            break;
                        }
                        Err(current) => {
                            debug!(
                                expected_state = start_state_name(state),
                                current_state = start_state_name(current),
                                local_peer_id = %self.local_peer_id,
                                "network start race detected, retrying compare_exchange"
                            );
                            state = current;
                            continue;
                        }
                    }
                }
                START_STATE_STARTING | START_STATE_STARTED => {
                    info!(
                        state = start_state_name(state),
                        local_peer_id = %self.local_peer_id,
                        "start_network no-op because network already active"
                    );
                    return Ok(());
                }
                _ => {
                    warn!(
                        state,
                        local_peer_id = %self.local_peer_id,
                        "start_network saw invalid start state, resetting to idle"
                    );
                    self.start_state.store(START_STATE_IDLE, Ordering::Release);
                    state = START_STATE_IDLE;
                }
            }
        }

        if self.pairing_runtime_owner == PairingRuntimeOwner::ExternalDaemon {
            self.start_state
                .store(START_STATE_STARTED, Ordering::Release);
            info!(
                state = start_state_name(START_STATE_STARTED),
                local_peer_id = %self.local_peer_id,
                "start_network skipped because external daemon owns libp2p swarm"
            );
            return Ok(());
        }

        match self.spawn_swarm() {
            Ok(()) => {
                self.start_state
                    .store(START_STATE_STARTED, Ordering::Release);
                info!(
                    state = start_state_name(START_STATE_STARTED),
                    local_peer_id = %self.local_peer_id,
                    "network swarm started successfully"
                );
                Ok(())
            }
            Err(err) => {
                self.start_state.store(START_STATE_IDLE, Ordering::Release);
                error!(
                    error = %err,
                    local_peer_id = %self.local_peer_id,
                    "failed to start network swarm"
                );
                Err(err)
            }
        }
    }
}

#[async_trait]
impl uc_core::ports::FileTransportPort for Libp2pNetworkAdapter {
    async fn send_file_announce(
        &self,
        _peer_id: &str,
        _announce: uc_core::network::protocol::FileTransferMessage,
    ) -> Result<()> {
        // Individual message methods are not used — full transfer goes through send_file()
        Ok(())
    }

    async fn send_file_data(
        &self,
        _peer_id: &str,
        _data: uc_core::network::protocol::FileTransferMessage,
    ) -> Result<()> {
        Ok(())
    }

    async fn send_file_complete(
        &self,
        _peer_id: &str,
        _complete: uc_core::network::protocol::FileTransferMessage,
    ) -> Result<()> {
        Ok(())
    }

    async fn cancel_transfer(
        &self,
        _peer_id: &str,
        _cancel: uc_core::network::protocol::FileTransferMessage,
    ) -> Result<()> {
        Ok(())
    }

    async fn send_file(
        &self,
        peer_id: &str,
        file_path: std::path::PathBuf,
        transfer_id: String,
        batch_id: Option<String>,
        batch_total: Option<u32>,
    ) -> Result<()> {
        let service = {
            let guard = self
                .file_transfer_service
                .lock()
                .map_err(|_| anyhow!("file transfer service mutex poisoned"))?;
            guard
                .as_ref()
                .ok_or_else(|| {
                    anyhow!("file transfer service not initialized — network not started")
                })?
                .clone()
        };
        service
            .send_file(peer_id, file_path, transfer_id, batch_id, batch_total)
            .await
    }
}

async fn check_business_allowed(
    policy_resolver: &Arc<dyn ConnectionPolicyResolverPort>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    peer_id: &str,
    direction: ProtocolDirection,
) -> Result<ResolvedConnectionPolicy> {
    let peer = uc_core::PeerId::from(peer_id);
    match policy_resolver.resolve_for_peer(&peer).await {
        Ok(resolved) => {
            if resolved.allowed.allows(ProtocolKind::Business) {
                Ok(resolved)
            } else {
                emit_protocol_denied(
                    event_tx,
                    peer_id.to_string(),
                    BUSINESS_PROTOCOL_ID,
                    resolved.pairing_state,
                    direction,
                    uc_core::network::ProtocolDenyReason::NotTrusted,
                )
                .await;
                Err(anyhow!("business protocol denied"))
            }
        }
        Err(err) => {
            emit_protocol_denied(
                event_tx,
                peer_id.to_string(),
                BUSINESS_PROTOCOL_ID,
                PairingState::Pending,
                direction,
                uc_core::network::ProtocolDenyReason::RepoError,
            )
            .await;
            Err(anyhow!("policy resolver failed: {err}"))
        }
    }
}

fn listen_on_swarm(
    swarm: &mut libp2p::swarm::Swarm<Libp2pBehaviour>,
    listen_addr: Multiaddr,
) -> Result<()> {
    if let Err(e) = swarm.listen_on(listen_addr.clone()) {
        let message = format!("failed to listen on {listen_addr}: {e}");
        warn!(
            event = "network.listen_register_failed",
            listen_addr = %listen_addr,
            transport = transport_label(&listen_addr),
            error = %e,
            "{message}"
        );
        return Err(anyhow!(message));
    }

    info!(
        event = "network.listen_registered",
        listen_addr = %listen_addr,
        transport = transport_label(&listen_addr),
        "registered listen address with swarm"
    );

    Ok(())
}

/// Attempts a non-blocking send of a `NetworkEvent` into the provided `mpsc::Sender`,
/// logging a warning if the send fails.
///
/// # Examples
///
/// ```ignore
/// use tokio::sync::mpsc;
///
/// let (tx, mut _rx) = mpsc::channel(1);
/// try_send_event(&tx, NetworkEvent::Error("oops".into()), "test").unwrap();
///
/// drop(_rx);
/// assert!(try_send_event(&tx, NetworkEvent::Error("again".into()), "test").is_err());
/// ```
///
/// # Returns
///
/// `Ok(())` on success, `Err(TrySendError<NetworkEvent>)` if the channel is full or closed.
fn try_send_event(
    event_tx: &mpsc::Sender<NetworkEvent>,
    event: NetworkEvent,
    label: &str,
) -> Result<(), mpsc::error::TrySendError<NetworkEvent>> {
    event_tx.try_send(event).map_err(|err| {
        warn!("failed to send {label} event: {err}");
        err
    })
}

async fn snapshot_pairing_open_success(
    caches: &Arc<RwLock<PeerCaches>>,
    peer_id: &str,
    dial_decision: &str,
    attempt_started_at: DateTime<Utc>,
) -> PeerAddressSnapshot {
    for attempt in 0..PAIRING_OPEN_SUCCESS_OBSERVATION_POLL_ATTEMPTS {
        let snapshot = {
            let caches = caches.read().await;
            snapshot_peer_addresses(&caches, peer_id, Utc::now())
        };
        let has_current_attempt_dial = snapshot
            .last_dial_observed_at
            .is_some_and(|observed_at| observed_at >= attempt_started_at);
        if dial_decision == "reuse_existing_connection"
            || has_current_attempt_dial
            || attempt + 1 == PAIRING_OPEN_SUCCESS_OBSERVATION_POLL_ATTEMPTS
        {
            return snapshot;
        }
        sleep(PAIRING_OPEN_SUCCESS_OBSERVATION_POLL_INTERVAL).await;
    }

    let caches = caches.read().await;
    snapshot_peer_addresses(&caches, peer_id, Utc::now())
}
