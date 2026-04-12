use super::super::pairing_stream::service::PairingStreamError;
#[allow(deprecated)]
use super::behaviour::{build_mdns_config, Libp2pBehaviour};
use super::business_stream::{apply_business_stream_result, execute_business_stream};
use super::dial_strategy::{
    chosen_dial_addr_for_log, dial_decision_for_snapshot, infer_address_scope,
    infer_chosen_dial_addr_resolution, sort_addresses_quic_first, successful_dial_observation,
};
use super::discovery::{
    apply_mdns_discovered, apply_mdns_expired, apply_peer_not_ready, apply_peer_ready,
    apply_peer_ready_from_connection, collect_mdns_discovered, collect_mdns_expired,
};
use super::stream_handler::{handle_pairing_open_error, handle_standard_message};
use super::*;
use crate::adapters::{InMemoryEncryptionSessionPort, PairingRuntimeOwner};
use libp2p::futures::{AsyncReadExt, AsyncWriteExt};
use libp2p::identity;
use libp2p::swarm::ConnectionId;
use libp2p::Multiaddr;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::time::{sleep, timeout, Duration};
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;
use uc_core::network::address_registry::{AddressScope, AddressSource};
use uc_core::network::protocol::ClipboardPayloadVersion;
use uc_core::network::{
    ConnectionPolicy, DeviceAnnounceMessage, PairingState, ProtocolDenyReason, ProtocolId,
    ResolvedConnectionPolicy,
};
use uc_core::ports::{ConnectionPolicyResolverError, ConnectionPolicyResolverPort};
use uc_core::security::MasterKey;

struct PassthroughTransferPayloadDecryptor;

impl TransferPayloadDecryptorPort for PassthroughTransferPayloadDecryptor {
    fn decrypt(
        &self,
        encrypted: &[u8],
        _master_key: &MasterKey,
    ) -> Result<Vec<u8>, uc_core::ports::TransferCryptoError> {
        Ok(encrypted.to_vec())
    }
}

struct PassthroughTransferPayloadEncryptor;

