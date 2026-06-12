//! Window-scoped mDNS announce for an in-flight pairing invitation.
//!
//! Lives only while a code is `Pending` in the sponsor's holder. A new
//! [`MdnsPairingPublisher`] is started when `issue_invitation` mints a
//! code; the returned [`PublisherHandle`] holds the `DropGuard` returned
//! by `swarm_discovery::Discoverer::spawn`. Dropping the handle stops
//! the announce — used by `consume_invitation` (joiner showed up) and
//! `cancel_all` (user cancelled or process exit).
//!
//! ## Why a dedicated `Discoverer` instead of riding iroh's
//!
//! iroh's `MdnsAddressLookup` is fixed at Endpoint bind time and runs
//! for the endpoint's life. We need:
//!
//! 1. A separate service name so peer-discovery TXT records and pairing
//!    TXT records can't be confused.
//! 2. A lifecycle that starts at "user clicked Create Invitation" and
//!    stops at "joiner showed up or code expired" — which iroh has no
//!    hook for.
//! 3. A privacy posture: a daemon at idle is **silent** on the LAN, not
//!    constantly broadcasting its NodeId tagged with a code hash.
//!
//! Sharing iroh's `Discoverer` would force compromises on all three.

use std::net::IpAddr;
use std::time::Duration;

use swarm_discovery::{Discoverer, DropGuard, IpClass, SpawnError, TxtAttributeError};
use thiserror::Error;
use tokio::runtime::Handle;
use tracing::{debug, info, warn};

use super::discovery_constants::{
    compute_code_hash, PAIR_SERVICE_NAME, TXT_CODE_HASH, TXT_EXPIRES_AT_MS, TXT_NODE_ID, TXT_TICKET,
};

/// Default mDNS query/announce cadence for pairing. Tighter than the
/// 10s default `swarm-discovery` ships because pairing is a UX-critical
/// moment — the joiner is *waiting on the screen* — and the window is
/// short anyway (typically <5 min). Trade-off is more multicast packets;
/// at one TXT record per device for the duration of the window this is
/// well below "noticed by other LAN traffic."
const PAIR_CADENCE: Duration = Duration::from_secs(2);

/// Errors raised while starting a publisher. Stopping is infallible
/// (`Drop` impl).
#[derive(Debug, Error)]
pub enum MdnsPublisherError {
    /// `swarm-discovery` couldn't bind a multicast socket on any local
    /// interface (IPv4 or IPv6). Sponsor's UI surface this as "LAN
    /// channel unavailable" while the cloud channel may still work.
    #[error("mDNS socket bind failed: {0}")]
    SocketBind(String),

    /// The TXT record we tried to publish exceeded `swarm-discovery`'s
    /// per-attribute size budget. Should not happen with the fixed
    /// shape we use — surfaced for defence in depth.
    #[error("TXT attribute too long: {0}")]
    TxtTooLong(String),
}

impl From<SpawnError> for MdnsPublisherError {
    fn from(err: SpawnError) -> Self {
        Self::SocketBind(err.to_string())
    }
}

impl From<TxtAttributeError> for MdnsPublisherError {
    fn from(err: TxtAttributeError) -> Self {
        Self::TxtTooLong(err.to_string())
    }
}

/// Active mDNS announce. Drop to stop announcing. Cheap to construct —
/// the actual multicast loop runs on a tokio task held inside the
/// inner `DropGuard`.
#[must_use = "dropping this handle stops the mDNS pairing announce"]
pub struct PublisherHandle {
    _guard: DropGuard,
}

/// Factory for [`PublisherHandle`]. Stateless — each call to
/// [`start`](MdnsPairingPublisher::start) spawns an independent
/// `swarm_discovery::Discoverer` actor with its own multicast sockets.
pub struct MdnsPairingPublisher;

