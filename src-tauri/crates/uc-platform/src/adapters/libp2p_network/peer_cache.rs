//! Peer state caches — tracks discovered, reachable, and connected peers along
//! with address lifecycle metadata and dial observations.

use chrono::{DateTime, Utc};
use libp2p::swarm::ConnectionId;
use std::collections::{HashMap, HashSet};
use uc_core::network::address_registry::{AddressRegistry, AddressScope, AddressSource};
use uc_core::network::DiscoveredPeer;

use super::dial_strategy::{
    effective_priority_for_addr, infer_address_scope, sort_addresses_quic_first,
};

// ── Supporting types ──────────────────────────────────────────

/// Records the outcome of a single outbound dial attempt to a peer.
#[derive(Debug, Clone)]
pub(crate) struct PeerDialObservation {
    pub chosen_dial_addr: Option<String>,
    pub chosen_dial_addr_resolution: &'static str,
    pub dial_attempt_addresses: Vec<String>,
    pub dial_outcome: &'static str,
    pub observed_at: DateTime<Utc>,
}

/// Tracks one active swarm connection for a peer so dial decisions can compare
/// the current path against newly discovered candidates.
#[derive(Debug, Clone)]
pub(crate) struct ActivePeerConnection {
    pub address: Option<String>,
    pub connected_at: DateTime<Utc>,
}

impl ActivePeerConnection {
    pub fn effective_priority(&self) -> Option<u8> {
        self.address.as_deref().map(effective_priority_for_addr)
    }
}

/// A point-in-time snapshot of a peer's address state used for dial decisions
/// and observability logging.
#[derive(Debug, Clone)]
pub(crate) struct PeerAddressSnapshot {
    pub candidate_addresses: Vec<String>,
    #[allow(dead_code)] // retained for Debug snapshot diagnostics
    pub connected_addresses: Vec<String>,
    pub connected_address_count: usize,
    pub peer_marked_reachable: bool,
    pub connected_age_ms: Option<i64>,
    pub discovered_age_ms: Option<i64>,
    pub last_seen_age_ms: Option<i64>,
    pub best_connected_address: Option<String>,
    pub best_connected_effective_priority: Option<u8>,
    pub chosen_dial_addr: Option<String>,
    pub chosen_dial_addr_resolution: Option<&'static str>,
    pub dial_attempt_addresses: Vec<String>,
    pub dial_attempt_address_count: usize,
    pub last_dial_outcome: Option<&'static str>,
    pub last_dial_age_ms: Option<i64>,
    pub last_dial_observed_at: Option<DateTime<Utc>>,
}

// ── PeerCaches ────────────────────────────────────────────────

/// Central cache of peer state for the libp2p network adapter.
///
/// Thread-safety is managed externally via `Arc<RwLock<PeerCaches>>`.
pub struct PeerCaches {
    pub(crate) discovered_peers: HashMap<String, DiscoveredPeer>,
    pub(crate) reachable_peers: HashSet<String>,
    pub(crate) connected_at: HashMap<String, DateTime<Utc>>,
    pub(crate) active_connections: HashMap<String, HashMap<ConnectionId, ActivePeerConnection>>,
    pub(crate) last_dial_observations: HashMap<String, PeerDialObservation>,
    pub(crate) address_registry: AddressRegistry,
    /// Consecutive outgoing dial failures per peer, reset on connection success.
    pub(crate) consecutive_dial_failures: HashMap<String, u32>,
}

impl PeerCaches {
    /// Creates an empty `PeerCaches` instance with a fresh `AddressRegistry`.
    pub fn new() -> Self {
        Self {
            discovered_peers: HashMap::new(),
            reachable_peers: HashSet::new(),
            connected_at: HashMap::new(),
            active_connections: HashMap::new(),
            last_dial_observations: HashMap::new(),
            address_registry: AddressRegistry::new(),
            consecutive_dial_failures: HashMap::new(),
        }
    }

    /// Inserts or updates a peer discovered via mDNS, registers each discovered
    /// address in the address registry, and preserves any existing device name
    /// and device id from a prior entry.
    pub fn upsert_discovered(
        &mut self,
        peer_id: String,
        mut addresses: Vec<String>,
        discovered_at: DateTime<Utc>,
    ) -> DiscoveredPeer {
        sort_addresses_quic_first(&mut addresses);

        for addr in &addresses {
            self.address_registry
                .register(&peer_id, addr, AddressSource::Mdns, AddressScope::Lan);
        }

        let (existing_name, existing_device_id) = self
            .discovered_peers
            .get(&peer_id)
            .map(|p| (p.device_name.clone(), p.device_id.clone()))
            .unwrap_or((None, None));
        let peer = DiscoveredPeer {
            peer_id,
            device_name: existing_name,
            device_id: existing_device_id,
            addresses,
            discovered_at,
            last_seen: discovered_at,
            is_paired: false,
        };
        self.discovered_peers
            .insert(peer.peer_id.clone(), peer.clone());
        peer
    }

