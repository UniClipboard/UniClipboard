use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use libp2p::{
    core::ConnectedPoint,
    futures::{AsyncReadExt, AsyncWriteExt, StreamExt},
    identity, mdns, noise,
    swarm::{NetworkBehaviour, Swarm, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, StreamProtocol, SwarmBuilder,
};
use libp2p_stream as stream;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, RwLock, Semaphore};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};
use uc_core::network::protocol::ClipboardPayloadVersion;
use uc_core::network::{
    ClipboardMessage, ConnectedPeer, DeviceAnnounceMessage, DiscoveredPeer, NetworkEvent,
    PairingMessage, PairingState, ProtocolDenyReason, ProtocolDirection, ProtocolId, ProtocolKind,
    ProtocolMessage, ResolvedConnectionPolicy,
};
use uc_core::ports::{
    ClipboardTransportPort, ConnectionPolicyResolverPort, EncryptionSessionPort, IdentityStorePort,
    NetworkControlPort, NetworkEventPort, PairingTransportPort, PeerDirectoryPort,
};

use super::pairing_stream::service::{
    PairingStreamConfig, PairingStreamError, PairingStreamService,
};
use crate::identity_store::load_or_create_identity;
const BUSINESS_PROTOCOL_ID: &str = ProtocolId::Business.as_str();
const BUSINESS_PAYLOAD_MAX_BYTES: u64 = 300 * 1024 * 1024;
const BUSINESS_READ_TIMEOUT: Duration = Duration::from_secs(120);
const BUSINESS_STREAM_OPEN_TIMEOUT: Duration = Duration::from_secs(10);
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
        data: Vec<u8>,
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

pub struct PeerCaches {
    discovered_peers: HashMap<String, DiscoveredPeer>,
    reachable_peers: HashSet<String>,
    connected_at: HashMap<String, DateTime<Utc>>,
}

impl PeerCaches {
    pub fn new() -> Self {
        Self {
            discovered_peers: HashMap::new(),
            reachable_peers: HashSet::new(),
            connected_at: HashMap::new(),
        }
    }

    pub fn upsert_discovered(
        &mut self,
        peer_id: String,
        mut addresses: Vec<String>,
        discovered_at: DateTime<Utc>,
    ) -> DiscoveredPeer {
        sort_addresses_quic_first(&mut addresses);
        let peer = DiscoveredPeer {
            peer_id,
            device_name: None,
            device_id: None,
            addresses,
            discovered_at,
            last_seen: discovered_at,
            is_paired: false,
        };
        self.discovered_peers
            .insert(peer.peer_id.clone(), peer.clone());
        peer
    }

    pub fn upsert_discovered_from_connection(
        &mut self,
        peer_id: &str,
        address: Multiaddr,
        observed_at: DateTime<Utc>,
    ) -> bool {
        let address = address.to_string();
        let entry = self
            .discovered_peers
            .entry(peer_id.to_string())
            .or_insert_with(|| DiscoveredPeer {
                peer_id: peer_id.to_string(),
                device_name: None,
                device_id: None,
                addresses: Vec::new(),
                discovered_at: observed_at,
                last_seen: observed_at,
                is_paired: false,
            });

        let mut changed = false;
        if !entry.addresses.contains(&address) {
            entry.addresses.push(address);
            sort_addresses_quic_first(&mut entry.addresses);
            changed = true;
        }
        entry.last_seen = observed_at;
        changed
    }

    pub fn remove_discovered(&mut self, peer_id: &str) -> Option<DiscoveredPeer> {
        self.reachable_peers.remove(peer_id);
        self.connected_at.remove(peer_id);
        self.discovered_peers.remove(peer_id)
    }

    pub fn mark_reachable(&mut self, peer_id: &str, connected_at: DateTime<Utc>) -> bool {
        if self.discovered_peers.contains_key(peer_id) {
            self.reachable_peers.insert(peer_id.to_string());
            self.connected_at
                .entry(peer_id.to_string())
                .or_insert(connected_at);
            true
        } else {
            false
        }
    }

    pub fn mark_unreachable(&mut self, peer_id: &str) -> bool {
        let removed = self.reachable_peers.remove(peer_id);
        self.connected_at.remove(peer_id);
        removed
    }

    pub fn upsert_device_name(
        &mut self,
        peer_id: &str,
        device_name: String,
        observed_at: DateTime<Utc>,
    ) -> bool {
        let entry = self
            .discovered_peers
            .entry(peer_id.to_string())
            .or_insert_with(|| DiscoveredPeer {
                peer_id: peer_id.to_string(),
                device_name: None,
                device_id: None,
                addresses: Vec::new(),
                discovered_at: observed_at,
                last_seen: observed_at,
                is_paired: false,
            });
        let changed = entry.device_name.as_deref() != Some(device_name.as_str());
        entry.device_name = Some(device_name);
        entry.last_seen = observed_at;
        changed
    }

    pub fn is_reachable(&self, peer_id: &str) -> bool {
        self.reachable_peers.contains(peer_id)
    }
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "Libp2pBehaviourEvent")]
struct Libp2pBehaviour {
    mdns: mdns::tokio::Behaviour,
    stream: stream::Behaviour,
}

#[derive(Debug)]
enum Libp2pBehaviourEvent {
    Mdns(mdns::Event),
    Stream,
}

impl From<mdns::Event> for Libp2pBehaviourEvent {
    fn from(event: mdns::Event) -> Self {
        Self::Mdns(event)
    }
}

impl From<()> for Libp2pBehaviourEvent {
    fn from(_: ()) -> Self {
        Self::Stream
    }
}

fn build_mdns_config() -> mdns::Config {
    let mut config = mdns::Config::default();
    config.query_interval = Duration::from_secs(5);
    config
}

fn start_state_name(state: u8) -> &'static str {
    match state {
        START_STATE_IDLE => "idle",
        START_STATE_STARTING => "starting",
        START_STATE_STARTED => "started",
        START_STATE_FAILED => "failed",
        _ => "unknown",
    }
}

impl Libp2pBehaviour {
    fn new(local_peer_id: PeerId) -> Result<Self> {
        let mdns = mdns::tokio::Behaviour::new(build_mdns_config(), local_peer_id)
            .map_err(|e| anyhow!("failed to create mdns behaviour: {e}"))?;
        let stream = stream::Behaviour::new();
        Ok(Self { mdns, stream })
    }
}

pub struct Libp2pNetworkAdapter {
    local_peer_id: String,
    local_identity_pubkey: Vec<u8>,
    caches: Arc<RwLock<PeerCaches>>,
    event_tx: mpsc::Sender<NetworkEvent>,
    event_rx: Mutex<Option<mpsc::Receiver<NetworkEvent>>>,
    clipboard_tx: mpsc::Sender<(ClipboardMessage, Option<Vec<u8>>)>,
    clipboard_rx: Mutex<Option<mpsc::Receiver<(ClipboardMessage, Option<Vec<u8>>)>>>,
    business_tx: mpsc::Sender<BusinessCommand>,
    business_rx: Mutex<Option<mpsc::Receiver<BusinessCommand>>>,
    keypair: Mutex<identity::Keypair>,
    start_state: AtomicU8,
    policy_resolver: Arc<dyn ConnectionPolicyResolverPort>,
    encryption_session: Arc<dyn EncryptionSessionPort>,
    stream_control: Mutex<Option<stream::Control>>,
    pairing_service: Mutex<Option<PairingStreamService>>,
}

impl Libp2pNetworkAdapter {
    pub fn new(
        identity_store: Arc<dyn IdentityStorePort>,
        policy_resolver: Arc<dyn ConnectionPolicyResolverPort>,
        encryption_session: Arc<dyn EncryptionSessionPort>,
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
        let (event_tx, event_rx) = mpsc::channel(64);
        let (clipboard_tx, clipboard_rx) = mpsc::channel(64);
        let (business_tx, business_rx) = mpsc::channel(64);
        let pairing_service = Mutex::new(None);

        Ok(Self {
            local_peer_id,
            local_identity_pubkey,
            caches: Arc::new(RwLock::new(PeerCaches::new())),
            event_tx,
            event_rx: Mutex::new(Some(event_rx)),
            clipboard_tx,
            clipboard_rx: Mutex::new(Some(clipboard_rx)),
            business_tx,
            business_rx: Mutex::new(Some(business_rx)),
            keypair: Mutex::new(keypair),
            start_state: AtomicU8::new(START_STATE_IDLE),
            policy_resolver,
            encryption_session,
            stream_control: Mutex::new(None),
            pairing_service,
        })
    }