impl MdnsPairingPublisher {
    /// Spawns a fresh announce for `code`.
    ///
    /// `node_id`, `ticket_hex` and `expires_at_ms` go into the TXT
    /// record; `code` itself is hashed (not broadcast) per the privacy
    /// design (see `discovery_constants.rs`). The advertised IP set is
    /// enumerated from local interfaces via `if-addrs`; an empty set is
    /// not an error (`swarm-discovery` will still listen, it just won't
    /// have an address to publish — useful for tests on loopback).
    ///
    /// `port` is the sponsor's iroh endpoint port. Joiner reads this off
    /// the resolved `Peer.addrs` and uses it directly as the
    /// `EndpointAddr` socket port.
    ///
    /// `node_id` is hashed to derive a DNS-label-safe instance name
    /// (swarm-discovery uses it as a DNS label, capped at 63 bytes by
    /// RFC 1035, so a 64-hex iroh NodeId would not fit).
    pub fn start(
        handle: &Handle,
        code: &str,
        node_id: &str,
        ticket_hex: &str,
        expires_at_ms: i64,
        port: u16,
    ) -> Result<PublisherHandle, MdnsPublisherError> {
        let code_hash = compute_code_hash(code);
        let actor_id = derive_actor_id(node_id);
        let addrs = enumerate_publish_addrs();

        debug!(
            code_hash = %code_hash,
            addr_count = addrs.len(),
            port,
            "starting mDNS pairing publisher",
        );

        let discoverer = Discoverer::new(PAIR_SERVICE_NAME.to_string(), actor_id.clone())
            .with_addrs(port, addrs)
            .with_cadence(PAIR_CADENCE)
            // `Auto` binds whatever the kernel lets us bind (v4 alone, v6
            // alone, or both) and only fails when both sockets are
            // unavailable. `V4AndV6` would *require* both and surface as
            // SocketBind on any host whose IPv6 default route is missing
            // — exactly the LAN-only / Wi-Fi-off scenarios we exist to
            // serve.
            .with_ip_class(IpClass::Auto)
            // `TXT_NODE_ID` carries the derived actor id (DNS-label-safe),
            // not the full hex NodeId — that already lives inside the
            // ticket. Resolver compares its own derived id against this
            // field to skip self-announces.
            .with_txt_attributes(vec![
                (TXT_CODE_HASH.to_string(), Some(code_hash.clone())),
                (TXT_NODE_ID.to_string(), Some(actor_id.clone())),
                (TXT_TICKET.to_string(), Some(ticket_hex.to_string())),
                (
                    TXT_EXPIRES_AT_MS.to_string(),
                    Some(expires_at_ms.to_string()),
                ),
            ])?;

        let guard = discoverer.spawn(handle)?;

        info!(
            code_hash = %code_hash,
            "mDNS pairing announce live (window-scoped)",
        );

        Ok(PublisherHandle { _guard: guard })
    }
}

/// Derive a DNS-label-safe instance id from a `node_id` of arbitrary
/// length. `swarm-discovery` uses this value as a DNS label, which
/// RFC 1035 caps at 63 bytes — long enough for iroh's `fmt_short()`
/// 10-char shortcut but not for the full 64-hex public key. We hash so
/// the publisher and resolver can independently compute the same
/// transformation: as long as both sides feed the *same* `node_id`
/// string, they land on the same instance id without any negotiation.
pub(crate) fn derive_actor_id(node_id: &str) -> String {
    // ≤32 ASCII chars: pass through unchanged. This covers iroh's
    // `fmt_short()` form (10 chars) and stays human-readable when the
    // caller already shortened the id themselves.
    if node_id.len() <= 32 && node_id.is_ascii() {
        return node_id.to_string();
    }
    // Otherwise hash to a 16-hex-char prefix (8 bytes = 64 bits of
    // entropy — collision-proof at any realistic LAN swarm size).
    let digest = blake3::hash(node_id.as_bytes());
    hex::encode(&digest.as_bytes()[..8])
}

/// Enumerate local interface IPs eligible to publish. Skips loopback in
/// production; tests that need loopback construct a `Discoverer`
/// directly with a hand-picked address.
///
/// Best-effort: any `if-addrs` failure yields an empty list (logged
/// warn) rather than failing the whole publisher — a sponsor with no
/// usable interface still gets to publish via cloud channel.
fn enumerate_publish_addrs() -> Vec<IpAddr> {
    match if_addrs::get_if_addrs() {
        Ok(ifs) => ifs
            .into_iter()
            .map(|i| i.addr.ip())
            .filter(|ip| !ip.is_loopback())
            .collect(),
        Err(err) => {
            warn!(error = %err, "if-addrs enumerate failed; mDNS publisher will run without local addresses");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `start` on a loopback handle should succeed and return a guard.
    /// We don't assert anything about the actual mDNS traffic here —
    /// integration tests in `tests/` cover round-trip publish/resolve.
    #[tokio::test]
    async fn start_returns_handle_on_loopback() {
        let handle = Handle::current();
        let result = MdnsPairingPublisher::start(
            &handle,
            "ABCD-1234",
            "test-node-id",
            "deadbeef",
            1_700_000_000_000,
            12345,
        );
        // On loopback-only CI runners `swarm-discovery` may fail to bind
        // a multicast socket; either outcome is acceptable here — the
        // contract this test pins is "no panic, error shape is right."
        match result {
            Ok(_handle) => { /* publisher live; drops at end of scope */ }
            Err(MdnsPublisherError::SocketBind(_)) => { /* expected on some CI */ }
            Err(other) => panic!("unexpected error shape: {other:?}"),
        }
    }
}
