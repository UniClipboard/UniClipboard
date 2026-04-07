//! Dial strategy helpers — address classification, transport labelling, dial
//! decision logic, and observation construction.

use chrono::{DateTime, Utc};
use libp2p::Multiaddr;
use uc_core::network::address_registry::AddressScope;

use super::peer_cache::{PeerAddressSnapshot, PeerDialObservation};

// ── Address classification ───────────────────────────────────

/// Classifies a libp2p multiaddr string as LAN, WAN, or Relay.
///
/// - `AddressScope::Relay` for addresses containing `/p2p-circuit`.
/// - `AddressScope::Lan` for loopback, private, or link-local IPv4; loopback,
///   link-local, or ULA IPv6.
/// - `AddressScope::Wan` otherwise.
pub(crate) fn infer_address_scope(addr: &str) -> AddressScope {
    if addr.contains("/p2p-circuit") {
        return AddressScope::Relay;
    }

    let parts: Vec<&str> = addr.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        match *part {
            "ip4" => {
                if let Some(ip_str) = parts.get(i + 1) {
                    if let Ok(ip) = ip_str.parse::<std::net::Ipv4Addr>() {
                        return if ip.is_loopback() || ip.is_private() || ip.is_link_local() {
                            AddressScope::Lan
                        } else {
                            AddressScope::Wan
                        };
                    }
                }
            }
            "ip6" => {
                if let Some(ip_str) = parts.get(i + 1) {
                    if let Ok(ip) = ip_str.parse::<std::net::Ipv6Addr>() {
                        let octets = ip.octets();
                        let is_loopback = ip.is_loopback();
                        let is_link_local = octets[0] == 0xfe && (octets[1] & 0xc0) == 0x80;
                        let is_ula = (octets[0] & 0xfe) == 0xfc;
                        return if is_loopback || is_link_local || is_ula {
                            AddressScope::Lan
                        } else {
                            AddressScope::Wan
                        };
                    }
                }
            }
            _ => {}
        }
    }

    AddressScope::Wan
}

/// Sorts multiaddress strings in-place so that QUIC addresses come first.
pub(crate) fn sort_addresses_quic_first(addresses: &mut Vec<String>) {
    addresses.sort_by_key(|addr| if addr.contains("/quic-v1") { 0 } else { 1 });
}

// ── Transport labelling ──────────────────────────────────────

pub(crate) fn transport_label(address: &Multiaddr) -> &'static str {
    transport_label_str(&address.to_string())
}

pub(crate) fn transport_label_str(address: &str) -> &'static str {
    if address.contains("/quic-v1") {
        "quic"
    } else if address.contains("/tcp/") {
        "tcp"
    } else {
        "other"
    }
}

// ── Dial decisions ───────────────────────────────────────────

pub(crate) fn dial_decision_for_snapshot(snapshot: &PeerAddressSnapshot) -> &'static str {
    if snapshot.peer_marked_reachable {
        "reuse_existing_connection"
    } else {
        "new_dial_required"
    }
}

pub(crate) fn preferred_candidate_transport(snapshot: &PeerAddressSnapshot) -> &'static str {
    snapshot
        .candidate_addresses
        .first()
        .map(|addr| transport_label_str(addr))
        .unwrap_or("none")
}

pub(crate) fn infer_chosen_dial_addr_resolution(
    snapshot: &PeerAddressSnapshot,
    dial_decision: &str,
    attempt_started_at: DateTime<Utc>,
) -> &'static str {
    if dial_decision == "reuse_existing_connection" {
        "not_applicable"
    } else if snapshot
        .last_dial_observed_at
        .is_some_and(|observed_at| observed_at >= attempt_started_at)
    {
        snapshot.chosen_dial_addr_resolution.unwrap_or("unknown")
    } else if !snapshot.peer_marked_reachable && snapshot.candidate_addresses.len() == 1 {
        "single_candidate_inferred"
    } else {
        "unknown"
    }
}