impl TransferPayloadEncryptorPort for PassthroughTransferPayloadEncryptor {
    fn encrypt(
        &self,
        _master_key: &MasterKey,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, uc_core::ports::TransferCryptoError> {
        Ok(plaintext.to_vec())
    }
}

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
fn cache_upsert_discovered_preserves_device_name() {
    let mut caches = PeerCaches::new();
    let t0 = Utc::now();

    // Initial discovery: no name yet
    let peer = caches.upsert_discovered(
        "peer-1".to_string(),
        vec!["/ip4/192.168.1.2/tcp/4001".to_string()],
        t0,
    );
    assert!(peer.device_name.is_none());

    // Device name resolved via DeviceAnnounce protocol
    caches.upsert_device_name("peer-1", "My Laptop".to_string(), t0);

    // Re-discovery via mDNS: device_name must be preserved
    let peer = caches.upsert_discovered(
        "peer-1".to_string(),
        vec!["/ip4/192.168.1.2/tcp/4001".to_string()],
        t0,
    );
    assert_eq!(peer.device_name.as_deref(), Some("My Laptop"));
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
fn mark_unreachable_preserves_last_dial_observation_for_recovery() {
    // Recovery-wave-1 contract: mark_unreachable must keep
    // `last_dial_observations` around so the recovery coordinator can retry
    // the last known usable path after a transient drop. See
    // docs/p2p/2026-04-11-connection-stability-recovery-prd.md §Definitions.
    let mut caches = PeerCaches::new();
    let t0 = Utc::now();
    let dial_addr = "/ip4/10.0.0.8/tcp/4001";

    caches.upsert_discovered("peer-1".to_string(), vec![dial_addr.to_string()], t0);
    assert!(caches.mark_reachable("peer-1", t0));
    caches.record_dial_observation("peer-1", successful_dial_observation(dial_addr, t0));

    assert!(caches.mark_unreachable("peer-1"));

    assert!(
        caches.last_dial_observations.contains_key("peer-1"),
        "mark_unreachable must preserve last_dial_observations"
    );

    let observation = caches
        .last_dial_observations
        .get("peer-1")
        .expect("observation should still be present");
    assert_eq!(observation.chosen_dial_addr.as_deref(), Some(dial_addr));
}

#[test]
fn remove_discovered_preserves_last_dial_observation_for_recovery() {
    // Recovery-wave-1 contract: mDNS expiry must not erase the usable path.
    let mut caches = PeerCaches::new();
    let t0 = Utc::now();
    let dial_addr = "/ip4/10.0.0.8/tcp/4001";

    caches.upsert_discovered("peer-1".to_string(), vec![dial_addr.to_string()], t0);
    assert!(caches.mark_reachable("peer-1", t0));
    caches.record_dial_observation("peer-1", successful_dial_observation(dial_addr, t0));

    let removed = caches.remove_discovered("peer-1");
    assert!(
        removed.is_some(),
        "peer should be fully removed from discovered_peers"
    );

    assert!(
        caches.last_dial_observations.contains_key("peer-1"),
        "remove_discovered must preserve last_dial_observations"
    );
}

#[test]
fn forget_peer_clears_last_dial_observation() {
    // Explicit forget (unpair) must erase everything including the usable path.
    let mut caches = PeerCaches::new();
    let t0 = Utc::now();
    let dial_addr = "/ip4/10.0.0.8/tcp/4001";

    caches.upsert_discovered("peer-1".to_string(), vec![dial_addr.to_string()], t0);
    assert!(caches.mark_reachable("peer-1", t0));
    caches.record_dial_observation("peer-1", successful_dial_observation(dial_addr, t0));

    let removed = caches.forget_peer("peer-1");
    assert!(
        removed.is_some(),
        "forget_peer should return the removed entry"
    );

    assert!(
        !caches.last_dial_observations.contains_key("peer-1"),
        "forget_peer must clear last_dial_observations"
    );
    assert!(caches.discovered_peers.get("peer-1").is_none());
    assert!(!caches.is_reachable("peer-1"));
}

#[test]
fn mark_connection_closed_preserves_last_dial_observation_for_recovery() {
    // Recovery-wave-1 contract: closing the last active connection must NOT
    // erase last_dial_observations. The recovery coordinator needs the last
    // known usable path to issue a Step-1 probe after connection drop.
    let mut caches = PeerCaches::new();
    let t0 = Utc::now();
    let dial_addr = "/ip4/10.0.0.8/tcp/4001";
    let conn_id = ConnectionId::new_unchecked(1);

    caches.upsert_discovered("peer-1".to_string(), vec![dial_addr.to_string()], t0);
    caches.mark_connection_established("peer-1", conn_id, Some(dial_addr.to_string()), t0);
    caches.record_dial_observation("peer-1", successful_dial_observation(dial_addr, t0));

    let became_unreachable = caches.mark_connection_closed("peer-1", conn_id);
    assert!(
        became_unreachable,
        "closing the last connection should mark peer unreachable"
    );
    assert!(!caches.is_reachable("peer-1"));

    assert!(
        caches.last_dial_observations.contains_key("peer-1"),
        "mark_connection_closed must preserve last_dial_observations"
    );
    let obs = caches.last_dial_observations.get("peer-1").unwrap();
    assert_eq!(obs.chosen_dial_addr.as_deref(), Some(dial_addr));
}

#[test]
fn regression_stale_dial_observation_not_used_for_new_attempt() {
    let mut caches = PeerCaches::new();
    let t0 = Utc::now();
    let addr_a = "/ip4/10.0.0.8/tcp/4001";
    let addr_b = "/ip4/10.0.0.9/tcp/4001";

    caches.upsert_discovered(
        "peer-1".to_string(),
        vec![addr_a.to_string(), addr_b.to_string()],
        t0,
    );
    caches.record_dial_observation("peer-1", successful_dial_observation(addr_a, t0));

    let attempt_started_at = t0 + chrono::TimeDelta::seconds(1);
    let snapshot = snapshot_peer_addresses(&caches, "peer-1", attempt_started_at);

    assert_eq!(
        chosen_dial_addr_for_log(&snapshot, "new_dial_required", attempt_started_at),
        None
    );
    assert_eq!(
        infer_chosen_dial_addr_resolution(&snapshot, "new_dial_required", attempt_started_at),
        "unknown"
    );
}

#[test]
fn regression_reuse_existing_connection_does_not_emit_chosen_dial_addr() {
    let mut caches = PeerCaches::new();
    let t0 = Utc::now();
    let addr = "/ip4/10.0.0.8/tcp/4001";

    caches.upsert_discovered("peer-1".to_string(), vec![addr.to_string()], t0);
    assert!(caches.mark_reachable("peer-1", t0));
    caches.record_dial_observation("peer-1", successful_dial_observation(addr, t0));

    let attempt_started_at = t0 + chrono::TimeDelta::seconds(1);
    let snapshot = snapshot_peer_addresses(&caches, "peer-1", attempt_started_at);

    assert_eq!(
        chosen_dial_addr_for_log(&snapshot, "reuse_existing_connection", attempt_started_at),
        None
    );
    assert_eq!(
        infer_chosen_dial_addr_resolution(
            &snapshot,
            "reuse_existing_connection",
            attempt_started_at
        ),
        "not_applicable"
    );
}

#[test]
fn dial_decision_upgrades_when_better_candidate_appears() {
    let mut caches = PeerCaches::new();
    let t0 = Utc::now();
    let wan_addr = "/ip4/203.0.113.10/tcp/4001";
    let lan_addr = "/ip4/192.168.1.8/udp/4001/quic-v1";

    caches.upsert_discovered(
        "peer-1".to_string(),
        vec![wan_addr.to_string(), lan_addr.to_string()],
        t0,
    );
    assert!(caches.mark_connection_established(
        "peer-1",
        ConnectionId::new_unchecked(1),
        Some(wan_addr.to_string()),
        t0,
    ));

    let snapshot = snapshot_peer_addresses(&caches, "peer-1", t0 + chrono::TimeDelta::seconds(1));

    assert_eq!(snapshot.best_connected_address.as_deref(), Some(wan_addr));
    assert_eq!(
        dial_decision_for_snapshot(&snapshot),
        "upgrade_to_better_connection"
    );
}

#[test]
fn dial_decision_reuses_when_current_connection_is_already_best() {
    let mut caches = PeerCaches::new();
    let t0 = Utc::now();
    let lan_addr = "/ip4/192.168.1.8/udp/4001/quic-v1";
    let wan_addr = "/ip4/203.0.113.10/tcp/4001";

    caches.upsert_discovered(
        "peer-1".to_string(),
        vec![wan_addr.to_string(), lan_addr.to_string()],
        t0,
    );
    assert!(caches.mark_connection_established(
        "peer-1",
        ConnectionId::new_unchecked(1),
        Some(lan_addr.to_string()),
        t0,
    ));

    let snapshot = snapshot_peer_addresses(&caches, "peer-1", t0 + chrono::TimeDelta::seconds(1));

    assert_eq!(snapshot.best_connected_address.as_deref(), Some(lan_addr));
    assert_eq!(
        dial_decision_for_snapshot(&snapshot),
        "reuse_existing_connection"
    );
}

#[test]
fn inferior_connections_only_returns_worse_paths() {
    let mut caches = PeerCaches::new();
    let t0 = Utc::now();
    let lan_addr = "/ip4/192.168.1.8/udp/4001/quic-v1";
    let wan_addr = "/ip4/203.0.113.10/tcp/4001";
    let lan_connection = ConnectionId::new_unchecked(1);
    let wan_connection = ConnectionId::new_unchecked(2);

    caches.upsert_discovered(
        "peer-1".to_string(),
        vec![wan_addr.to_string(), lan_addr.to_string()],
        t0,
    );
    assert!(caches.mark_connection_established(
        "peer-1",
        wan_connection,
        Some(wan_addr.to_string()),
        t0,
    ));
    assert!(!caches.mark_connection_established(
        "peer-1",
        lan_connection,
        Some(lan_addr.to_string()),
        t0,
    ));

    let inferior = caches.inferior_connection_ids("peer-1");

    assert_eq!(inferior, vec![wan_connection]);
}

#[test]
fn closing_one_connection_keeps_peer_reachable_when_another_remains() {
    let mut caches = PeerCaches::new();
    let t0 = Utc::now();
    let lan_addr = "/ip4/192.168.1.8/udp/4001/quic-v1";
    let wan_addr = "/ip4/203.0.113.10/tcp/4001";
    let lan_connection = ConnectionId::new_unchecked(1);
    let wan_connection = ConnectionId::new_unchecked(2);

    caches.upsert_discovered(
        "peer-1".to_string(),
        vec![wan_addr.to_string(), lan_addr.to_string()],
        t0,
    );
    assert!(caches.mark_connection_established(
        "peer-1",
        wan_connection,
        Some(wan_addr.to_string()),
        t0,
    ));
    assert!(!caches.mark_connection_established(
        "peer-1",
        lan_connection,
        Some(lan_addr.to_string()),
        t0,
    ));

    assert!(!caches.mark_connection_closed("peer-1", wan_connection));

    let snapshot = snapshot_peer_addresses(&caches, "peer-1", t0 + chrono::TimeDelta::seconds(1));
    assert!(caches.is_reachable("peer-1"));
    assert_eq!(snapshot.connected_address_count, 1);
    assert_eq!(snapshot.best_connected_address.as_deref(), Some(lan_addr));
}

#[test]
fn mdns_discovery_groups_addresses_by_peer() {
    let peer = PeerId::random();
    let addr_one: Multiaddr = "/ip4/192.168.1.2/tcp/4001".parse().unwrap();
    let addr_two: Multiaddr = "/ip4/192.168.1.3/tcp/4001".parse().unwrap();

    let grouped = collect_mdns_discovered(vec![(peer, addr_one.clone()), (peer, addr_two.clone())]);

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
        ConnectionId::new_unchecked(1),
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

#[tokio::test]
async fn business_stream_failure_keeps_peer_ready_when_another_connection_is_still_alive() {
    let caches = Arc::new(RwLock::new(PeerCaches::new()));
    let (event_tx, mut event_rx) = mpsc::channel(4);
    let t0 = Utc::now();
    let wan_addr = "/ip4/203.0.113.10/tcp/4001";
    let lan_addr = "/ip4/192.168.1.8/udp/4001/quic-v1";

    {
        let mut caches = caches.write().await;
        caches.upsert_discovered(
            "peer-1".to_string(),
            vec![wan_addr.to_string(), lan_addr.to_string()],
            t0,
        );
        assert!(caches.mark_connection_established(
            "peer-1",
            ConnectionId::new_unchecked(1),
            Some(wan_addr.to_string()),
            t0,
        ));
        assert!(!caches.mark_connection_established(
            "peer-1",
            ConnectionId::new_unchecked(2),
            Some(lan_addr.to_string()),
            t0,
        ));
    }

    let failure: anyhow::Result<()> = Err(anyhow::anyhow!("simulated stream failure"));
    apply_business_stream_result(&caches, &event_tx, "peer-1", &failure).await;

    assert!(
        event_rx.try_recv().is_err(),
        "no PeerNotReady should be emitted"
    );
    let caches = caches.read().await;
    assert!(caches.is_reachable("peer-1"));
    assert!(caches.has_active_connections("peer-1"));
    assert_eq!(
        caches
            .active_connections
            .get("peer-1")
            .map(|connections| connections.len()),
        Some(2)
    );
}

#[tokio::test]
async fn business_stream_failure_marks_peer_not_ready_when_no_connection_remains() {
    let caches = Arc::new(RwLock::new(PeerCaches::new()));
    let (event_tx, mut event_rx) = mpsc::channel(4);
    let t0 = Utc::now();

    {
        let mut caches = caches.write().await;
        caches.upsert_discovered(
            "peer-1".to_string(),
            vec!["/ip4/192.168.1.2/tcp/4001".to_string()],
            t0,
        );
        assert!(caches.mark_reachable("peer-1", t0));
    }

    let failure: anyhow::Result<()> = Err(anyhow::anyhow!("simulated stream failure"));
    apply_business_stream_result(&caches, &event_tx, "peer-1", &failure).await;

    let event = event_rx
        .recv()
        .await
        .expect("PeerNotReady should be emitted");
    assert!(matches!(
        event,
        NetworkEvent::PeerNotReady { peer_id } if peer_id == "peer-1"
    ));
    let caches = caches.read().await;
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
    fn load_identity(&self) -> Result<Option<Vec<u8>>, crate::ports::IdentityStoreError> {
        let guard = self.data.lock().expect("lock test identity store");
        Ok(guard.clone())
    }

    fn store_identity(&self, identity: &[u8]) -> Result<(), crate::ports::IdentityStoreError> {
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

#[derive(Default)]
struct EventNameVisitor {
    event_name: Option<String>,
}

impl Visit for EventNameVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "event" {
            self.event_name = Some(value.to_string());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "event" && self.event_name.is_none() {
            self.event_name = Some(format!("{value:?}").trim_matches('"').to_string());
        }
    }
}

#[derive(Clone)]
struct EventScopeCaptureLayer {
    captured: Arc<Mutex<Vec<(String, Vec<String>)>>>,
}

impl EventScopeCaptureLayer {
    fn new(captured: Arc<Mutex<Vec<(String, Vec<String>)>>>) -> Self {
        Self { captured }
    }
}

impl<S> Layer<S> for EventScopeCaptureLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut visitor = EventNameVisitor::default();
        event.record(&mut visitor);
        let Some(event_name) = visitor.event_name else {
            return;
        };
        if event_name != "business_stream.open_attempt" {
            return;
        }

        let scope = ctx
            .event_scope(event)
            .map(|scope| {
                scope
                    .from_root()
                    .map(|span| span.name().to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.captured
            .lock()
            .expect("lock captured events")
            .push((event_name, scope));
    }
}

fn test_adapter(pairing_runtime_owner: PairingRuntimeOwner) -> Libp2pNetworkAdapter {
    Libp2pNetworkAdapter::new(
        Arc::new(TestIdentityStore::default()),
        Arc::new(FakeResolver),
        Arc::new(InMemoryEncryptionSessionPort::default()),
        Arc::new(PassthroughTransferPayloadDecryptor),
        Arc::new(PassthroughTransferPayloadEncryptor),
        PathBuf::from("/tmp/test-file-cache"),
        pairing_runtime_owner,
    )
    .expect("create adapter")
}

#[tokio::test]
async fn adapter_constructs_with_policy_resolver() {
    let resolver: Arc<dyn ConnectionPolicyResolverPort> = Arc::new(FakeResolver);
    let adapter = Libp2pNetworkAdapter::new(
        Arc::new(TestIdentityStore::default()),
        resolver,
        Arc::new(InMemoryEncryptionSessionPort::default()),
        Arc::new(PassthroughTransferPayloadDecryptor),
        Arc::new(PassthroughTransferPayloadEncryptor),
        PathBuf::from("/tmp/test-file-cache"),
        PairingRuntimeOwner::CurrentProcess,
    );
    assert!(adapter.is_ok());
}

#[tokio::test]
async fn pairing_runtime_disabled_does_not_initialize_pairing_service() {
    let adapter = test_adapter(PairingRuntimeOwner::ExternalDaemon);

    adapter.spawn_swarm().expect("start swarm");

    let guard = adapter
        .pairing_service
        .lock()
        .expect("lock pairing service mutex");
    assert!(guard.is_none(), "pairing service must stay disabled");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pairing_runtime_disabled_does_not_register_pairing_protocol() {
    let current_process = test_adapter(PairingRuntimeOwner::CurrentProcess);
    let external_daemon = test_adapter(PairingRuntimeOwner::ExternalDaemon);
    let rx_a = current_process
        .subscribe_events()
        .await
        .expect("subscribe a");
    let rx_b = external_daemon
        .subscribe_events()
        .await
        .expect("subscribe b");

    current_process.spawn_swarm().expect("start swarm a");
    external_daemon.spawn_swarm().expect("start swarm b");

    let peer_a = current_process.local_peer_id();
    let peer_b = external_daemon.local_peer_id();

    sleep(Duration::from_millis(200)).await;

    if wait_for_mutual_discovery_or_skip(rx_a, rx_b, &peer_a, &peer_b)
        .await
        .is_none()
    {
        return;
    }

    let result = timeout(
        Duration::from_secs(10),
        PairingTransportPort::open_pairing_session(
            &current_process,
            peer_b.clone(),
            "disabled-pairing-protocol".to_string(),
        ),
    )
    .await
    .expect("open pairing session timeout")
    .expect_err("pairing protocol must be unavailable");

    assert!(
        result.to_string().contains("unsupported"),
        "expected unsupported protocol error, got: {result}"
    );
}

#[tokio::test]
async fn pairing_runtime_current_process_initializes_pairing_service() {
    let adapter = test_adapter(PairingRuntimeOwner::CurrentProcess);

    adapter.spawn_swarm().expect("start swarm");

    let guard = adapter
        .pairing_service
        .lock()
        .expect("lock pairing service mutex");
    assert!(guard.is_some(), "pairing service must be initialized");
}

#[tokio::test]
async fn start_network_is_idempotent_when_called_twice() {
    let adapter = Libp2pNetworkAdapter::new(
        Arc::new(TestIdentityStore::default()),
        Arc::new(FakeResolver),
        Arc::new(InMemoryEncryptionSessionPort::default()),
        Arc::new(PassthroughTransferPayloadDecryptor),
        Arc::new(PassthroughTransferPayloadEncryptor),
        PathBuf::from("/tmp/test-file-cache"),
        PairingRuntimeOwner::CurrentProcess,
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
async fn start_network_skips_swarm_when_pairing_runtime_is_external_daemon() {
    let adapter = Libp2pNetworkAdapter::new(
        Arc::new(TestIdentityStore::default()),
        Arc::new(FakeResolver),
        Arc::new(InMemoryEncryptionSessionPort::default()),
        Arc::new(PassthroughTransferPayloadDecryptor),
        Arc::new(PassthroughTransferPayloadEncryptor),
        PathBuf::from("/tmp/test-file-cache"),
        PairingRuntimeOwner::ExternalDaemon,
    )
    .expect("create adapter");

    let result = NetworkControlPort::start_network(&adapter).await;

    assert!(
        result.is_ok(),
        "external daemon start should succeed: {result:?}"
    );
    assert_eq!(
        adapter.start_state.load(Ordering::Acquire),
        START_STATE_STARTED,
        "external daemon mode should still mark network as started"
    );
    assert!(
        adapter
            .stream_control
            .lock()
            .expect("lock stream control")
            .is_none(),
        "external daemon mode must not spawn a local swarm"
    );
    assert!(
        adapter
            .pairing_service
            .lock()
            .expect("lock pairing service")
            .is_none(),
        "external daemon mode must not initialize pairing service"
    );
}

#[tokio::test]
async fn start_network_can_retry_after_failed_start() {
    let adapter = Libp2pNetworkAdapter::new(
        Arc::new(TestIdentityStore::default()),
        Arc::new(FakeResolver),
        Arc::new(InMemoryEncryptionSessionPort::default()),
        Arc::new(PassthroughTransferPayloadDecryptor),
        Arc::new(PassthroughTransferPayloadEncryptor),
        PathBuf::from("/tmp/test-file-cache"),
        PairingRuntimeOwner::CurrentProcess,
    )
    .expect("create adapter");

    let stolen_business_rx = Libp2pNetworkAdapter::take_receiver(&adapter.business_rx, "business")
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
async fn v3_clipboard_with_header_payload_uses_standard_forward_path() {
    let caches = Arc::new(RwLock::new(PeerCaches::new()));
    let (event_tx, mut event_rx) = mpsc::channel(1);
    let (clipboard_tx, mut clipboard_rx) = mpsc::channel(1);
    let message = ClipboardMessage {
        id: "msg-header-v3".to_string(),
        content_hash: "hash-header-v3".to_string(),
        encrypted_content: vec![7, 8, 9],
        timestamp: Utc::now(),
        origin_device_id: "peer-1".to_string(),
        origin_device_name: "Desk".to_string(),
        payload_version: ClipboardPayloadVersion::V3,
        origin_flow_id: None,
        traceparent: None,
        file_transfers: vec![],
    };

    handle_standard_message(
        caches,
        event_tx,
        clipboard_tx,
        "peer-1".to_string(),
        ProtocolMessage::Clipboard(message.clone()),
    )
    .await;

    let (forwarded, pre_decoded) = clipboard_rx.recv().await.expect("clipboard payload");
    assert_eq!(forwarded.id, message.id);
    assert_eq!(forwarded.content_hash, message.content_hash);
    assert_eq!(forwarded.encrypted_content, message.encrypted_content);
    assert!(
        pre_decoded.is_none(),
        "standard path should not attach plaintext"
    );

    let event = event_rx.recv().await.expect("clipboard received event");
    match event {
        NetworkEvent::ClipboardReceived(received) => {
            assert_eq!(received.id, message.id);
            assert_eq!(received.encrypted_content, message.encrypted_content);
        }
        _ => panic!("expected ClipboardReceived"),
    }
}

#[tokio::test]
async fn announce_device_name_queues_command() {
    let adapter = Libp2pNetworkAdapter::new(
        Arc::new(TestIdentityStore::default()),
        Arc::new(FakeResolver),
        Arc::new(InMemoryEncryptionSessionPort::default()),
        Arc::new(PassthroughTransferPayloadDecryptor),
        Arc::new(PassthroughTransferPayloadEncryptor),
        PathBuf::from("/tmp/test-file-cache"),
        PairingRuntimeOwner::CurrentProcess,
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
        check_business_allowed(&resolver, &event_tx, "peer-1", ProtocolDirection::Outbound).await;

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

    let (dial_tx, _dial_rx) = mpsc::channel(4);
    let result = execute_business_stream(
        &control,
        &caches,
        &resolver,
        &event_tx,
        &dial_tx,
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
async fn business_stream_open_attempt_is_scoped_to_stable_span() {
    let captured = Arc::new(Mutex::new(Vec::<(String, Vec<String>)>::new()));
    let subscriber =
        tracing_subscriber::registry().with(EventScopeCaptureLayer::new(captured.clone()));
    let _subscriber_guard = tracing::subscriber::set_default(subscriber);

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
    }

    let resolver: Arc<dyn ConnectionPolicyResolverPort> = Arc::new(FakeResolver);
    let (event_tx, _event_rx) = mpsc::channel(4);
    let uc_peer_id = uc_core::PeerId::from(remote_peer_id.as_str());
    let control = swarm.behaviour().stream.new_control();

    let (dial_tx, _dial_rx) = mpsc::channel(4);
    let result = execute_business_stream(
        &control,
        &caches,
        &resolver,
        &event_tx,
        &dial_tx,
        &uc_peer_id,
        remote_peer,
        Some(b"clipboard"),
        Duration::from_millis(1),
        Duration::from_millis(1),
        Duration::from_millis(1),
        "clipboard",
    )
    .await;

    assert!(
        result.is_err(),
        "unconnected peer should not open business stream"
    );

    let captured = captured.lock().expect("lock captured events");
    let (_, scope) = captured
        .iter()
        .find(|(event_name, _)| event_name == "business_stream.open_attempt")
        .expect("business stream open attempt should be captured");
    assert!(
        scope.iter().any(|span_name| span_name == "business_stream.execute"),
        "business_stream.open_attempt should be emitted inside business_stream.execute span, got scope {scope:?}"
    );
}

#[tokio::test]
async fn inbound_business_denied_drops_stream_and_emits_event() {
    let resolver: Arc<dyn ConnectionPolicyResolverPort> = Arc::new(PendingResolver);
    let (event_tx, mut event_rx) = mpsc::channel(1);

    let result =
        check_business_allowed(&resolver, &event_tx, "peer-2", ProtocolDirection::Inbound).await;

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
        Arc::new(PassthroughTransferPayloadDecryptor),
        Arc::new(PassthroughTransferPayloadEncryptor),
        PathBuf::from("/tmp/test-file-cache"),
        PairingRuntimeOwner::CurrentProcess,
    )
    .expect("create adapter");
    let payload: Arc<[u8]> = Arc::from(vec![1u8, 2, 3, 4].into_boxed_slice());
    let expected_payload = payload.clone();
    let mut rx = Libp2pNetworkAdapter::take_receiver(&adapter.business_rx, "business")
        .expect("business receiver");

    let send_task = tokio::spawn(async move { adapter.send_clipboard("peer-2", payload).await });
    let command = rx.recv().await.expect("business command");
    match command {
        BusinessCommand::SendClipboard {
            peer_id,
            data,
            result_tx,
            ..
        } => {
            assert_eq!(peer_id.as_str(), "peer-2");
            assert_eq!(&*data, &*expected_payload);
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
        Arc::new(PassthroughTransferPayloadDecryptor),
        Arc::new(PassthroughTransferPayloadEncryptor),
        PathBuf::from("/tmp/test-file-cache"),
        PairingRuntimeOwner::CurrentProcess,
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
        Arc::new(PassthroughTransferPayloadDecryptor),
        Arc::new(PassthroughTransferPayloadEncryptor),
        PathBuf::from("/tmp/test-file-cache"),
        PairingRuntimeOwner::CurrentProcess,
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
        Arc::new(PassthroughTransferPayloadDecryptor),
        Arc::new(PassthroughTransferPayloadEncryptor),
        PathBuf::from("/tmp/test-file-cache"),
        PairingRuntimeOwner::CurrentProcess,
    )
    .expect("create adapter a");
    let adapter_b = Libp2pNetworkAdapter::new(
        Arc::new(TestIdentityStore::default()),
        Arc::new(FakeResolver),
        Arc::new(InMemoryEncryptionSessionPort::default()),
        Arc::new(PassthroughTransferPayloadDecryptor),
        Arc::new(PassthroughTransferPayloadEncryptor),
        PathBuf::from("/tmp/test-file-cache"),
        PairingRuntimeOwner::CurrentProcess,
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
        Arc::new(PassthroughTransferPayloadDecryptor),
        Arc::new(PassthroughTransferPayloadEncryptor),
        PathBuf::from("/tmp/test-file-cache"),
        PairingRuntimeOwner::CurrentProcess,
    )
    .expect("create adapter a");
    let adapter_b = Libp2pNetworkAdapter::new(
        Arc::new(TestIdentityStore::default()),
        Arc::new(FakeResolver),
        Arc::new(InMemoryEncryptionSessionPort::default()),
        Arc::new(PassthroughTransferPayloadDecryptor),
        Arc::new(PassthroughTransferPayloadEncryptor),
        PathBuf::from("/tmp/test-file-cache"),
        PairingRuntimeOwner::CurrentProcess,
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
        Arc::new(PassthroughTransferPayloadDecryptor),
        Arc::new(PassthroughTransferPayloadEncryptor),
        PathBuf::from("/tmp/test-file-cache"),
        PairingRuntimeOwner::CurrentProcess,
    )
    .expect("create adapter a");
    let adapter_b = Libp2pNetworkAdapter::new(
        Arc::new(TestIdentityStore::default()),
        Arc::new(FakeResolver),
        Arc::new(InMemoryEncryptionSessionPort::default()),
        Arc::new(PassthroughTransferPayloadDecryptor),
        Arc::new(PassthroughTransferPayloadEncryptor),
        PathBuf::from("/tmp/test-file-cache"),
        PairingRuntimeOwner::CurrentProcess,
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
        payload_version: uc_core::network::protocol::ClipboardPayloadVersion::V3,
        origin_flow_id: None,
        traceparent: None,
        file_transfers: vec![],
    };
    // Use frame_to_bytes for the two-segment wire format (header + no trailing payload for this test)
    let payload: Arc<[u8]> = Arc::from(
        ProtocolMessage::Clipboard(expected.clone())
            .frame_to_bytes(None)
            .expect("serialize clipboard protocol payload with frame_to_bytes")
            .into_boxed_slice(),
    );

    let mut received = None;
    for _attempt in 0..20 {
        ClipboardTransportPort::send_clipboard(&adapter_a, &peer_b, payload.clone())
            .await
            .expect("send clipboard protocol payload");

        match timeout(Duration::from_millis(500), clipboard_rx_b.recv()).await {
            Ok(Some((message, _pre_decoded))) => {
                // This is a test-only scenario without actual encrypted trailing payload
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

#[tokio::test]
async fn subscribe_events_allows_multiple_subscribers_on_one_adapter() {
    let adapter = Libp2pNetworkAdapter::new(
        Arc::new(TestIdentityStore::default()),
        Arc::new(FakeResolver),
        Arc::new(InMemoryEncryptionSessionPort::default()),
        Arc::new(PassthroughTransferPayloadDecryptor),
        Arc::new(PassthroughTransferPayloadEncryptor),
        PathBuf::from("/tmp/test-file-cache"),
        PairingRuntimeOwner::CurrentProcess,
    )
    .expect("create adapter");

    let mut rx_a = adapter
        .subscribe_events()
        .await
        .expect("first subscriber should succeed");
    let mut rx_b = adapter
        .subscribe_events()
        .await
        .expect("second subscriber should also succeed");

    adapter
        .event_tx
        .send(NetworkEvent::Error("fanout".to_string()))
        .await
        .expect("event publish should succeed");

    let event_a = rx_a
        .recv()
        .await
        .expect("first subscriber should receive event");
    let event_b = rx_b
        .recv()
        .await
        .expect("second subscriber should receive event");

    assert!(matches!(event_a, NetworkEvent::Error(ref message) if message == "fanout"));
    assert!(matches!(event_b, NetworkEvent::Error(ref message) if message == "fanout"));
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

// ── Regression tests: staleness must never break sync ────────────────
//
// Context: commit 62320c21 introduced a presence staleness sweep that
// *removed* peers from `discovered_peers` after 20s of no mDNS heartbeat.
// These tests encode the invariant:
//   "Only mDNS Expired events may remove a peer from discovered_peers."
// Any future staleness/offline logic must mark peers (not remove them).

/// Regression: only `apply_mdns_expired` may remove peers from
/// `discovered_peers`.  `remove_discovered` is available but must only be
/// called from the mDNS expiry path.  This test documents the invariant.
#[test]
fn regression_only_mdns_expired_removes_discovered_peer() {
    let mut caches = PeerCaches::new();
    let now = Utc::now();
    let stale_time = now - chrono::Duration::seconds(300);

    caches.upsert_discovered(
        "peer-1".to_string(),
        vec!["/ip4/192.168.1.2/tcp/4001".to_string()],
        stale_time,
    );

    // Peer must persist regardless of last_seen age
    assert!(
        caches.discovered_peers.contains_key("peer-1"),
        "peer must exist in discovered_peers even when last_seen is very old"
    );

    // Only mDNS expired should remove it
    let mut expired = HashSet::new();
    expired.insert("peer-1".to_string());
    let events = apply_mdns_expired(&mut caches, expired);

    assert_eq!(events.len(), 1);
    assert!(!caches.discovered_peers.contains_key("peer-1"));
}

/// Regression: verifies that `discovered_peers` count is not reduced by any
/// non-mDNS mechanism.  If a future PR adds a cleanup/sweep, this test
/// ensures it does not shrink the map.
#[test]
fn regression_discovered_peers_count_stable_without_mdns_expiry() {
    let mut caches = PeerCaches::new();
    let old = Utc::now() - chrono::Duration::seconds(600);
    let now = Utc::now();

    caches.upsert_discovered(
        "very-old-peer".to_string(),
        vec!["/ip4/10.0.0.1/tcp/4001".to_string()],
        old,
    );
    caches.upsert_discovered(
        "fresh-peer".to_string(),
        vec!["/ip4/10.0.0.2/tcp/4001".to_string()],
        now,
    );

    assert_eq!(caches.discovered_peers.len(), 2);

    // mark_unreachable must NOT remove from discovered_peers
    caches.mark_reachable("very-old-peer", old);
    caches.mark_unreachable("very-old-peer");
    assert_eq!(
        caches.discovered_peers.len(),
        2,
        "mark_unreachable must not remove peer from discovered_peers"
    );

    // Only mDNS expiry should reduce count
    let mut expired = HashSet::new();
    expired.insert("very-old-peer".to_string());
    apply_mdns_expired(&mut caches, expired);
    assert_eq!(caches.discovered_peers.len(), 1);
}

#[tokio::test]
async fn get_discovered_peers_excludes_local_peer_id() {
    let adapter = test_adapter(PairingRuntimeOwner::ExternalDaemon);
    let local_id = adapter.local_peer_id();

    // Seed caches: local peer + one remote peer
    {
        let mut caches = adapter.caches.write().await;
        caches.upsert_discovered(
            local_id.clone(),
            vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
            Utc::now(),
        );
        caches.upsert_discovered(
            "remote-peer-abc".to_string(),
            vec!["/ip4/192.168.1.2/tcp/4001".to_string()],
            Utc::now(),
        );
    }

    let peers = PeerDirectoryPort::get_discovered_peers(&adapter)
        .await
        .expect("get_discovered_peers must succeed");

    // local peer must be excluded
    assert!(
        peers.iter().all(|p| p.peer_id != local_id),
        "local_peer_id must not appear in get_discovered_peers result"
    );
    // remote peer must be present
    assert_eq!(peers.len(), 1, "only remote-peer-abc should be returned");
    assert_eq!(peers[0].peer_id, "remote-peer-abc");
}

// ── AddressRegistry integration tests ─────────────────────────

#[test]
fn mdns_expired_preserves_non_mdns_addresses_in_discovered_peers() {
    let mut caches = PeerCaches::new();
    let now = Utc::now();

    // Discover peer via mDNS (LAN address).
    caches.upsert_discovered(
        "peer-1".to_string(),
        vec!["/ip4/192.168.1.5/udp/9000/quic-v1".to_string()],
        now,
    );

    // Also register a WAN address manually in the registry.
    caches.address_registry.register(
        "peer-1",
        "/ip4/203.0.113.1/tcp/8000",
        AddressSource::Manual,
        AddressScope::Wan,
    );
    // Add the WAN address to discovered_peers too (simulating a multi-source peer).
    if let Some(entry) = caches.discovered_peers.get_mut("peer-1") {
        entry
            .addresses
            .push("/ip4/203.0.113.1/tcp/8000".to_string());
    }

    // mDNS expires — should NOT fully remove the peer.
    let removed = caches.remove_discovered("peer-1");
    assert!(
        removed.is_none(),
        "peer should not be fully removed when non-mDNS addresses remain"
    );

    // Peer should still be in discovered_peers with only the WAN address.
    let peer = caches.discovered_peers.get("peer-1").unwrap();
    assert_eq!(peer.addresses.len(), 1);
    assert!(peer.addresses[0].contains("203.0.113.1"));
}

/// Verifies that a peer discovered only via mDNS is fully removed from the caches when mDNS entries are cleared.
///
/// This test inserts a discovered peer with only mDNS-sourced addresses, calls `remove_discovered`, and
/// asserts that the returned value is `Some` (the removed peer) and that the peer no longer exists in
/// `discovered_peers`.
///
/// # Examples
///
/// ```
/// let mut caches = PeerCaches::new();
/// let now = Utc::now();
///
/// caches.upsert_discovered(
///     "peer-1".to_string(),
///     vec!["/ip4/192.168.1.5/tcp/8000".to_string()],
///     now,
/// );
///
/// let removed = caches.remove_discovered("peer-1");
/// assert!(removed.is_some());
/// assert!(caches.discovered_peers.get("peer-1").is_none());
/// ```
#[test]
fn mdns_expired_fully_removes_peer_when_no_other_sources() {
    let mut caches = PeerCaches::new();
    let now = Utc::now();

    caches.upsert_discovered(
        "peer-1".to_string(),
        vec!["/ip4/192.168.1.5/tcp/8000".to_string()],
        now,
    );

    let removed = caches.remove_discovered("peer-1");
    assert!(
        removed.is_some(),
        "peer should be fully removed when only mDNS addresses existed"
    );
    assert!(caches.discovered_peers.get("peer-1").is_none());
}

#[test]
fn infer_address_scope_private_ips_are_lan() {
    assert_eq!(
        infer_address_scope("/ip4/192.168.1.5/udp/9000/quic-v1"),
        AddressScope::Lan
    );
    assert_eq!(
        infer_address_scope("/ip4/10.0.0.1/tcp/8000"),
        AddressScope::Lan
    );
    assert_eq!(
        infer_address_scope("/ip4/172.16.0.1/tcp/8000"),
        AddressScope::Lan
    );
    assert_eq!(
        infer_address_scope("/ip4/127.0.0.1/tcp/8000"),
        AddressScope::Lan
    );
}

#[test]
fn infer_address_scope_public_ips_are_wan() {
    assert_eq!(
        infer_address_scope("/ip4/203.0.113.1/tcp/8000"),
        AddressScope::Wan
    );
    assert_eq!(
        infer_address_scope("/ip4/8.8.8.8/udp/9000/quic-v1"),
        AddressScope::Wan
    );
    // 172.2.x.x is NOT private (only 172.16-31 is) — must be WAN.
    assert_eq!(
        infer_address_scope("/ip4/172.2.0.1/tcp/8000"),
        AddressScope::Wan
    );
}

#[test]
fn infer_address_scope_ipv6_ula_is_lan() {
    // fd00::/8 (ULA)
    assert_eq!(
        infer_address_scope("/ip6/fd12::1/tcp/8000"),
        AddressScope::Lan
    );
    // fc00::/7
    assert_eq!(
        infer_address_scope("/ip6/fc00::1/tcp/8000"),
        AddressScope::Lan
    );
    // fe80::/10 (link-local)
    assert_eq!(
        infer_address_scope("/ip6/fe80::1/tcp/8000"),
        AddressScope::Lan
    );
    // ::1 (loopback)
    assert_eq!(infer_address_scope("/ip6/::1/tcp/8000"), AddressScope::Lan);
    // Global IPv6 — must be WAN.
    assert_eq!(
        infer_address_scope("/ip6/2001:db8::1/tcp/8000"),
        AddressScope::Wan
    );
}

#[test]
fn mdns_expired_preserves_peer_when_non_mdns_addr_in_cooldown() {
    let mut caches = PeerCaches::new();
    let now = Utc::now();

    // Discover peer via mDNS.
    caches.upsert_discovered(
        "peer-1".to_string(),
        vec!["/ip4/192.168.1.5/tcp/8000".to_string()],
        now,
    );

    // Register a WAN address manually and put it in cooldown.
    caches.address_registry.register(
        "peer-1",
        "/ip4/203.0.113.1/tcp/8000",
        AddressSource::Manual,
        AddressScope::Wan,
    );
    caches.address_registry.record_failure(
        "peer-1",
        "/ip4/203.0.113.1/tcp/8000",
        "connection refused",
    );
    // Add to discovered_peers too.
    if let Some(entry) = caches.discovered_peers.get_mut("peer-1") {
        entry
            .addresses
            .push("/ip4/203.0.113.1/tcp/8000".to_string());
    }

    // mDNS expires — WAN address is cooling down but should NOT cause peer removal.
    let removed = caches.remove_discovered("peer-1");
    assert!(
        removed.is_none(),
        "peer should not be removed when non-mDNS address exists even in cooldown"
    );
    assert!(caches.discovered_peers.get("peer-1").is_some());
}

#[test]
fn infer_address_scope_relay_detected() {
    assert_eq!(
        infer_address_scope("/ip4/203.0.113.1/tcp/8000/p2p-circuit"),
        AddressScope::Relay
    );
}

#[test]
fn inbound_connection_uses_inferred_scope() {
    let mut caches = PeerCaches::new();
    let now = Utc::now();

    // Inbound from a public IP.
    let wan_addr: Multiaddr = "/ip4/203.0.113.1/tcp/8000".parse().unwrap();
    caches.upsert_discovered_from_connection("peer-1", wan_addr, now);

    let records = caches.address_registry.all_for("peer-1");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].source, AddressSource::Inbound);
    assert_eq!(records[0].scope, AddressScope::Wan);

    // Inbound from a private IP.
    let lan_addr: Multiaddr = "/ip4/192.168.1.5/udp/9000/quic-v1".parse().unwrap();
    caches.upsert_discovered_from_connection("peer-2", lan_addr, now);

    let records = caches.address_registry.all_for("peer-2");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].scope, AddressScope::Lan);
}
