//! Window-scoped mDNS browse for a pending invitation code.
//!
//! Joiner side: when the user types a code and the dial path starts,
//! we briefly listen for pairing announces on the LAN, filter by
//! `code_hash`, and return the first matching sponsor ticket. The
//! browse window closes as soon as we get a match or `timeout` elapses.
//!
//! ## Contract
//!
//! * Returns `Ok(Some(ticket))` once the first sponsor that publishes a
//!   matching `code_hash` is seen. We do **not** wait for "all" answers
//!   — first-match wins.
//! * Returns `Ok(None)` on timeout. The caller will then either fall
//!   back to the cloud channel or report `InvitationNotFound`.
//! * The TXT ticket must round-trip through hex without loss; a
//!   corrupted or missing TXT field is treated as "not a match" and we
//!   keep listening for the next announcement until timeout.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use swarm_discovery::{Discoverer, IpClass, SpawnError};
use thiserror::Error;
use tokio::runtime::Handle;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::discovery_constants::{
    compute_code_hash, PAIR_SERVICE_NAME, TXT_CODE_HASH, TXT_EXPIRES_AT_MS, TXT_TICKET,
};

/// Errors raised while starting / running a resolver. Timeout is
/// **not** an error — it returns `Ok(None)` so the caller can compose
/// `tokio::select!` cleanly with the cloud-channel path.
#[derive(Debug, Error)]
pub enum MdnsResolverError {
    /// `swarm-discovery` couldn't bind a multicast socket on any local
    /// interface. Joiner UI: "LAN resolution unavailable".
    #[error("mDNS socket bind failed: {0}")]
    SocketBind(String),

    /// Internal task plumbing failure. Should not normally happen — kept
    /// for defence in depth.
    #[error("internal resolver error: {0}")]
    Internal(String),
}

impl From<SpawnError> for MdnsResolverError {
    fn from(err: SpawnError) -> Self {
        Self::SocketBind(err.to_string())
    }
}

/// Stateless factory; each `resolve` call spins an independent browse.
pub struct MdnsPairingResolver;