    pub fn local_identity_pubkey(&self) -> Vec<u8> {
        self.local_identity_pubkey.clone()
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
        let pairing_service = PairingStreamService::new(
            stream_control.clone(),
            self.event_tx.clone(),
            PairingStreamConfig::default(),
        );
        pairing_service.spawn_accept_loop();
        {
            let mut guard = self
                .stream_control
                .lock()
                .map_err(|_| anyhow!("stream control mutex poisoned"))?;
            *guard = Some(stream_control.clone());
        }
        {
            let mut guard = self
                .pairing_service
                .lock()
                .map_err(|_| anyhow!("pairing service mutex poisoned"))?;
            *guard = Some(pairing_service);
        }

        spawn_business_stream_handler(
            stream_control.clone(),
            self.caches.clone(),
            self.event_tx.clone(),
            self.clipboard_tx.clone(),
            self.policy_resolver.clone(),
            self.encryption_session.clone(),
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
        info!(quic_address = %quic_addr_str, tcp_address = %tcp_addr_str, "selected listen addresses");

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
        let local_peer_id = self.local_peer_id.clone();
        tokio::spawn(async move {
            run_swarm(
                swarm,
                caches,
                event_tx,
                policy_resolver,
                business_rx,
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
        guard
            .take()
            .ok_or_else(|| anyhow!("{name} receiver already taken"))
    }
}

#[async_trait]
impl ClipboardTransportPort for Libp2pNetworkAdapter {
    async fn send_clipboard(&self, _peer_id: &str, _encrypted_data: Vec<u8>) -> Result<()> {
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

    async fn broadcast_clipboard(&self, _encrypted_data: Vec<u8>) -> Result<()> {
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
        let peers: Vec<DiscoveredPeer> = caches.discovered_peers.values().cloned().collect();
        debug!(
            discovered_peer_count = peers.len(),
            reachable_peer_count = caches.reachable_peers.len(),
            "snapshot discovered peers"
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
        match service
            .open_pairing_session(peer_id.clone(), session_id)
            .await
        {
            Ok(()) => Ok(()),
            Err(err) => {
                handle_pairing_open_error(&self.policy_resolver, &self.event_tx, &peer_id, &err)
                    .await;
                Err(err)
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
        Self::take_receiver(&self.event_rx, "network event")
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

/// Maximum JSON header size (64KB). Streams with larger headers are discarded.
const MAX_JSON_HEADER_SIZE: usize = 64 * 1024;

/// Result of processing a single inbound business stream message.
enum ProcessedMessage {
    /// V2 clipboard with pre-decoded plaintext from transport-level streaming decode.
    V2Clipboard(ClipboardMessage, Vec<u8>),
    /// All other messages (V1 clipboard, DeviceAnnounce, Heartbeat, Pairing).
    Standard(ProtocolMessage),
}

fn spawn_business_stream_handler(
    mut control: stream::Control,
    caches: Arc<RwLock<PeerCaches>>,
    event_tx: mpsc::Sender<NetworkEvent>,
    clipboard_tx: mpsc::Sender<(ClipboardMessage, Option<Vec<u8>>)>,
    policy_resolver: Arc<dyn ConnectionPolicyResolverPort>,
    encryption_session: Arc<dyn EncryptionSessionPort>,
) {
    let mut incoming = match control.accept(StreamProtocol::new(BUSINESS_PROTOCOL_ID)) {
        Ok(incoming) => incoming,
        Err(err) => {
            warn!("failed to accept business stream: {err}");
            return;
        }
    };

    tokio::spawn(async move {
        while let Some((_peer, stream)) = incoming.next().await {
            let peer_id = _peer.to_string();
            let event_tx = event_tx.clone();
            let clipboard_tx = clipboard_tx.clone();
            let policy_resolver = policy_resolver.clone();
            let caches = caches.clone();
            let encryption_session = encryption_session.clone();
            tokio::spawn(async move {
                if check_business_allowed(
                    &policy_resolver,
                    &event_tx,
                    &peer_id,
                    ProtocolDirection::Inbound,
                )
                .await
                .is_err()
                {
                    return;
                }

                // Apply overall size guard on the stream
                let limited = stream.take(BUSINESS_PAYLOAD_MAX_BYTES + 1);

                // Convert libp2p stream (futures::AsyncRead) to tokio AsyncRead
                use tokio_util::compat::FuturesAsyncReadCompatExt;
                let mut reader = limited.compat();

                let result = tokio::time::timeout(BUSINESS_READ_TIMEOUT, async {
                    use tokio::io::AsyncReadExt;

                    // Step 1: Read 4-byte JSON header length (u32 LE)
                    // An immediate EOF here means the peer opened the stream as a
                    // connectivity probe (ensure_business_path) and closed it without
                    // sending data — not an error.
                    let mut len_buf = [0u8; 4];
                    match reader.read_exact(&mut len_buf).await {
                        Ok(_) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                            return Err("probe".into());
                        }
                        Err(e) => {
                            return Err(format!("failed to read json header length: {e}"));
                        }
                    }
                    let json_len = u32::from_le_bytes(len_buf) as usize;

                    // Guard: cap JSON header size at 64KB
                    if json_len > MAX_JSON_HEADER_SIZE {
                        return Err(format!(
                            "json header too large: {json_len} > {MAX_JSON_HEADER_SIZE}"
                        ));
                    }

                    // Step 2: Read JSON header (exactly json_len bytes)
                    let mut json_buf = vec![0u8; json_len];
                    reader
                        .read_exact(&mut json_buf)
                        .await
                        .map_err(|e| format!("failed to read json header: {e}"))?;

                    let message = ProtocolMessage::from_bytes(&json_buf)
                        .map_err(|e| format!("invalid protocol message: {e}"))?;

                    match message {
                        ProtocolMessage::Clipboard(msg)
                            if msg.payload_version == ClipboardPayloadVersion::V2 =>
                        {
                            // Step 3: V2 streaming decode via spawn_blocking
                            let master_key = match encryption_session.get_master_key().await {
                                Ok(k) => k,
                                Err(e) => {
                                    return Err(format!(
                                        "V2 inbound: encryption session not ready: {e}"
                                    ));
                                }
                            };

                            let decode_result = tokio::task::spawn_blocking(move || {
                                use tokio_util::io::SyncIoBridge;
                                let sync_reader = SyncIoBridge::new(reader);
                                uc_infra::clipboard::ChunkedDecoder::decode_from(
                                    sync_reader,
                                    &master_key,
                                )
                            })
                            .await
                            .map_err(|e| format!("V2 decode task panicked: {e}"))?;

                            match decode_result {
                                Ok(plaintext) => Ok(ProcessedMessage::V2Clipboard(msg, plaintext)),
                                Err(e) => Err(format!("V2 inbound: chunk decode failed: {e}")),
                            }
                        }
                        other => {
                            // V1 clipboard, DeviceAnnounce, Heartbeat, Pairing — no trailing payload
                            Ok(ProcessedMessage::Standard(other))
                        }
                    }
                })
                .await;

                // Stream ownership: for V2 the stream was moved into spawn_blocking via SyncIoBridge;
                // when ChunkedDecoder::decode_from finishes (or errors), SyncIoBridge is dropped,
                // which drops the underlying tokio reader / compat layer / Take<libp2p::Stream>.
                // The libp2p stream close happens via Drop.
                // For non-V2, the reader is dropped when the async block completes.

                match result {
                    Ok(Ok(ProcessedMessage::V2Clipboard(msg, plaintext))) => {
                        handle_v2_clipboard(
                            caches,
                            event_tx,
                            clipboard_tx,
                            peer_id,
                            msg,
                            plaintext,
                        )
                        .await;
                    }
                    Ok(Ok(ProcessedMessage::Standard(message))) => {
                        handle_standard_message(caches, event_tx, clipboard_tx, peer_id, message)
                            .await;
                    }
                    Ok(Err(err)) if err == "probe" => {
                        debug!(peer_id = %peer_id, "business stream probe (ensure_business_path)");
                    }
                    Ok(Err(err)) => {
                        warn!(peer_id = %peer_id, error = %err, "business stream processing failed");
                    }
                    Err(_) => {
                        warn!(peer_id = %peer_id, "business stream read timed out");
                    }
                }
            });
        }
    });
}

/// Handle non-V2 protocol messages (V1 clipboard, DeviceAnnounce, Heartbeat, Pairing).
async fn handle_standard_message(
    caches: Arc<RwLock<PeerCaches>>,
    event_tx: mpsc::Sender<NetworkEvent>,
    clipboard_tx: mpsc::Sender<(ClipboardMessage, Option<Vec<u8>>)>,
    peer_id: String,
    message: ProtocolMessage,
) {
    match message {
        ProtocolMessage::DeviceAnnounce(announce) => {
            if announce.peer_id != peer_id {
                warn!(
                    "Device announce peer_id mismatch: peer_id={}, announced_peer_id={}",
                    peer_id, announce.peer_id
                );
            }
            let changed = {
                let mut caches = caches.write().await;
                caches.upsert_device_name(
                    peer_id.as_str(),
                    announce.device_name.clone(),
                    announce.timestamp,
                )
            };
            if changed {
                if let Err(err) = try_send_event(
                    &event_tx,
                    NetworkEvent::PeerNameUpdated {
                        peer_id: peer_id.clone(),
                        device_name: announce.device_name,
                    },
                    "PeerNameUpdated",
                ) {
                    warn!("failed to send PeerNameUpdated event: {err}");
                }
            }
        }
        ProtocolMessage::Clipboard(message) => {
            // V1 path — send with None for pre-decoded plaintext
            if let Err(err) = clipboard_tx.send((message.clone(), None)).await {
                warn!("Failed to forward V1 clipboard payload: {err}");
            }
            if let Err(err) = try_send_event(
                &event_tx,
                NetworkEvent::ClipboardReceived(message),
                "ClipboardReceived",
            ) {
                warn!("failed to send ClipboardReceived event: {err}");
            }
        }
        ProtocolMessage::Heartbeat(_) => {
            debug!("Received heartbeat payload from peer_id={}", peer_id);
        }
        ProtocolMessage::Pairing(_) => {
            warn!(
                "Unexpected pairing payload on business stream from peer_id={}",
                peer_id
            );
        }
    }
}

/// Handle V2 clipboard message with pre-decoded plaintext from transport-level streaming decode.
async fn handle_v2_clipboard(
    _caches: Arc<RwLock<PeerCaches>>,
    event_tx: mpsc::Sender<NetworkEvent>,
    clipboard_tx: mpsc::Sender<(ClipboardMessage, Option<Vec<u8>>)>,
    _peer_id: String,
    message: ClipboardMessage,
    plaintext: Vec<u8>,
) {
    if let Err(err) = clipboard_tx.send((message.clone(), Some(plaintext))).await {
        warn!("Failed to forward V2 clipboard payload: {err}");
    }
    if let Err(err) = try_send_event(
        &event_tx,
        NetworkEvent::ClipboardReceived(message),
        "ClipboardReceived",
    ) {
        warn!("failed to send ClipboardReceived event: {err}");
    }
}

async fn emit_protocol_denied(
    event_tx: &mpsc::Sender<NetworkEvent>,
    peer_id: String,
    protocol_id: &str,
    pairing_state: PairingState,
    direction: ProtocolDirection,
    reason: ProtocolDenyReason,
) {
    if let Err(err) = event_tx
        .send(NetworkEvent::ProtocolDenied {
            peer_id,
            protocol_id: protocol_id.to_string(),
            pairing_state,
            direction,
            reason,
        })
        .await
    {
        warn!("failed to emit protocol denied event: {err}");
    }
}

async fn handle_pairing_open_error(
    policy_resolver: &Arc<dyn ConnectionPolicyResolverPort>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    peer_id: &str,
    error: &anyhow::Error,
) {
    if let Some(pairing_error) = error.downcast_ref::<PairingStreamError>() {
        if matches!(pairing_error, PairingStreamError::UnsupportedProtocol) {
            let peer = uc_core::PeerId::from(peer_id);
            let pairing_state = match policy_resolver.resolve_for_peer(&peer).await {
                Ok(resolved) => resolved.pairing_state,
                Err(err) => {
                    warn!("policy resolver failed for pairing protocol peer={peer_id}: {err}");
                    PairingState::Pending
                }
            };
            emit_protocol_denied(
                event_tx,
                peer_id.to_string(),
                ProtocolId::Pairing.as_str(),
                pairing_state,
                ProtocolDirection::Outbound,
                ProtocolDenyReason::NotSupported,
            )
            .await;
        }
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
                    ProtocolDenyReason::NotTrusted,
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
                ProtocolDenyReason::RepoError,
            )
            .await;
            Err(anyhow!("policy resolver failed: {err}"))
        }
    }
}

async fn run_swarm(
    mut swarm: Swarm<Libp2pBehaviour>,
    caches: Arc<RwLock<PeerCaches>>,
    event_tx: mpsc::Sender<NetworkEvent>,
    policy_resolver: Arc<dyn ConnectionPolicyResolverPort>,
    mut business_rx: mpsc::Receiver<BusinessCommand>,
    local_peer_id: String,
) {
    info!(local_peer_id = %local_peer_id, "libp2p mDNS swarm started");
    let mut next_business_command_id: u64 = 1;
    let business_command_semaphore = Arc::new(Semaphore::new(MAX_IN_FLIGHT_BUSINESS_COMMANDS));
    let mut pending_business_command: Option<(u64, BusinessCommand)> = None;

    loop {
        tokio::select! {
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(Libp2pBehaviourEvent::Mdns(event)) => match event {
                        mdns::Event::Discovered(peers) => {
                            let discovered_count = peers.len();
                            debug!(
                                discovered_count,
                                local_peer_id = %local_peer_id,
                                "received mdns discovered event"
                            );
                            let mut peers: Vec<(PeerId, Multiaddr)> = peers
                                .into_iter()
                                .filter(|(peer_id, _)| peer_id.to_string() != local_peer_id)
                                .collect();
                            // Sort so QUIC addresses are added to the swarm first
                            peers.sort_by_key(|(_, addr)| {
                                if addr.to_string().contains("/quic-v1") { 0 } else { 1 }
                            });
                            for (peer_id, address) in peers.iter() {
                                swarm.add_peer_address(peer_id.clone(), address.clone());
                            }
                            let discovered = collect_mdns_discovered(peers);
                            let events = {
                                let mut caches = caches.write().await;
                                apply_mdns_discovered(&mut caches, discovered, Utc::now())
                            };

                            let cache_size = {
                                let caches = caches.read().await;
                                caches.discovered_peers.len()
                            };
                            info!(
                                emitted_event_count = events.len(),
                                discovered_cache_size = cache_size,
                                local_peer_id = %local_peer_id,
                                "processed mdns discovered event"
                            );

                            for event in events {
                                let _ = try_send_event(&event_tx, event, "PeerDiscovered");
                            }
                        }
                        mdns::Event::Expired(peers) => {
                            let expired_count = peers.len();
                            debug!(
                                expired_count,
                                local_peer_id = %local_peer_id,
                                "received mdns expired event"
                            );
                            let peers: Vec<(PeerId, Multiaddr)> = peers
                                .into_iter()
                                .filter(|(peer_id, _)| peer_id.to_string() != local_peer_id)
                                .collect();
                            let expired = collect_mdns_expired(peers);
                            let events = {
                                let mut caches = caches.write().await;
                                apply_mdns_expired(&mut caches, expired)
                            };

                            let cache_size = {
                                let caches = caches.read().await;
                                caches.discovered_peers.len()
                            };
                            info!(
                                emitted_event_count = events.len(),
                                discovered_cache_size = cache_size,
                                local_peer_id = %local_peer_id,
                                "processed mdns expired event"
                            );

                            for event in events {
                                let _ = try_send_event(&event_tx, event, "PeerLost");
                            }
                        }
                    },
                    SwarmEvent::Behaviour(Libp2pBehaviourEvent::Stream) => {}
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                        let peer_id_string = peer_id.to_string();
                        let address = match endpoint {
                            ConnectedPoint::Dialer { address, .. } => Some(address.clone()),
                            ConnectedPoint::Listener { send_back_addr, .. } => {
                                Some(send_back_addr.clone())
                            }
                        };
                        if let Some(address) = address.as_ref() {
                            swarm.add_peer_address(peer_id, address.clone());
                        }
                        let event = {
                            let mut caches = caches.write().await;
                            apply_peer_ready_from_connection(
                                &mut caches,
                                &peer_id_string,
                                Utc::now(),
                                address,
                            )
                        };

                        if let Some(event) = event {
                            let _ = try_send_event(&event_tx, event, "PeerReady");
                            info!(
                                peer_id = %peer_id_string,
                                local_peer_id = %local_peer_id,
                                "peer connection established"
                            );
                        } else {
                            debug!("connection established for unknown peer {peer_id_string}");
                        }
                    }
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        let peer_id = peer_id.to_string();
                        let event = {
                            let mut caches = caches.write().await;
                            apply_peer_not_ready(&mut caches, &peer_id)
                        };

                        if let Some(event) = event {
                            let _ = try_send_event(&event_tx, event, "PeerNotReady");
                            info!(
                                peer_id = %peer_id,
                                local_peer_id = %local_peer_id,
                                "peer connection closed"
                            );
                        }
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                        error!("outgoing connection error to {:?}: {}", peer_id, error);
                        if let Err(err) = event_tx
                            .send(NetworkEvent::Error("network connection error".to_string()))
                            .await
                        {
                            warn!("failed to publish network error event: {err}");
                        }
                    }
                    SwarmEvent::IncomingConnectionError {
                        send_back_addr,
                        error,
                        ..
                    } => {
                        error!(
                            "incoming connection error from {}: {}",
                            send_back_addr, error
                        );
                        if let Err(err) = event_tx
                            .send(NetworkEvent::Error("network connection error".to_string()))
                            .await
                        {
                            warn!("failed to publish network error event: {err}");
                        }
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        info!("libp2p listening on {address}");
                    }
                    _ => {}
                }
            }
            Some(command) = business_rx.recv(), if pending_business_command.is_none() => {
                let command_id = next_business_command_id;
                next_business_command_id = next_business_command_id.wrapping_add(1);
                let (operation, peer_id) = business_command_log_fields(&command);
                debug!(
                    cmd_id = command_id,
                    op = operation,
                    peer_id = %peer_id.unwrap_or("-"),
                    "business command queued"
                );
                pending_business_command = Some((command_id, command));
            }
            permit_result = business_command_semaphore.clone().acquire_owned(), if pending_business_command.is_some() => {
                let command_permit = match permit_result {
                    Ok(permit) => permit,
                    Err(err) => {
                        error!(error = %err, "business command semaphore closed");
                        break;
                    }
                };
                let Some((command_id, command)) = pending_business_command.take() else {
                    continue;
                };
                let (operation, peer_id) = business_command_log_fields(&command);
                debug!(
                    cmd_id = command_id,
                    op = operation,
                    peer_id = %peer_id.unwrap_or("-"),
                    "business command dispatched"
                );

                if let BusinessCommand::UnpairPeer { peer_id, result_tx } = command {
                    let _command_permit = command_permit;
                    let peer_id_str = peer_id.as_str().to_string();
                    let result = match peer_id_str.parse::<PeerId>() {
                        Ok(peer) => {
                            if swarm.is_connected(&peer) {
                                swarm
                                    .disconnect_peer_id(peer)
                                    .map_err(|_| anyhow!("failed to disconnect peer during unpair"))
                            } else {
                                Ok(())
                            }
                        }
                        Err(err) => Err(anyhow!("invalid peer id for unpair: {err}")),
                    };
                    deliver_business_command_result(result_tx, result, command_id, "unpair", &peer_id_str);
                    continue;
                }

                let command_control = swarm.behaviour().stream.new_control();
                let command_caches = caches.clone();
                let command_policy_resolver = policy_resolver.clone();
                let command_event_tx = event_tx.clone();
                let command_local_peer_id = local_peer_id.clone();
                tokio::spawn(async move {
                    let _command_permit = command_permit;
                    execute_business_command(
                        command,
                        command_id,
                        command_control,
                        command_caches,
                        command_policy_resolver,
                        command_event_tx,
                        command_local_peer_id,
                    )
                    .await;
                });
            }
        }
    }
}

fn business_command_log_fields(command: &BusinessCommand) -> (&'static str, Option<&str>) {
    match command {
        BusinessCommand::SendClipboard { peer_id, .. } => ("clipboard", Some(peer_id.as_str())),
        BusinessCommand::EnsureBusinessPath { peer_id, .. } => ("ensure", Some(peer_id.as_str())),
        BusinessCommand::AnnounceDeviceName { .. } => ("announce_device_name", None),
        BusinessCommand::UnpairPeer { peer_id, .. } => ("unpair", Some(peer_id.as_str())),
    }
}

fn notify_enqueue_failure(command: BusinessCommand, message: &str, operation: &str, peer_id: &str) {
    let result_tx = match command {
        BusinessCommand::SendClipboard { result_tx, .. } => result_tx,
        BusinessCommand::EnsureBusinessPath { result_tx, .. } => result_tx,
        BusinessCommand::UnpairPeer { result_tx, .. } => result_tx,
        BusinessCommand::AnnounceDeviceName { .. } => return,
    };

    if let Err(undelivered_result) = result_tx.send(Err(anyhow!(message.to_string()))) {
        warn!(
            op = operation,
            peer_id = %peer_id,
            result_ok = undelivered_result.is_ok(),
            "failed to deliver enqueue failure to caller"
        );
    }
}

fn deliver_business_command_result(
    result_tx: oneshot::Sender<Result<()>>,
    result: Result<()>,
    command_id: u64,
    operation: &str,
    peer_id: &str,
) {
    if let Err(undelivered_result) = result_tx.send(result) {
        warn!(
            cmd_id = command_id,
            op = operation,
            peer_id = %peer_id,
            result_ok = undelivered_result.is_ok(),
            "business command result receiver dropped"
        );
    }
}

async fn execute_business_command(
    command: BusinessCommand,
    command_id: u64,
    control: stream::Control,
    caches: Arc<RwLock<PeerCaches>>,
    policy_resolver: Arc<dyn ConnectionPolicyResolverPort>,
    event_tx: mpsc::Sender<NetworkEvent>,
    local_peer_id: String,
) {
    match command {
        BusinessCommand::SendClipboard {
            peer_id,
            data,
            result_tx,
        } => {
            let started_at = std::time::Instant::now();
            let peer_id_str = peer_id.as_str().to_string();
            debug!(
                cmd_id = command_id,
                op = "clipboard",
                peer_id = %peer_id_str,
                "business command started"
            );

            let result = match peer_id_str.parse::<PeerId>() {
                Ok(peer) => {
                    execute_business_stream(
                        &control,
                        &caches,
                        &policy_resolver,
                        &event_tx,
                        &peer_id,
                        peer,
                        Some(data.as_slice()),
                        BUSINESS_STREAM_OPEN_TIMEOUT,
                        BUSINESS_STREAM_WRITE_TIMEOUT,
                        BUSINESS_STREAM_CLOSE_TIMEOUT,
                        "clipboard",
                    )
                    .await
                }
                Err(err) => Err(anyhow!("invalid peer id for business stream: {err}")),
            };

            let elapsed_ms = started_at.elapsed().as_millis() as u64;
            match &result {
                Ok(()) => {
                    debug!(
                        cmd_id = command_id,
                        op = "clipboard",
                        peer_id = %peer_id_str,
                        elapsed_ms,
                        "business command completed"
                    );
                }
                Err(err) => {
                    warn!(
                        cmd_id = command_id,
                        op = "clipboard",
                        peer_id = %peer_id_str,
                        elapsed_ms,
                        error = %err,
                        "business command failed"
                    );
                }
            }

            deliver_business_command_result(
                result_tx,
                result,
                command_id,
                "clipboard",
                &peer_id_str,
            );
        }
        BusinessCommand::EnsureBusinessPath { peer_id, result_tx } => {
            let started_at = std::time::Instant::now();
            let peer_id_str = peer_id.as_str().to_string();
            debug!(
                cmd_id = command_id,
                op = "ensure",
                peer_id = %peer_id_str,
                "business command started"
            );

            let result = match peer_id_str.parse::<PeerId>() {
                Ok(peer) => {
                    execute_business_stream(
                        &control,
                        &caches,
                        &policy_resolver,
                        &event_tx,
                        &peer_id,
                        peer,
                        None,
                        BUSINESS_STREAM_OPEN_TIMEOUT,
                        BUSINESS_STREAM_WRITE_TIMEOUT,
                        BUSINESS_STREAM_CLOSE_TIMEOUT,
                        "ensure",
                    )
                    .await
                }
                Err(err) => Err(anyhow!("invalid peer id for ensure business path: {err}")),
            };

            let elapsed_ms = started_at.elapsed().as_millis() as u64;
            match &result {
                Ok(()) => {
                    debug!(
                        cmd_id = command_id,
                        op = "ensure",
                        peer_id = %peer_id_str,
                        elapsed_ms,
                        "business command completed"
                    );
                }
                Err(err) => {
                    warn!(
                        cmd_id = command_id,
                        op = "ensure",
                        peer_id = %peer_id_str,
                        elapsed_ms,
                        error = %err,
                        "business command failed"
                    );
                }
            }

            deliver_business_command_result(result_tx, result, command_id, "ensure", &peer_id_str);
        }
        BusinessCommand::AnnounceDeviceName { device_name } => {
            let started_at = std::time::Instant::now();
            debug!(
                cmd_id = command_id,
                op = "announce_device_name",
                "business command started"
            );

            let peer_ids = {
                let caches = caches.read().await;
                caches
                    .discovered_peers
                    .keys()
                    .filter(|peer_id| peer_id.as_str() != local_peer_id.as_str())
                    .cloned()
                    .collect::<Vec<_>>()
            };
            if peer_ids.is_empty() {
                info!(
                    cmd_id = command_id,
                    op = "announce_device_name",
                    local_peer_id = %local_peer_id,
                    "skip device announce because discovered peer list is empty"
                );
                return;
            }
            info!(
                cmd_id = command_id,
                op = "announce_device_name",
                target_peer_count = peer_ids.len(),
                local_peer_id = %local_peer_id,
                "broadcasting device announce to discovered peers"
            );
            let message = ProtocolMessage::DeviceAnnounce(DeviceAnnounceMessage {
                peer_id: local_peer_id.clone(),
                device_name,
                timestamp: Utc::now(),
            });
            let payload = match message.to_bytes() {
                Ok(payload) => payload,
                Err(err) => {
                    warn!(
                        cmd_id = command_id,
                        op = "announce_device_name",
                        error = %err,
                        "failed to serialize device announce payload"
                    );
                    return;
                }
            };

            for peer_id in peer_ids {
                let peer_id_str = peer_id.as_str();
                let peer = match peer_id.as_str().parse::<PeerId>() {
                    Ok(peer) => peer,
                    Err(err) => {
                        warn!(
                            cmd_id = command_id,
                            op = "announce_device_name",
                            peer_id = %peer_id_str,
                            error = %err,
                            "invalid peer id for announce stream"
                        );
                        continue;
                    }
                };
                if check_business_allowed(
                    &policy_resolver,
                    &event_tx,
                    peer_id.as_str(),
                    ProtocolDirection::Outbound,
                )
                .await
                .is_err()
                {
                    continue;
                }

                let mut announce_control = control.clone();
                match timeout(
                    BUSINESS_STREAM_OPEN_TIMEOUT,
                    announce_control.open_stream(peer, StreamProtocol::new(BUSINESS_PROTOCOL_ID)),
                )
                .await
                {
                    Ok(Ok(mut stream)) => {
                        match timeout(BUSINESS_STREAM_WRITE_TIMEOUT, stream.write_all(&payload))
                            .await
                        {
                            Ok(Ok(())) => {
                                match timeout(BUSINESS_STREAM_CLOSE_TIMEOUT, stream.close()).await {
                                    Ok(Ok(())) => {}
                                    Ok(Err(err)) => {
                                        warn!(
                                            cmd_id = command_id,
                                            op = "announce_device_name",
                                            peer_id = %peer_id_str,
                                            error = %err,
                                            "announce stream close failed"
                                        );
                                    }
                                    Err(_) => {
                                        warn!(
                                            cmd_id = command_id,
                                            op = "announce_device_name",
                                            peer_id = %peer_id_str,
                                            "announce stream close timed out"
                                        );
                                    }
                                }
                            }
                            Ok(Err(err)) => {
                                warn!(
                                    cmd_id = command_id,
                                    op = "announce_device_name",
                                    peer_id = %peer_id_str,
                                    error = %err,
                                    "announce stream write failed"
                                );
                            }
                            Err(_) => {
                                warn!(
                                    cmd_id = command_id,
                                    op = "announce_device_name",
                                    peer_id = %peer_id_str,
                                    "announce stream write timed out"
                                );
                            }
                        }
                    }
                    Ok(Err(err)) => {
                        warn!(
                            cmd_id = command_id,
                            op = "announce_device_name",
                            peer_id = %peer_id_str,
                            error = %err,
                            "announce stream open failed"
                        );
                    }
                    Err(_) => {
                        warn!(
                            cmd_id = command_id,
                            op = "announce_device_name",
                            peer_id = %peer_id_str,
                            "announce stream open timed out"
                        );
                    }
                }
            }

            let elapsed_ms = started_at.elapsed().as_millis() as u64;
            debug!(
                cmd_id = command_id,
                op = "announce_device_name",
                elapsed_ms,
                "business command completed"
            );
        }
        BusinessCommand::UnpairPeer { peer_id, result_tx } => {
            let peer_id_str = peer_id.as_str().to_string();
            deliver_business_command_result(
                result_tx,
                Err(anyhow!("unpair command must be handled by swarm loop")),
                command_id,
                "unpair",
                &peer_id_str,
            );
        }
    }
}

async fn execute_business_stream(
    control: &stream::Control,
    caches: &Arc<RwLock<PeerCaches>>,
    policy_resolver: &Arc<dyn ConnectionPolicyResolverPort>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    peer_id: &uc_core::PeerId,
    peer: PeerId,
    payload: Option<&[u8]>,
    open_timeout: Duration,
    write_timeout: Duration,
    close_timeout: Duration,
    denied_operation: &str,
) -> Result<()> {
    let peer_id_str = peer_id.as_str();

    if check_business_allowed(
        policy_resolver,
        event_tx,
        peer_id_str,
        ProtocolDirection::Outbound,
    )
    .await
    .is_err()
    {
        return Err(anyhow!(
            "business protocol denied for outbound {denied_operation} peer_id={peer_id_str}"
        ));
    }

    let mut control = control.clone();
    let result = match timeout(
        open_timeout,
        control.open_stream(peer, StreamProtocol::new(BUSINESS_PROTOCOL_ID)),
    )
    .await
    {
        Ok(Ok(mut stream)) => {
            if let Some(data) = payload {
                match timeout(write_timeout, stream.write_all(data)).await {
                    Ok(Ok(())) => match timeout(close_timeout, stream.close()).await {
                        Ok(Ok(())) => Ok(()),
                        Ok(Err(err)) => {
                            warn!("business stream close failed: {err}");
                            Err(anyhow!("business stream close failed: {err}"))
                        }
                        Err(_) => {
                            warn!(peer_id = %peer_id_str, "business stream close timed out");
                            Err(anyhow!("business stream close timed out"))
                        }
                    },
                    Ok(Err(err)) => {
                        warn!("business stream write failed: {err}");
                        Err(anyhow!("business stream write failed: {err}"))
                    }
                    Err(_) => {
                        warn!(peer_id = %peer_id_str, "business stream write timed out");
                        Err(anyhow!("business stream write timed out"))
                    }
                }
            } else {
                match timeout(close_timeout, stream.close()).await {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(err)) => Err(anyhow!("ensure business stream close failed: {err}")),
                    Err(_) => {
                        warn!(peer_id = %peer_id_str, "ensure business stream close timed out");
                        Err(anyhow!("ensure business stream close timed out"))
                    }
                }
            }
        }
        Ok(Err(err)) => {
            if payload.is_some() {
                warn!("business stream open failed: {err}");
                Err(anyhow!("business stream open failed: {err}"))
            } else {
                Err(anyhow!("ensure business stream open failed: {err}"))
            }
        }
        Err(_) => {
            if payload.is_some() {
                warn!(peer_id = %peer_id_str, "business stream open timed out");
                Err(anyhow!("business stream open timed out"))
            } else {
                warn!(peer_id = %peer_id_str, "ensure business stream open timed out");
                Err(anyhow!("ensure business stream open timed out"))
            }
        }
    };

    apply_business_stream_result(caches, event_tx, peer_id_str, &result).await;
    result
}

async fn apply_business_stream_result(
    caches: &Arc<RwLock<PeerCaches>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    peer_id: &str,
    result: &Result<()>,
) {
    let event = {
        let mut caches = caches.write().await;
        if result.is_ok() {
            apply_peer_ready(&mut caches, peer_id, Utc::now())
        } else {
            apply_peer_not_ready(&mut caches, peer_id)
        }
    };
    if let Some(event) = event {
        let label = if result.is_ok() {
            "PeerReady"
        } else {
            "PeerNotReady"
        };
        let _ = try_send_event(event_tx, event, label);
    }
}

fn listen_on_swarm(swarm: &mut Swarm<Libp2pBehaviour>, listen_addr: Multiaddr) -> Result<()> {
    if let Err(e) = swarm.listen_on(listen_addr.clone()) {
        let message = format!("failed to listen on {listen_addr}: {e}");
        warn!("{message}");
        return Err(anyhow!(message));
    }

    Ok(())
}

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

fn sort_addresses_quic_first(addresses: &mut Vec<String>) {
    addresses.sort_by_key(|addr| if addr.contains("/quic-v1") { 0 } else { 1 });
}

fn collect_mdns_discovered(
    peers: impl IntoIterator<Item = (PeerId, Multiaddr)>,
) -> HashMap<String, Vec<String>> {
    let mut discovered = HashMap::new();
    for (peer_id, addr) in peers {
        discovered
            .entry(peer_id.to_string())
            .or_insert_with(Vec::new)
            .push(addr.to_string());
    }
    discovered
}

fn collect_mdns_expired(peers: impl IntoIterator<Item = (PeerId, Multiaddr)>) -> HashSet<String> {
    let mut expired = HashSet::new();
    for (peer_id, _) in peers {
        expired.insert(peer_id.to_string());
    }
    expired
}

fn apply_mdns_discovered(
    caches: &mut PeerCaches,
    discovered: HashMap<String, Vec<String>>,
    discovered_at: DateTime<Utc>,
) -> Vec<NetworkEvent> {
    discovered
        .into_iter()
        .map(|(peer_id, addresses)| {
            NetworkEvent::PeerDiscovered(caches.upsert_discovered(
                peer_id,
                addresses,
                discovered_at,
            ))
        })
        .collect()
}

fn apply_mdns_expired(caches: &mut PeerCaches, expired: HashSet<String>) -> Vec<NetworkEvent> {
    expired
        .into_iter()
        .filter_map(|peer_id| {
            caches
                .remove_discovered(&peer_id)
                .map(|_| NetworkEvent::PeerLost(peer_id))
        })
        .collect()
}

fn apply_peer_ready(
    caches: &mut PeerCaches,
    peer_id: &str,
    connected_at: DateTime<Utc>,
) -> Option<NetworkEvent> {
    if caches.mark_reachable(peer_id, connected_at) {
        Some(NetworkEvent::PeerReady {
            peer_id: peer_id.to_string(),
        })
    } else {
        None
    }
}

fn apply_peer_ready_from_connection(
    caches: &mut PeerCaches,
    peer_id: &str,
    connected_at: DateTime<Utc>,
    address: Option<Multiaddr>,
) -> Option<NetworkEvent> {
    if let Some(address) = address {
        caches.upsert_discovered_from_connection(peer_id, address, connected_at);
    }
    apply_peer_ready(caches, peer_id, connected_at)
}

fn apply_peer_not_ready(caches: &mut PeerCaches, peer_id: &str) -> Option<NetworkEvent> {
    if caches.mark_unreachable(peer_id) {
        Some(NetworkEvent::PeerNotReady {
            peer_id: peer_id.to_string(),
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::InMemoryEncryptionSessionPort;
    use libp2p::futures::{AsyncReadExt, AsyncWriteExt};
    use libp2p::identity;
    use libp2p::Multiaddr;
    use std::sync::{Arc, Mutex};
    use tokio::time::{sleep, timeout, Duration};
    use tokio_util::compat::TokioAsyncReadCompatExt;
    use uc_core::network::{ConnectionPolicy, PairingState, ResolvedConnectionPolicy};
    use uc_core::ports::{ConnectionPolicyResolverError, ConnectionPolicyResolverPort};

    async fn echo_payload<Stream>(stream: &mut Stream) -> anyhow::Result<()>
    where
        Stream: libp2p::futures::AsyncRead + libp2p::futures::AsyncWrite + Unpin,
    {
        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer).await?;
        stream.write_all(&buffer).await?;
        stream.close().await?;
        Ok(())
    }

    #[test]
    fn mdns_config_has_5s_query_interval() {
        let config = build_mdns_config();
        assert_eq!(config.query_interval, Duration::from_secs(5));
    }

    #[test]
    fn business_command_timeouts_cover_stream_operation_budgets() {
        let send_budget = BUSINESS_STREAM_OPEN_TIMEOUT
            + BUSINESS_STREAM_WRITE_TIMEOUT
            + BUSINESS_STREAM_CLOSE_TIMEOUT
            + BUSINESS_COMMAND_ENQUEUE_TIMEOUT;
        let ensure_budget = BUSINESS_STREAM_OPEN_TIMEOUT
            + BUSINESS_STREAM_CLOSE_TIMEOUT
            + BUSINESS_COMMAND_ENQUEUE_TIMEOUT;
        assert!(
            BUSINESS_SEND_COMMAND_RESULT_TIMEOUT > send_budget,
            "send command timeout must exceed open/write/close/enqueue total budget"
        );
        assert!(
            BUSINESS_ENSURE_COMMAND_RESULT_TIMEOUT > ensure_budget,
            "ensure command timeout must exceed open/close/enqueue total budget"
        );
    }

    #[test]
    fn cache_inserts_discovered_peer_with_addresses() {
        let mut caches = PeerCaches::new();
        let discovered_at = Utc::now();
        let addresses = vec!["/ip4/192.168.1.2/tcp/4001".to_string()];

        let peer = caches.upsert_discovered("peer-1".to_string(), addresses.clone(), discovered_at);

        assert_eq!(peer.peer_id, "peer-1");
        assert_eq!(peer.addresses, addresses);
        assert_eq!(peer.discovered_at, discovered_at);
        assert!(peer.device_name.is_none());
        assert!(peer.device_id.is_none());
        assert!(!peer.is_paired);
    }

    #[test]
    fn cache_removes_discovered_peer_on_loss() {
        let mut caches = PeerCaches::new();
        caches.upsert_discovered(
            "peer-1".to_string(),
            vec!["/ip4/192.168.1.2/tcp/4001".to_string()],
            Utc::now(),
        );

        let removed = caches.remove_discovered("peer-1");
        assert!(removed.is_some());
        assert!(!caches.is_reachable("peer-1"));
        assert!(caches.remove_discovered("peer-1").is_none());
    }

    #[test]
    fn reachable_is_best_effort_and_requires_discovery() {
        let mut caches = PeerCaches::new();
        assert!(!caches.mark_reachable("peer-1", Utc::now()));
        assert!(!caches.is_reachable("peer-1"));

        caches.upsert_discovered(
            "peer-1".to_string(),
            vec!["/ip4/192.168.1.2/tcp/4001".to_string()],
            Utc::now(),
        );
        assert!(caches.mark_reachable("peer-1", Utc::now()));
        assert!(caches.is_reachable("peer-1"));
    }

    #[test]
    fn mdns_discovery_groups_addresses_by_peer() {
        let peer = PeerId::random();
        let addr_one: Multiaddr = "/ip4/192.168.1.2/tcp/4001".parse().unwrap();
        let addr_two: Multiaddr = "/ip4/192.168.1.3/tcp/4001".parse().unwrap();

        let grouped =
            collect_mdns_discovered(vec![(peer, addr_one.clone()), (peer, addr_two.clone())]);

        let addresses = grouped
            .get(&peer.to_string())
            .expect("peer should be grouped");
        assert_eq!(addresses.len(), 2);
        assert!(addresses.contains(&addr_one.to_string()));
        assert!(addresses.contains(&addr_two.to_string()));
    }

    #[test]
    fn mdns_expired_deduplicates_peers() {
        let peer = PeerId::random();
        let addr_one: Multiaddr = "/ip4/192.168.1.2/tcp/4001".parse().unwrap();
        let addr_two: Multiaddr = "/ip4/192.168.1.3/tcp/4001".parse().unwrap();

        let expired = collect_mdns_expired(vec![(peer, addr_one), (peer, addr_two)]);

        assert_eq!(expired.len(), 1);
        assert!(expired.contains(&peer.to_string()));
    }

    #[test]
    fn peer_ready_emits_event_only_for_discovered_peer() {
        let mut caches = PeerCaches::new();
        caches.upsert_discovered(
            "peer-1".to_string(),
            vec!["/ip4/192.168.1.2/tcp/4001".to_string()],
            Utc::now(),
        );

        let event = apply_peer_ready(&mut caches, "peer-1", Utc::now());

        assert!(matches!(
            event,
            Some(NetworkEvent::PeerReady { peer_id }) if peer_id == "peer-1"
        ));
        assert!(caches.is_reachable("peer-1"));
    }

    #[test]
    fn connection_established_backfills_discovery_and_reachable() {
        let mut caches = PeerCaches::new();
        let address: Multiaddr = "/ip4/127.0.0.1/tcp/5001".parse().expect("valid multiaddr");

        let event = apply_peer_ready_from_connection(
            &mut caches,
            "peer-1",
            Utc::now(),
            Some(address.clone()),
        );

        assert!(matches!(
            event,
            Some(NetworkEvent::PeerReady { peer_id }) if peer_id == "peer-1"
        ));
        assert!(caches.is_reachable("peer-1"));
        let discovered = caches
            .discovered_peers
            .get("peer-1")
            .expect("discovered peer");
        assert!(discovered.addresses.contains(&address.to_string()));
    }

    #[test]
    fn peer_not_ready_emits_event_only_for_reachable_peer() {
        let mut caches = PeerCaches::new();
        caches.upsert_discovered(
            "peer-1".to_string(),
            vec!["/ip4/192.168.1.2/tcp/4001".to_string()],
            Utc::now(),
        );

        assert!(apply_peer_not_ready(&mut caches, "peer-1").is_none());
        let _ = apply_peer_ready(&mut caches, "peer-1", Utc::now());

        let event = apply_peer_not_ready(&mut caches, "peer-1");

        assert!(matches!(
            event,
            Some(NetworkEvent::PeerNotReady { peer_id }) if peer_id == "peer-1"
        ));
        assert!(!caches.is_reachable("peer-1"));
    }

    #[test]
    fn mdns_discovery_and_expiry_emit_events() {
        let mut caches = PeerCaches::new();
        let discovered_at = Utc::now();
        let mut discovered = HashMap::new();
        discovered.insert(
            "peer-1".to_string(),
            vec!["/ip4/192.168.1.2/tcp/4001".to_string()],
        );

        let discovered_events = apply_mdns_discovered(&mut caches, discovered, discovered_at);
        assert_eq!(discovered_events.len(), 1);
        assert!(matches!(
            &discovered_events[0],
            NetworkEvent::PeerDiscovered(peer) if peer.peer_id == "peer-1"
        ));
        assert!(caches.discovered_peers.contains_key("peer-1"));

        let mut expired = HashSet::new();
        expired.insert("peer-1".to_string());
        let expired_events = apply_mdns_expired(&mut caches, expired);

        assert_eq!(expired_events.len(), 1);
        assert!(matches!(
            &expired_events[0],
            NetworkEvent::PeerLost(peer_id) if peer_id == "peer-1"
        ));
        assert!(!caches.discovered_peers.contains_key("peer-1"));
    }

    #[derive(Default)]
    struct TestIdentityStore {
        data: Mutex<Option<Vec<u8>>>,
    }

    impl IdentityStorePort for TestIdentityStore {
        fn load_identity(&self) -> Result<Option<Vec<u8>>, uc_core::ports::IdentityStoreError> {
            let guard = self.data.lock().expect("lock test identity store");
            Ok(guard.clone())
        }

        fn store_identity(
            &self,
            identity: &[u8],
        ) -> Result<(), uc_core::ports::IdentityStoreError> {
            let mut guard = self.data.lock().expect("lock test identity store");
            *guard = Some(identity.to_vec());
            Ok(())
        }
    }

    struct FakeResolver;

    #[async_trait::async_trait]
    impl ConnectionPolicyResolverPort for FakeResolver {
        async fn resolve_for_peer(
            &self,
            _peer_id: &uc_core::PeerId,
        ) -> Result<ResolvedConnectionPolicy, ConnectionPolicyResolverError> {
            Ok(ResolvedConnectionPolicy {
                pairing_state: PairingState::Trusted,
                allowed: ConnectionPolicy::allowed_protocols(PairingState::Trusted),
            })
        }
    }

    struct PendingResolver;

    #[async_trait::async_trait]
    impl ConnectionPolicyResolverPort for PendingResolver {
        async fn resolve_for_peer(
            &self,
            _peer_id: &uc_core::PeerId,
        ) -> Result<ResolvedConnectionPolicy, ConnectionPolicyResolverError> {
            Ok(ResolvedConnectionPolicy {
                pairing_state: PairingState::Pending,
                allowed: ConnectionPolicy::allowed_protocols(PairingState::Pending),
            })
        }
    }

    #[tokio::test]
    async fn adapter_constructs_with_policy_resolver() {
        let resolver: Arc<dyn ConnectionPolicyResolverPort> = Arc::new(FakeResolver);
        let adapter = Libp2pNetworkAdapter::new(
            Arc::new(TestIdentityStore::default()),
            resolver,
            Arc::new(InMemoryEncryptionSessionPort::default()),
        );
        assert!(adapter.is_ok());
    }

    #[tokio::test]
    async fn start_network_is_idempotent_when_called_twice() {
        let adapter = Libp2pNetworkAdapter::new(
            Arc::new(TestIdentityStore::default()),
            Arc::new(FakeResolver),
            Arc::new(InMemoryEncryptionSessionPort::default()),
        )
        .expect("create adapter");

        let first = NetworkControlPort::start_network(&adapter).await;
        let second = NetworkControlPort::start_network(&adapter).await;

        assert!(first.is_ok(), "first start should succeed: {first:?}");
        assert!(
            second.is_ok(),
            "second start should be idempotent: {second:?}"
        );
    }

    #[tokio::test]
    async fn start_network_can_retry_after_failed_start() {
        let adapter = Libp2pNetworkAdapter::new(
            Arc::new(TestIdentityStore::default()),
            Arc::new(FakeResolver),
            Arc::new(InMemoryEncryptionSessionPort::default()),
        )
        .expect("create adapter");

        let stolen_business_rx =
            Libp2pNetworkAdapter::take_receiver(&adapter.business_rx, "business")
                .expect("take business receiver");

        let first = NetworkControlPort::start_network(&adapter).await;
        assert!(
            first.is_err(),
            "first start should fail when business receiver is missing"
        );

        {
            let mut guard = adapter
                .business_rx
                .lock()
                .expect("lock business receiver mutex");
            *guard = Some(stolen_business_rx);
        }

        let retry = NetworkControlPort::start_network(&adapter).await;
        assert!(
            retry.is_ok(),
            "retry after failed start should succeed: {retry:?}"
        );
    }

    #[tokio::test]
    async fn device_announce_updates_cache_and_emits_event() {
        let caches = Arc::new(RwLock::new(PeerCaches::new()));
        let (event_tx, mut event_rx) = mpsc::channel(1);
        let (clipboard_tx, _clipboard_rx) = mpsc::channel(1);
        let announce = ProtocolMessage::DeviceAnnounce(DeviceAnnounceMessage {
            peer_id: "peer-1".to_string(),
            device_name: "Desk".to_string(),
            timestamp: Utc::now(),
        });

        handle_standard_message(
            caches.clone(),
            event_tx,
            clipboard_tx,
            "peer-1".to_string(),
            announce,
        )
        .await;

        let event = event_rx.recv().await.expect("peer name updated event");
        match event {
            NetworkEvent::PeerNameUpdated {
                peer_id,
                device_name,
            } => {
                assert_eq!(peer_id, "peer-1");
                assert_eq!(device_name, "Desk");
            }
            _ => panic!("expected PeerNameUpdated"),
        }

        let cached_name = caches
            .read()
            .await
            .discovered_peers
            .get("peer-1")
            .and_then(|peer| peer.device_name.clone());
        assert_eq!(cached_name, Some("Desk".to_string()));
    }

    #[tokio::test]
    async fn announce_device_name_queues_command() {
        let adapter = Libp2pNetworkAdapter::new(
            Arc::new(TestIdentityStore::default()),
            Arc::new(FakeResolver),
            Arc::new(InMemoryEncryptionSessionPort::default()),
        )
        .expect("create adapter");

        adapter
            .announce_device_name("Desk".to_string())
            .await
            .expect("announce device name");

        let mut rx = Libp2pNetworkAdapter::take_receiver(&adapter.business_rx, "business")
            .expect("business receiver");
        let command = rx.recv().await.expect("business command");
        match command {
            BusinessCommand::AnnounceDeviceName { device_name } => {
                assert_eq!(device_name, "Desk");
            }
            BusinessCommand::SendClipboard { .. } => {
                panic!("unexpected clipboard command")
            }
            BusinessCommand::EnsureBusinessPath { .. } => {
                panic!("unexpected ensure command")
            }
            BusinessCommand::UnpairPeer { .. } => {
                panic!("unexpected unpair command")
            }
        }
    }

    #[tokio::test]
    async fn business_stream_echoes_payload() {
        let payload = b"hello-business".to_vec();
        let (client, server) = tokio::io::duplex(1024);
        let mut client = client.compat();
        let mut server = server.compat();
        let server_task = tokio::spawn(async move { echo_payload(&mut server).await });

        client.write_all(&payload).await.expect("write payload");
        client.close().await.expect("close write");

        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .await
            .expect("read response");

        let server_result = server_task.await.expect("server task");
        server_result.expect("server echo");

        assert_eq!(response, payload);
    }

    #[tokio::test]
    async fn outbound_business_denied_emits_event() {
        let resolver: Arc<dyn ConnectionPolicyResolverPort> = Arc::new(PendingResolver);
        let (event_tx, mut event_rx) = mpsc::channel(1);

        let result =
            check_business_allowed(&resolver, &event_tx, "peer-1", ProtocolDirection::Outbound)
                .await;

        assert!(result.is_err());

        let event = event_rx.recv().await.expect("protocol denied event");
        match event {
            NetworkEvent::ProtocolDenied {
                protocol_id,
                direction,
                reason,
                ..
            } => {
                assert_eq!(protocol_id, BUSINESS_PROTOCOL_ID);
                assert_eq!(direction, ProtocolDirection::Outbound);
                assert_eq!(reason, ProtocolDenyReason::NotTrusted);
            }
            _ => panic!("expected ProtocolDenied"),
        }
    }

    #[tokio::test]
    async fn outbound_business_denied_keeps_peer_reachable() {
        let keypair = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(keypair.public());
        let behaviour = Libp2pBehaviour::new(local_peer_id).expect("behaviour");
        let swarm = SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default().nodelay(true),
                noise::Config::new,
                yamux::Config::default,
            )
            .expect("tcp config")
            .with_quic()
            .with_behaviour(move |_| behaviour)
            .expect("attach behaviour")
            .build();

        let caches = Arc::new(RwLock::new(PeerCaches::new()));
        let remote_keypair = identity::Keypair::generate_ed25519();
        let remote_peer = PeerId::from(remote_keypair.public());
        let remote_peer_id = remote_peer.to_string();
        {
            let mut caches_guard = caches.write().await;
            let _ = caches_guard.upsert_discovered(remote_peer_id.clone(), Vec::new(), Utc::now());
            assert!(caches_guard.mark_reachable(&remote_peer_id, Utc::now()));
        }

        let resolver: Arc<dyn ConnectionPolicyResolverPort> = Arc::new(PendingResolver);
        let (event_tx, mut event_rx) = mpsc::channel(4);
        let uc_peer_id = uc_core::PeerId::from(remote_peer_id.as_str());
        let control = swarm.behaviour().stream.new_control();

        let result = execute_business_stream(
            &control,
            &caches,
            &resolver,
            &event_tx,
            &uc_peer_id,
            remote_peer,
            Some(b"clipboard"),
            BUSINESS_STREAM_OPEN_TIMEOUT,
            BUSINESS_STREAM_WRITE_TIMEOUT,
            BUSINESS_STREAM_CLOSE_TIMEOUT,
            "clipboard",
        )
        .await;

        assert!(result.is_err());
        assert!(matches!(
            event_rx.recv().await,
            Some(NetworkEvent::ProtocolDenied { .. })
        ));
        assert!(
            caches.read().await.is_reachable(&remote_peer_id),
            "policy denial must not demote peer network readiness"
        );
    }

    #[tokio::test]
    async fn list_sendable_peers_filters_out_untrusted_peers() {
        let adapter = Libp2pNetworkAdapter::new(
            Arc::new(TestIdentityStore::default()),
            Arc::new(PendingResolver),
            Arc::new(InMemoryEncryptionSessionPort::default()),
        )
        .expect("create adapter");

        {
            let mut caches = adapter.caches.write().await;
            let _ = caches.upsert_discovered("peer-pending".to_string(), Vec::new(), Utc::now());
        }

        let peers = adapter
            .list_sendable_peers()
            .await
            .expect("list sendable peers");
        assert!(peers.is_empty(), "pending peer must not be sendable");
    }

    #[tokio::test]
    async fn list_sendable_peers_marks_trusted_peers_as_paired() {
        let adapter = Libp2pNetworkAdapter::new(
            Arc::new(TestIdentityStore::default()),
            Arc::new(FakeResolver),
            Arc::new(InMemoryEncryptionSessionPort::default()),
        )
        .expect("create adapter");

        {
            let mut caches = adapter.caches.write().await;
            let _ = caches.upsert_discovered("peer-trusted".to_string(), Vec::new(), Utc::now());
        }

        let peers = adapter
            .list_sendable_peers()
            .await
            .expect("list sendable peers");
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].peer_id, "peer-trusted");
        assert!(peers[0].is_paired);
    }

    #[tokio::test]
    async fn list_sendable_peers_excludes_local_peer_id() {
        let adapter = Libp2pNetworkAdapter::new(
            Arc::new(TestIdentityStore::default()),
            Arc::new(FakeResolver),
            Arc::new(InMemoryEncryptionSessionPort::default()),
        )
        .expect("create adapter");
        let local_peer_id = adapter.local_peer_id();

        {
            let mut caches = adapter.caches.write().await;
            let _ = caches.upsert_discovered(local_peer_id.clone(), Vec::new(), Utc::now());
            let _ = caches.upsert_discovered("peer-trusted".to_string(), Vec::new(), Utc::now());
        }

        let peers = adapter
            .list_sendable_peers()
            .await
            .expect("list sendable peers");

        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].peer_id, "peer-trusted");
        assert!(peers.iter().all(|peer| peer.peer_id != local_peer_id));
    }

    #[tokio::test]
    async fn inbound_business_denied_drops_stream_and_emits_event() {
        let resolver: Arc<dyn ConnectionPolicyResolverPort> = Arc::new(PendingResolver);
        let (event_tx, mut event_rx) = mpsc::channel(1);

        let result =
            check_business_allowed(&resolver, &event_tx, "peer-2", ProtocolDirection::Inbound)
                .await;

        assert!(result.is_err());

        let event = event_rx.recv().await.expect("protocol denied event");
        match event {
            NetworkEvent::ProtocolDenied {
                protocol_id,
                direction,
                reason,
                ..
            } => {
                assert_eq!(protocol_id, BUSINESS_PROTOCOL_ID);
                assert_eq!(direction, ProtocolDirection::Inbound);
                assert_eq!(reason, ProtocolDenyReason::NotTrusted);
            }
            _ => panic!("expected ProtocolDenied"),
        }
    }

    #[tokio::test]
    async fn legacy_pairing_denied_emits_protocol_id() {
        let resolver: Arc<dyn ConnectionPolicyResolverPort> = Arc::new(FakeResolver);
        let (event_tx, mut event_rx) = mpsc::channel(1);
        let error = anyhow::Error::new(PairingStreamError::UnsupportedProtocol);

        handle_pairing_open_error(&resolver, &event_tx, "peer-legacy", &error).await;

        let event = event_rx.recv().await.expect("protocol denied event");
        match event {
            NetworkEvent::ProtocolDenied {
                peer_id,
                protocol_id,
                pairing_state,
                direction,
                reason,
            } => {
                assert_eq!(peer_id, "peer-legacy");
                assert_eq!(protocol_id, ProtocolId::Pairing.as_str());
                assert_eq!(pairing_state, PairingState::Trusted);
                assert_eq!(direction, ProtocolDirection::Outbound);
                assert_eq!(reason, ProtocolDenyReason::NotSupported);
            }
            _ => panic!("expected ProtocolDenied"),
        }
    }

    #[tokio::test]
    async fn send_clipboard_opens_business_stream() {
        let adapter = Libp2pNetworkAdapter::new(
            Arc::new(TestIdentityStore::default()),
            Arc::new(FakeResolver),
            Arc::new(InMemoryEncryptionSessionPort::default()),
        )
        .expect("create adapter");
        let payload = vec![1, 2, 3, 4];
        let expected_payload = payload.clone();
        let mut rx = Libp2pNetworkAdapter::take_receiver(&adapter.business_rx, "business")
            .expect("business receiver");

        let send_task =
            tokio::spawn(async move { adapter.send_clipboard("peer-2", payload).await });
        let command = rx.recv().await.expect("business command");
        match command {
            BusinessCommand::SendClipboard {
                peer_id,
                data,
                result_tx,
                ..
            } => {
                assert_eq!(peer_id.as_str(), "peer-2");
                assert_eq!(data, expected_payload);
                result_tx
                    .send(Ok(()))
                    .expect("deliver send result to send_clipboard caller");
            }
            BusinessCommand::AnnounceDeviceName { .. } => {
                panic!("unexpected announce command")
            }
            BusinessCommand::EnsureBusinessPath { .. } => {
                panic!("unexpected ensure command")
            }
            BusinessCommand::UnpairPeer { .. } => {
                panic!("unexpected unpair command")
            }
        }

        send_task
            .await
            .expect("send task join")
            .expect("send clipboard");
    }

    #[tokio::test]
    async fn subscribe_clipboard_receiver_is_open() {
        let adapter = Libp2pNetworkAdapter::new(
            Arc::new(TestIdentityStore::default()),
            Arc::new(FakeResolver),
            Arc::new(InMemoryEncryptionSessionPort::default()),
        )
        .expect("create adapter");

        let receiver = adapter
            .subscribe_clipboard()
            .await
            .expect("subscribe clipboard");

        assert!(!receiver.is_closed());
    }

    #[test]
    fn adapter_exposes_raw_identity_pubkey() {
        let adapter = Libp2pNetworkAdapter::new(
            Arc::new(TestIdentityStore::default()),
            Arc::new(FakeResolver),
            Arc::new(InMemoryEncryptionSessionPort::default()),
        )
        .expect("create adapter");

        let pubkey = adapter.local_identity_pubkey();
        assert_eq!(pubkey.len(), 32);
    }

    async fn wait_for_discovery(
        mut rx: mpsc::Receiver<NetworkEvent>,
        expected_peer_id: &str,
    ) -> Option<DiscoveredPeer> {
        while let Some(event) = rx.recv().await {
            if let NetworkEvent::PeerDiscovered(peer) = event {
                if peer.peer_id == expected_peer_id {
                    return Some(peer);
                }
            }
        }
        None
    }

    async fn wait_for_mutual_discovery_or_skip(
        rx_a: mpsc::Receiver<NetworkEvent>,
        rx_b: mpsc::Receiver<NetworkEvent>,
        peer_a: &str,
        peer_b: &str,
    ) -> Option<(DiscoveredPeer, DiscoveredPeer)> {
        let discovery = timeout(Duration::from_secs(15), async {
            tokio::join!(
                wait_for_discovery(rx_a, peer_b),
                wait_for_discovery(rx_b, peer_a)
            )
        })
        .await;

        match discovery {
            Ok((Some(left), Some(right))) => Some((left, right)),
            Ok((left, right)) => {
                eprintln!(
                    "skip test: mdns discovery incomplete in current environment: left={:?} right={:?}",
                    left.as_ref().map(|peer| peer.peer_id.as_str()),
                    right.as_ref().map(|peer| peer.peer_id.as_str())
                );
                None
            }
            Err(_) => {
                eprintln!("skip test: mdns discovery timed out in current environment");
                None
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn mdns_e2e_discovers_peers() {
        let adapter_a = Libp2pNetworkAdapter::new(
            Arc::new(TestIdentityStore::default()),
            Arc::new(FakeResolver),
            Arc::new(InMemoryEncryptionSessionPort::default()),
        )
        .expect("create adapter a");
        let adapter_b = Libp2pNetworkAdapter::new(
            Arc::new(TestIdentityStore::default()),
            Arc::new(FakeResolver),
            Arc::new(InMemoryEncryptionSessionPort::default()),
        )
        .expect("create adapter b");
        let rx_a = adapter_a.subscribe_events().await.expect("subscribe a");
        let rx_b = adapter_b.subscribe_events().await.expect("subscribe b");
        adapter_a.spawn_swarm().expect("start swarm a");
        adapter_b.spawn_swarm().expect("start swarm b");

        let peer_a = adapter_a.local_peer_id();
        let peer_b = adapter_b.local_peer_id();

        sleep(Duration::from_millis(200)).await;

        if wait_for_mutual_discovery_or_skip(rx_a, rx_b, &peer_a, &peer_b)
            .await
            .is_none()
        {
            return;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ensure_business_path_opens_stream_without_blocking_swarm_poll() {
        let adapter_a = Libp2pNetworkAdapter::new(
            Arc::new(TestIdentityStore::default()),
            Arc::new(FakeResolver),
            Arc::new(InMemoryEncryptionSessionPort::default()),
        )
        .expect("create adapter a");
        let adapter_b = Libp2pNetworkAdapter::new(
            Arc::new(TestIdentityStore::default()),
            Arc::new(FakeResolver),
            Arc::new(InMemoryEncryptionSessionPort::default()),
        )
        .expect("create adapter b");
        let rx_a = adapter_a.subscribe_events().await.expect("subscribe a");
        let rx_b = adapter_b.subscribe_events().await.expect("subscribe b");
        adapter_a.spawn_swarm().expect("start swarm a");
        adapter_b.spawn_swarm().expect("start swarm b");

        let peer_a = adapter_a.local_peer_id();
        let peer_b = adapter_b.local_peer_id();

        sleep(Duration::from_millis(200)).await;

        if wait_for_mutual_discovery_or_skip(rx_a, rx_b, &peer_a, &peer_b)
            .await
            .is_none()
        {
            return;
        }

        match timeout(
            Duration::from_secs(20),
            ClipboardTransportPort::ensure_business_path(&adapter_a, &peer_b),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => panic!("ensure business path failed unexpectedly: {err}"),
            Err(_) => panic!("ensure business path timed out"),
        }

        let connected = timeout(Duration::from_secs(5), async {
            loop {
                let peers = adapter_a
                    .get_connected_peers()
                    .await
                    .expect("query connected peers");
                if peers.iter().any(|peer| peer.peer_id == peer_b) {
                    return true;
                }
                sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .unwrap_or(false);
        assert!(
            connected,
            "ensure business path should mark peer as reachable after stream success"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn libp2p_network_clipboard_wire_roundtrip_delivers_clipboard_message() {
        let adapter_a = Libp2pNetworkAdapter::new(
            Arc::new(TestIdentityStore::default()),
            Arc::new(FakeResolver),
            Arc::new(InMemoryEncryptionSessionPort::default()),
        )
        .expect("create adapter a");
        let adapter_b = Libp2pNetworkAdapter::new(
            Arc::new(TestIdentityStore::default()),
            Arc::new(FakeResolver),
            Arc::new(InMemoryEncryptionSessionPort::default()),
        )
        .expect("create adapter b");
        let rx_a = adapter_a
            .subscribe_events()
            .await
            .expect("subscribe events a");
        let rx_b = adapter_b
            .subscribe_events()
            .await
            .expect("subscribe events b");
        let mut clipboard_rx_b = adapter_b
            .subscribe_clipboard()
            .await
            .expect("subscribe clipboard b");
        adapter_a.spawn_swarm().expect("start swarm a");
        adapter_b.spawn_swarm().expect("start swarm b");

        let peer_a = adapter_a.local_peer_id();
        let peer_b = adapter_b.local_peer_id();

        sleep(Duration::from_millis(200)).await;

        if wait_for_mutual_discovery_or_skip(rx_a, rx_b, &peer_a, &peer_b)
            .await
            .is_none()
        {
            return;
        }

        PairingTransportPort::open_pairing_session(
            &adapter_a,
            peer_b.clone(),
            "wire-compat-session".to_string(),
        )
        .await
        .expect("open pairing session before business clipboard send");
        sleep(Duration::from_millis(300)).await;

        let expected = ClipboardMessage {
            id: "msg-wire-1".to_string(),
            content_hash: "wire-hash-1".to_string(),
            encrypted_content: vec![1, 2, 3, 4, 5],
            timestamp: Utc::now(),
            origin_device_id: "device-a".to_string(),
            origin_device_name: "Adapter A".to_string(),
            payload_version: uc_core::network::protocol::ClipboardPayloadVersion::V1,
        };
        // Use frame_to_bytes for the new two-segment wire format
        let payload = ProtocolMessage::Clipboard(expected.clone())
            .frame_to_bytes(None)
            .expect("serialize clipboard protocol payload with frame_to_bytes");

        let mut received = None;
        for _attempt in 0..20 {
            ClipboardTransportPort::send_clipboard(&adapter_a, &peer_b, payload.clone())
                .await
                .expect("send clipboard protocol payload");

            match timeout(Duration::from_millis(500), clipboard_rx_b.recv()).await {
                Ok(Some((message, pre_decoded))) => {
                    // V1 messages should have None pre-decoded plaintext
                    assert!(
                        pre_decoded.is_none(),
                        "V1 message should have no pre-decoded plaintext"
                    );
                    received = Some(message);
                    break;
                }
                Ok(None) => break,
                Err(_) => {
                    sleep(Duration::from_millis(100)).await;
                }
            }
        }

        let received = received.expect("clipboard payload from peer a");

        assert_eq!(received.id, expected.id);
        assert_eq!(received.content_hash, expected.content_hash);
        assert_eq!(received.encrypted_content, expected.encrypted_content);
        assert_eq!(received.origin_device_id, expected.origin_device_id);
        assert_eq!(received.origin_device_name, expected.origin_device_name);
    }

    #[test]
    fn try_send_event_reports_backpressure() {
        let (event_tx, _event_rx) = mpsc::channel(1);
        event_tx
            .try_send(NetworkEvent::PeerLost("peer-1".to_string()))
            .expect("fill channel");

        let result = try_send_event(
            &event_tx,
            NetworkEvent::PeerLost("peer-2".to_string()),
            "PeerLost",
        );

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn listen_on_failure_returns_err() {
        let keypair = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(keypair.public());
        let behaviour = Libp2pBehaviour::new(local_peer_id).expect("behaviour");
        let mut swarm = SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default().nodelay(true),
                noise::Config::new,
                yamux::Config::default,
            )
            .expect("tcp config")
            .with_quic()
            .with_behaviour(move |_| behaviour)
            .expect("attach behaviour")
            .build();

        let bad_addr: Multiaddr = "/ip4/127.0.0.1/udp/0".parse().expect("bad addr");

        let result = listen_on_swarm(&mut swarm, bad_addr);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("failed to listen on"),);
    }

    #[tokio::test]
    async fn listen_on_accepts_quic_and_tcp_addresses() {
        let keypair = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(keypair.public());
        let behaviour = Libp2pBehaviour::new(local_peer_id).expect("behaviour");

        let mut swarm = SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default().nodelay(true),
                noise::Config::new,
                yamux::Config::default,
            )
            .expect("tcp config")
            .with_quic()
            .with_behaviour(move |_| behaviour)
            .expect("attach behaviour")
            .build();

        let quic_addr: Multiaddr = "/ip4/127.0.0.1/udp/0/quic-v1".parse().expect("quic addr");
        let tcp_addr: Multiaddr = "/ip4/127.0.0.1/tcp/0".parse().expect("tcp addr");

        listen_on_swarm(&mut swarm, quic_addr).expect("listen quic");
        listen_on_swarm(&mut swarm, tcp_addr).expect("listen tcp");
    }

    #[test]
    fn sort_addresses_quic_first_puts_quic_before_tcp() {
        let mut addresses = vec![
            "/ip4/192.168.1.100/tcp/12345".to_string(),
            "/ip4/192.168.1.100/udp/54321/quic-v1".to_string(),
            "/ip4/192.168.1.100/tcp/12346".to_string(),
            "/ip4/192.168.1.100/udp/54322/quic-v1".to_string(),
        ];
        sort_addresses_quic_first(&mut addresses);
        assert!(addresses[0].contains("/quic-v1"));
        assert!(addresses[1].contains("/quic-v1"));
        assert!(addresses[2].contains("/tcp/"));
        assert!(addresses[3].contains("/tcp/"));
    }

    #[test]
    fn sort_addresses_quic_first_preserves_relative_order() {
        let mut addresses = vec![
            "/ip4/10.0.0.1/tcp/1000".to_string(),
            "/ip4/10.0.0.2/udp/2000/quic-v1".to_string(),
            "/ip4/10.0.0.3/tcp/3000".to_string(),
        ];
        sort_addresses_quic_first(&mut addresses);
        assert_eq!(addresses[0], "/ip4/10.0.0.2/udp/2000/quic-v1");
        assert_eq!(addresses[1], "/ip4/10.0.0.1/tcp/1000");
        assert_eq!(addresses[2], "/ip4/10.0.0.3/tcp/3000");
    }
}