    /// Insert or update a discovered peer address observed from a direct connection.
    ///
    /// Registers the observed multiaddr in the address registry with an inferred scope.
    ///
    /// Returns `true` if the peer's address list was modified.
    pub fn upsert_discovered_from_connection(
        &mut self,
        peer_id: &str,
        address: libp2p::Multiaddr,
        observed_at: DateTime<Utc>,
    ) -> bool {
        let address = address.to_string();

        let scope = infer_address_scope(&address);
        self.address_registry
            .register(peer_id, &address, AddressSource::Inbound, scope);

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

    /// Remove mDNS-sourced addresses for a discovered peer and remove the peer
    /// entry only when no other addresses remain.
    ///
    /// Returns `Some(DiscoveredPeer)` if fully removed; `None` if retained or
    /// did not exist.
    ///
    /// `last_dial_observations` is intentionally preserved: the recovery layer
    /// uses the last known usable dial target to retry after a transient mDNS
    /// drop or a local-session rebuild. Call [`PeerCaches::forget_peer`] to
    /// fully erase a peer (e.g. on unpair).
    pub fn remove_discovered(&mut self, peer_id: &str) -> Option<DiscoveredPeer> {
        self.address_registry
            .remove_peer_source(peer_id, AddressSource::Mdns);

        let remaining: Vec<String> = self
            .address_registry
            .all_for(peer_id)
            .iter()
            .map(|r| r.addr.clone())
            .collect();

        if !remaining.is_empty() {
            if let Some(entry) = self.discovered_peers.get_mut(peer_id) {
                entry.addresses = remaining;
                sort_addresses_quic_first(&mut entry.addresses);
            }
            return None;
        }

        // Only strip connection state when there is no live connection.
        // mark_connection_closed() / forget_peer() own connection teardown.
        if !self.active_connections.contains_key(peer_id) {
            self.reachable_peers.remove(peer_id);
            self.connected_at.remove(peer_id);
        }
        self.discovered_peers.remove(peer_id)
    }

    /// Fully erase a peer from every cache, including `last_dial_observations`
    /// and the address registry. Intended for unpair / explicit forget flows.
    pub fn forget_peer(&mut self, peer_id: &str) -> Option<DiscoveredPeer> {
        self.address_registry.remove_peer(peer_id);
        self.reachable_peers.remove(peer_id);
        self.connected_at.remove(peer_id);
        self.active_connections.remove(peer_id);
        self.last_dial_observations.remove(peer_id);
        self.consecutive_dial_failures.remove(peer_id);
        self.discovered_peers.remove(peer_id)
    }

    pub fn mark_reachable(&mut self, peer_id: &str, connected_at: DateTime<Utc>) -> bool {
        if self.discovered_peers.contains_key(peer_id) {
            self.reachable_peers.insert(peer_id.to_string());
            self.connected_at
                .entry(peer_id.to_string())
                .or_insert(connected_at);
            self.consecutive_dial_failures.remove(peer_id);
            true
        } else {
            false
        }
    }

    /// Mark a peer as unreachable, returning `true` if it was previously
    /// reachable.
    ///
    /// `last_dial_observations` is intentionally preserved so the recovery
    /// layer can retry the last known usable path after a transient outage.
    pub fn mark_unreachable(&mut self, peer_id: &str) -> bool {
        let removed = self.reachable_peers.remove(peer_id);
        self.connected_at.remove(peer_id);
        self.active_connections.remove(peer_id);
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

    pub(crate) fn has_active_connections(&self, peer_id: &str) -> bool {
        self.active_connections
            .get(peer_id)
            .is_some_and(|connections| !connections.is_empty())
    }

    pub(crate) fn record_dial_observation(
        &mut self,
        peer_id: &str,
        observation: PeerDialObservation,
    ) {
        self.last_dial_observations
            .insert(peer_id.to_string(), observation);
    }

    /// Increment the consecutive dial failure counter for `peer_id` and return
    /// the new count.
    pub(crate) fn record_dial_failure(&mut self, peer_id: &str) -> u32 {
        let count = self
            .consecutive_dial_failures
            .entry(peer_id.to_string())
            .or_insert(0);
        *count += 1;
        *count
    }

    pub(crate) fn record_address_success(&mut self, peer_id: &str, addr: &str) {
        self.address_registry.record_success(peer_id, addr);
    }

    pub(crate) fn record_address_failure(&mut self, peer_id: &str, addr: &str, error: &str) {
        self.address_registry.record_failure(peer_id, addr, error);
    }

    pub(crate) fn mark_connection_established(
        &mut self,
        peer_id: &str,
        connection_id: ConnectionId,
        address: Option<String>,
        connected_at: DateTime<Utc>,
    ) -> bool {
        let was_reachable = self.is_reachable(peer_id);
        let connections = self
            .active_connections
            .entry(peer_id.to_string())
            .or_default();
        connections.insert(
            connection_id,
            ActivePeerConnection {
                address,
                connected_at,
            },
        );

        if self.discovered_peers.contains_key(peer_id) {
            self.reachable_peers.insert(peer_id.to_string());
            if let Some(first_connected_at) =
                connections.values().map(|conn| conn.connected_at).min()
            {
                self.connected_at
                    .insert(peer_id.to_string(), first_connected_at);
            }
        }

        self.consecutive_dial_failures.remove(peer_id);

        !was_reachable && self.is_reachable(peer_id)
    }

    /// Mark a connection as closed. Returns `true` if the peer transitioned
    /// from reachable to unreachable (i.e., the last active connection closed).
    ///
    /// `last_dial_observations` is intentionally preserved so the recovery
    /// coordinator can retry the last known usable path after a transient
    /// connection drop. Call [`PeerCaches::forget_peer`] to fully erase a peer.
    pub(crate) fn mark_connection_closed(
        &mut self,
        peer_id: &str,
        connection_id: ConnectionId,
    ) -> bool {
        let was_reachable = self.is_reachable(peer_id);

        if let Some(connections) = self.active_connections.get_mut(peer_id) {
            connections.remove(&connection_id);
            if connections.is_empty() {
                self.active_connections.remove(peer_id);
                self.reachable_peers.remove(peer_id);
                self.connected_at.remove(peer_id);
                // last_dial_observations intentionally not removed here;
                // recovery layer needs the last usable path.
            } else if let Some(first_connected_at) =
                connections.values().map(|conn| conn.connected_at).min()
            {
                self.connected_at
                    .insert(peer_id.to_string(), first_connected_at);
            }
        }

        was_reachable && !self.is_reachable(peer_id)
    }

    pub(crate) fn inferior_connection_ids(&self, peer_id: &str) -> Vec<ConnectionId> {
        let Some(connections) = self.active_connections.get(peer_id) else {
            return Vec::new();
        };

        let Some(best_priority) = connections
            .values()
            .filter_map(ActivePeerConnection::effective_priority)
            .min()
        else {
            return Vec::new();
        };

        connections
            .iter()
            .filter_map(|(connection_id, connection)| {
                connection
                    .effective_priority()
                    .filter(|priority| *priority > best_priority)
                    .map(|_| *connection_id)
            })
            .collect()
    }

    pub(crate) fn gc_address_registry(&mut self) -> usize {
        self.address_registry.gc()
    }
}

// ── Snapshot helpers ──────────────────────────────────────────

/// Capture a point-in-time snapshot of a peer's address state from caches.
pub(crate) fn snapshot_peer_addresses(
    caches: &PeerCaches,
    peer_id: &str,
    observed_at: DateTime<Utc>,
) -> PeerAddressSnapshot {
    let discovered = caches.discovered_peers.get(peer_id);
    let last_dial = caches.last_dial_observations.get(peer_id);
    let connected_addresses = caches
        .active_connections
        .get(peer_id)
        .map(|connections| {
            connections
                .values()
                .filter_map(|connection| connection.address.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let best_connected = caches
        .active_connections
        .get(peer_id)
        .and_then(|connections| {
            connections
                .values()
                .filter_map(|connection| {
                    connection.address.as_ref().map(|address| {
                        (
                            address.clone(),
                            effective_priority_for_addr(address),
                            connection.connected_at,
                        )
                    })
                })
                .min_by_key(|(_, priority, _)| *priority)
        });
    PeerAddressSnapshot {
        candidate_addresses: caches
            .address_registry
            .candidates_for(peer_id)
            .iter()
            .map(|r| r.addr.clone())
            .collect(),
        connected_address_count: connected_addresses.len(),
        connected_addresses,
        peer_marked_reachable: caches.is_reachable(peer_id),
        connected_age_ms: best_connected
            .as_ref()
            .map(|(_, _, connected_at)| age_ms(observed_at, *connected_at))
            .or_else(|| {
                caches
                    .connected_at
                    .get(peer_id)
                    .map(|connected_at| age_ms(observed_at, *connected_at))
            }),
        discovered_age_ms: discovered.map(|peer| age_ms(observed_at, peer.discovered_at)),
        last_seen_age_ms: discovered.map(|peer| age_ms(observed_at, peer.last_seen)),
        best_connected_address: best_connected
            .as_ref()
            .map(|(address, _, _)| address.clone()),
        best_connected_effective_priority: best_connected
            .as_ref()
            .map(|(_, priority, _)| *priority),
        chosen_dial_addr: last_dial.and_then(|dial| dial.chosen_dial_addr.clone()),
        chosen_dial_addr_resolution: last_dial.map(|dial| dial.chosen_dial_addr_resolution),
        dial_attempt_addresses: last_dial
            .map(|dial| dial.dial_attempt_addresses.clone())
            .unwrap_or_default(),
        dial_attempt_address_count: last_dial
            .map(|dial| dial.dial_attempt_addresses.len())
            .unwrap_or(0),
        last_dial_outcome: last_dial.map(|dial| dial.dial_outcome),
        last_dial_age_ms: last_dial.map(|dial| age_ms(observed_at, dial.observed_at)),
        last_dial_observed_at: last_dial.map(|dial| dial.observed_at),
    }
}

/// Compute elapsed milliseconds between two timestamps (clamped to >= 0).
pub(crate) fn age_ms(observed_at: DateTime<Utc>, recorded_at: DateTime<Utc>) -> i64 {
    observed_at
        .signed_duration_since(recorded_at)
        .num_milliseconds()
        .max(0)
}
