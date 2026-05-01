//! Helper that turns an iroh [`EndpointAddr`] freshly observed from a
//! local [`iroh::Endpoint`] into a form that is **safe to persist** in
//! [`crate::storage::peer_address::DieselPeerAddressRepository`] (or any
//! other [`PeerAddressRepositoryPort`] implementor) for the lifetime of
//! a paired peer.
//!
//! ## Why a transformation is needed at all
//!
//! `endpoint.addr()` returns the endpoint's **current** view of itself:
//! the persistent NodeId, every direct UDP socket address magicsock has
//! discovered, and (when relays are enabled) the assigned relay URL.
//! Two of those parts have very different lifetimes:
//!
//! | Component | Stable across process restart? |
//! |-----------|--------------------------------|
//! | NodeId | Yes — derived from the persistent secret key |
//! | Relay URL | Yes — sticky per home-region selection |
//! | Direct `Ip(SocketAddr)` | **No** — magicsock binds a fresh random UDP port on every start, and NAT mappings rotate independently |
//!
//! Persisting the direct addresses bakes the **pairing-time UDP port**
//! into our repository. After the peer's daemon restarts the stored
//! port no longer matches the listening socket; packets sent to it land
//! on a closed socket, the kernel silently drops them, and `iroh`
//! `Endpoint::connect` waits the full QUIC handshake budget (~30 s) for
//! a reply that never comes. Real-device test runs surfaced this as
//! every dispatch attempt logging `→ Offline` even though both peers
//! were demonstrably up on the same LAN.
//!
//! ## Why this transformation lives at the producer (write side)
//!
//! Stripping at the read side is a patch — a translation layer that
//! quietly disagrees with what the repository claims to hold. By
//! running the transformation **before** the blob enters the
//! repository, the repository's contract becomes truthful: a stored
//! [`PeerAddressRecord`] only carries identity (NodeId) and a long-
//! lived hint (Relay), exactly the parts that survive a peer restart.
//! Read sites then decode and dial directly with no further massaging,
//! and `iroh`'s built-in pkarr discovery fills in the peer's *currently
//! published* direct addresses for each connect attempt.
//!
//! ## Conditional behaviour: keep direct addrs when no relay is present
//!
//! With no relay in the input there is nothing for `iroh` discovery to
//! fall back on once the directs are gone — connect would have no path
//! to try. Unit fixtures that bind endpoints with `RelayMode::Disabled`
//! (loopback-only tests) depend on this branch: their stored blobs are
//! direct-only by design. Production daemons run with the default
//! `RelayMode::Default`, which always assigns a relay URL, so the
//! stripping branch is the production path.
//!
//! [`PeerAddressRepositoryPort`]: uc_core::ports::PeerAddressRepositoryPort
//! [`PeerAddressRecord`]: uc_core::ports::PeerAddressRecord

use iroh::{EndpointAddr, TransportAddr};

/// Convert a freshly observed [`EndpointAddr`] into the form we want to
/// persist for a paired peer: NodeId + relay hint, with ephemeral
/// `Ip(...)` direct addresses dropped. When the input carries no relay
/// the addr is returned unchanged so the caller still has dialable
/// paths; see the module doc for the rationale.
pub fn to_persistable_addr(addr: EndpointAddr) -> EndpointAddr {
    let has_relay = addr
        .addrs
        .iter()
        .any(|a| matches!(a, TransportAddr::Relay(_)));
    if !has_relay {
        return addr;
    }

    let EndpointAddr { id, addrs } = addr;
    let kept = addrs
        .into_iter()
        .filter(|addr| !matches!(addr, TransportAddr::Ip(_)));
    EndpointAddr::from_parts(id, kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    use iroh::{EndpointId, RelayUrl, SecretKey};

    fn test_id() -> EndpointId {
        SecretKey::generate().public()
    }

    fn lan_addr(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 5), port))
    }

    fn wan_addr(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(1, 2, 3, 4), port))
    }

    #[test]
    fn drops_ip_entries_keeps_relay() {
        let id = test_id();
        let relay: RelayUrl = "https://relay.example.com/".parse().unwrap();

        let full = EndpointAddr::from_parts(
            id,
            [
                TransportAddr::Ip(lan_addr(50754)),
                TransportAddr::Ip(wan_addr(59875)),
                TransportAddr::Relay(relay.clone()),
            ],
        );

        let persisted = to_persistable_addr(full);
        assert_eq!(persisted.id, id);
        assert_eq!(persisted.addrs.len(), 1);
        assert!(persisted
            .addrs
            .iter()
            .all(|a| matches!(a, TransportAddr::Relay(_))));
    }

    #[test]
    fn id_only_input_is_unchanged() {
        let id = test_id();
        let addr = EndpointAddr::new(id);
        let persisted = to_persistable_addr(addr);
        assert_eq!(persisted.id, id);
        assert!(persisted.addrs.is_empty());
    }

    #[test]
    fn ip_only_input_is_kept_intact() {
        // No relay present → no discovery fallback to rely on → keep the
        // IPs as the only dialable paths. Mirrors test fixtures that
        // bind endpoints with `RelayMode::Disabled`.
        let id = test_id();
        let addr = EndpointAddr::from_parts(id, [TransportAddr::Ip(lan_addr(50754))]);
        let persisted = to_persistable_addr(addr);
        assert_eq!(persisted.id, id);
        assert_eq!(persisted.addrs.len(), 1);
        assert!(persisted
            .addrs
            .iter()
            .all(|a| matches!(a, TransportAddr::Ip(_))));
    }

    #[test]
    fn relay_only_input_is_unchanged() {
        let id = test_id();
        let relay: RelayUrl = "https://relay.example.com/".parse().unwrap();
        let addr = EndpointAddr::from_parts(id, [TransportAddr::Relay(relay)]);
        let persisted = to_persistable_addr(addr);
        assert_eq!(persisted.id, id);
        assert_eq!(persisted.addrs.len(), 1);
        assert!(persisted
            .addrs
            .iter()
            .all(|a| matches!(a, TransportAddr::Relay(_))));
    }
}