impl MdnsPairingResolver {
    /// Browses for an announce matching `code` for up to `timeout`.
    ///
    /// `self_node_id` is the **joiner's** own endpoint id, used to
    /// short-circuit "I'm seeing my own announce" loops if the same
    /// process happens to publish elsewhere. Pass an empty string if
    /// the caller has no node id (rare — only diagnostic tools).
    ///
    /// Returns the matching ticket as a hex string exactly as the
    /// publisher wrote it. Decoding into an `EndpointAddr` is the
    /// caller's responsibility — that keeps this module independent of
    /// the iroh type surface.
    pub async fn resolve(
        handle: &Handle,
        self_node_id: &str,
        code: &str,
        timeout: Duration,
    ) -> Result<Option<String>, MdnsResolverError> {
        let code_hash = compute_code_hash(code);
        // Derive the same DNS-label-safe form the publisher uses, so
        // (a) our own swarm-discovery actor instance name passes RFC 1035
        //     length limits even when the caller passes a 64-hex NodeId
        //     unchanged, and
        // (b) the TXT-based "ignore my own announce" filter compares
        //     apples to apples.
        let self_actor_id = super::mdns_publisher::derive_actor_id(self_node_id);

        debug!(code_hash = %code_hash, "starting mDNS pairing resolver");

        // One-shot channel: the callback fires for every peer the
        // browse sees; on the first match we send and stop using
        // further events. A `Mutex<Option<Sender>>` lets us take the
        // sender from the callback exactly once.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1);
        let tx_holder: Arc<Mutex<Option<tokio::sync::mpsc::Sender<String>>>> =
            Arc::new(Mutex::new(Some(tx)));
        let tx_for_cb = Arc::clone(&tx_holder);

        let expected_hash = code_hash.clone();
        let self_id_for_cb = self_actor_id.clone();

        // `swarm-discovery` uses the instance name as a DNS label
        // (capped at 63 bytes). `self_actor_id` is the hashed form so
        // the actor spawn never fails on long ids; the empty case is
        // still handled defensively with a probe label.
        let actor_id = if self_actor_id.is_empty() {
            "uniclipboard-pair-probe".to_string()
        } else {
            self_actor_id.clone()
        };

        let discoverer = Discoverer::new(PAIR_SERVICE_NAME.to_string(), actor_id)
            // See [`MdnsPairingPublisher::start`] for why `Auto` instead
            // of `V4AndV6`: hosts with no IPv6 default route (Wi-Fi off,
            // VPN-only, certain virtual interfaces) would otherwise see
            // the resolver crash on `join_multicast_v6` even when v4
            // multicast works fine.
            .with_ip_class(IpClass::Auto)
            .with_callback(move |peer_id, peer| {
                // Ignore our own announce if it leaked back.
                if peer_id == self_id_for_cb {
                    return;
                }
                // Extract code_hash + ticket from TXT.
                let saw_hash = peer
                    .txt_attribute(TXT_CODE_HASH)
                    .flatten()
                    .map(str::to_string);
                let saw_ticket = peer.txt_attribute(TXT_TICKET).flatten().map(str::to_string);

                let (Some(saw_hash), Some(saw_ticket)) = (saw_hash, saw_ticket) else {
                    // Announcement is missing required fields. Either a
                    // different protocol on the same service name or a
                    // partially-built publisher mid-update — skip.
                    return;
                };

                if saw_hash != expected_hash {
                    return;
                }

                // Stale-cache guard: mDNS records linger in caches past
                // the publisher's window, so a matching `code_hash` is not
                // enough. Honour the publisher's `expires_at_ms` and treat
                // a missing, unparseable, or already-expired value as a
                // non-match — keep listening for a fresh announcement.
                let Some(expires_at_ms) = peer
                    .txt_attribute(TXT_EXPIRES_AT_MS)
                    .flatten()
                    .and_then(|s| s.parse::<i64>().ok())
                else {
                    return;
                };
                let now_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(i64::MAX);
                if expires_at_ms <= now_ms {
                    return;
                }

                // Take the sender exactly once. Subsequent matches
                // become no-ops.
                let tx_for_cb = Arc::clone(&tx_for_cb);
                tokio::spawn(async move {
                    let mut slot = tx_for_cb.lock().await;
                    if let Some(sender) = slot.take() {
                        let _ = sender.send(saw_ticket).await;
                    }
                });
            });

        // Hold the guard until we either match, timeout, or error.
        // Dropping it stops the browse and frees the multicast socket.
        let _guard = discoverer.spawn(handle)?;

        match tokio::time::timeout(timeout, rx.recv()).await {
            Ok(Some(ticket)) => {
                info!(code_hash = %code_hash, "mDNS pairing resolver matched");
                Ok(Some(ticket))
            }
            Ok(None) => {
                // Channel closed without a message — shouldn't happen
                // until guard drop. Treat as no-match.
                warn!(code_hash = %code_hash, "mDNS resolver channel closed without match");
                Ok(None)
            }
            Err(_) => {
                debug!(code_hash = %code_hash, "mDNS resolver timed out without match");
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Resolver returns `Ok(None)` when nothing answers within the
    /// short timeout. Doesn't assert on actual multicast traffic —
    /// integration tests cover the round-trip.
    #[tokio::test]
    async fn resolver_times_out_cleanly_on_empty_lan() {
        let handle = Handle::current();
        let result = MdnsPairingResolver::resolve(
            &handle,
            "joiner-self-id",
            "ABCD-1234",
            Duration::from_millis(200),
        )
        .await;

        match result {
            Ok(None) => { /* expected */ }
            Ok(Some(t)) => panic!("unexpected match on empty LAN: {t}"),
            Err(MdnsResolverError::SocketBind(_)) => { /* CI without multicast: fine */ }
            Err(other) => panic!("unexpected error shape: {other:?}"),
        }
    }
}