pub(crate) fn chosen_dial_addr_for_log<'a>(
    snapshot: &'a PeerAddressSnapshot,
    dial_decision: &str,
    attempt_started_at: DateTime<Utc>,
) -> Option<&'a str> {
    if dial_decision == "reuse_existing_connection" {
        None
    } else if snapshot
        .last_dial_observed_at
        .is_some_and(|observed_at| observed_at >= attempt_started_at)
    {
        snapshot.chosen_dial_addr.as_deref()
    } else if !snapshot.peer_marked_reachable && snapshot.candidate_addresses.len() == 1 {
        Some(snapshot.candidate_addresses[0].as_str())
    } else {
        None
    }
}

// ── Dial observations ────────────────────────────────────────

pub(crate) fn successful_dial_observation(
    address: &str,
    observed_at: DateTime<Utc>,
) -> PeerDialObservation {
    PeerDialObservation {
        chosen_dial_addr: Some(address.to_string()),
        chosen_dial_addr_resolution: "exact",
        dial_attempt_addresses: vec![address.to_string()],
        dial_outcome: "connection_established",
        observed_at,
    }
}

pub(crate) fn dial_observation_from_error(
    error: &libp2p::swarm::DialError,
    observed_at: DateTime<Utc>,
) -> PeerDialObservation {
    match error {
        libp2p::swarm::DialError::LocalPeerId { address } => PeerDialObservation {
            chosen_dial_addr: Some(address.to_string()),
            chosen_dial_addr_resolution: "exact",
            dial_attempt_addresses: vec![address.to_string()],
            dial_outcome: "local_peer_id",
            observed_at,
        },
        libp2p::swarm::DialError::WrongPeerId { address, .. } => PeerDialObservation {
            chosen_dial_addr: Some(address.to_string()),
            chosen_dial_addr_resolution: "exact",
            dial_attempt_addresses: vec![address.to_string()],
            dial_outcome: "wrong_peer_id",
            observed_at,
        },
        libp2p::swarm::DialError::Transport(errors) => {
            let dial_attempt_addresses = errors
                .iter()
                .map(|(address, _)| address.to_string())
                .collect::<Vec<_>>();
            let chosen_dial_addr = if dial_attempt_addresses.len() == 1 {
                dial_attempt_addresses.first().cloned()
            } else {
                None
            };
            let chosen_dial_addr_resolution = if chosen_dial_addr.is_some() {
                "exact"
            } else if dial_attempt_addresses.is_empty() {
                "unknown"
            } else {
                "multiple_attempts"
            };
            PeerDialObservation {
                chosen_dial_addr,
                chosen_dial_addr_resolution,
                dial_attempt_addresses,
                dial_outcome: "transport_error",
                observed_at,
            }
        }
        libp2p::swarm::DialError::NoAddresses => PeerDialObservation {
            chosen_dial_addr: None,
            chosen_dial_addr_resolution: "no_addresses",
            dial_attempt_addresses: Vec::new(),
            dial_outcome: "no_addresses",
            observed_at,
        },
        libp2p::swarm::DialError::DialPeerConditionFalse(_) => PeerDialObservation {
            chosen_dial_addr: None,
            chosen_dial_addr_resolution: "peer_condition_false",
            dial_attempt_addresses: Vec::new(),
            dial_outcome: "peer_condition_false",
            observed_at,
        },
        libp2p::swarm::DialError::Aborted => PeerDialObservation {
            chosen_dial_addr: None,
            chosen_dial_addr_resolution: "aborted",
            dial_attempt_addresses: Vec::new(),
            dial_outcome: "aborted",
            observed_at,
        },
        libp2p::swarm::DialError::Denied { .. } => PeerDialObservation {
            chosen_dial_addr: None,
            chosen_dial_addr_resolution: "denied",
            dial_attempt_addresses: Vec::new(),
            dial_outcome: "denied",
            observed_at,
        },
    }
}
