//! mDNS discovery event processing — pure functions that translate mDNS
//! discovery/expiry events into cache mutations and `NetworkEvent` outputs.

use chrono::{DateTime, Utc};
use libp2p::swarm::ConnectionId;
use libp2p::{Multiaddr, PeerId};
use std::collections::{HashMap, HashSet};
use uc_core::network::NetworkEvent;

use super::peer_cache::PeerCaches;

/// Collect discovered peers from mDNS into a map of peer_id → addresses.
pub(crate) fn collect_mdns_discovered(
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

/// Collect expired peers from mDNS into a set of peer IDs.
pub(crate) fn collect_mdns_expired(
    peers: impl IntoIterator<Item = (PeerId, Multiaddr)>,
) -> HashSet<String> {
    let mut expired = HashSet::new();
    for (peer_id, _) in peers {
        expired.insert(peer_id.to_string());
    }
    expired
}

/// Apply mDNS discovered peers to caches, returning `PeerDiscovered` events.
pub(crate) fn apply_mdns_discovered(
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

/// Apply mDNS expired peers to caches, returning `PeerLost` events for fully
/// removed peers.
pub(crate) fn apply_mdns_expired(
    caches: &mut PeerCaches,
    expired: HashSet<String>,
) -> Vec<NetworkEvent> {
    expired
        .into_iter()
        .filter_map(|peer_id| {
            caches
                .remove_discovered(&peer_id)
                .map(|_| NetworkEvent::PeerLost(peer_id))
        })
        .collect()
}

/// Mark a peer as reachable, returning a `PeerReady` event if the state changed.
pub(crate) fn apply_peer_ready(
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

/// Register a connection-observed address and mark the peer as reachable.
pub(crate) fn apply_peer_ready_from_connection(
    caches: &mut PeerCaches,
    peer_id: &str,
    connection_id: ConnectionId,
    connected_at: DateTime<Utc>,
    address: Option<libp2p::Multiaddr>,
) -> Option<NetworkEvent> {
    let address_string = address.as_ref().map(|a| a.to_string());
    if let Some(address) = address {
        caches.upsert_discovered_from_connection(peer_id, address, connected_at);
    }
    if caches.mark_connection_established(peer_id, connection_id, address_string, connected_at) {
        Some(NetworkEvent::PeerReady {
            peer_id: peer_id.to_string(),
        })
    } else {
        None
    }
}

/// Mark a peer as unreachable, returning a `PeerNotReady` event if the state changed.
pub(crate) fn apply_peer_not_ready(caches: &mut PeerCaches, peer_id: &str) -> Option<NetworkEvent> {
    if caches.mark_unreachable(peer_id) {
        Some(NetworkEvent::PeerNotReady {
            peer_id: peer_id.to_string(),
        })
    } else {
        None
    }
}
