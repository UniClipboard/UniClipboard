//! Address Registry — per-(peer, addr) lifecycle management.
//!
//! Tracks every known address for every peer with metadata such as
//! source, scope, TTL, failure count and cooldown. Consumers call
//! [`AddressRegistry::candidates_for`] to get a prioritised, filtered
//! list of addresses that are neither expired nor cooling down.

use chrono::{DateTime, TimeDelta, Utc};
use std::collections::HashMap;

// ── Configuration ──────────────────────────────────────────────

/// How long a discovered address stays valid without being refreshed.
const DEFAULT_TTL_SECS: i64 = 90;

/// Base cooldown after the first dial failure.
const COOLDOWN_BASE_SECS: i64 = 60;

/// Cooldown multiplied by 2 after successive failures, capped here.
const COOLDOWN_MAX_SECS: i64 = 300;

/// How often the garbage-collector should run (caller drives the timer).
pub const GC_INTERVAL_SECS: u64 = 30;

// ── Domain types ───────────────────────────────────────────────

/// How we learned about an address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressSource {
    /// Discovered via mDNS on the local network.
    Mdns,
    /// Observed from an inbound connection.
    Inbound,
    /// Manually configured or provided by a global discovery service.
    Manual,
}

/// Network scope — drives the base priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressScope {
    /// Local-area network (mDNS-discovered).  Base priority 10.
    Lan,
    /// Wide-area network (global discovery / manual).  Base priority 30.
    Wan,
    /// Relay-mediated connection.  Base priority 50.
    Relay,
}

impl AddressScope {
    pub fn base_priority(self) -> u8 {
        match self {
            Self::Lan => 10,
            Self::Wan => 30,
            Self::Relay => 50,
        }
    }
}

/// A single address record for one peer.
#[derive(Debug, Clone)]
pub struct AddressRecord {
    /// The multiaddr string (e.g. `/ip4/192.168.1.5/udp/9000/quic-v1`).
    pub addr: String,
    /// How the address was discovered.
    pub source: AddressSource,
    /// Network scope (LAN / WAN / Relay).
    pub scope: AddressScope,
    /// When the address was first observed.
    pub observed_at: DateTime<Utc>,
    /// When the TTL expires — after this the record is eligible for GC.
    pub expires_at: DateTime<Utc>,
    /// Last time a dial was attempted to this address.
    pub last_dial_at: Option<DateTime<Utc>>,
    /// Last error message if the most recent dial failed.
    pub last_error: Option<String>,
    /// Earliest time the address may be dialled again (cooldown).
    pub next_dial_at: DateTime<Utc>,
    /// Cumulative successful dials.
    pub success_count: u32,
    /// Consecutive failures (reset on success).
    pub failure_count: u32,
}

impl AddressRecord {
    /// Computed effective priority (lower = better).
    ///
    /// `QUIC-LAN 10 | TCP-LAN 15 | QUIC-WAN 30 | TCP-WAN 35 | Relay 50`
    pub fn effective_priority(&self) -> u8 {
        let base = self.scope.base_priority();
        let transport_bonus = if is_quic_addr(&self.addr) { 0 } else { 5 };
        base.saturating_add(transport_bonus)
    }

    /// Whether the record has expired (past its TTL).
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now >= self.expires_at
    }

    /// Whether the address is still in its cooldown window.
    pub fn is_cooling_down(&self, now: DateTime<Utc>) -> bool {
        now < self.next_dial_at
    }
}

// ── Registry ───────────────────────────────────────────────────

/// Composite key for the registry: `(peer_id, addr)`.
type RegistryKey = (String, String);

/// Central address registry.
///
/// Thread-safety is the caller's responsibility (wrap in `Arc<RwLock<_>>`).
#[derive(Debug, Default)]
pub struct AddressRegistry {
    entries: HashMap<RegistryKey, AddressRecord>,
}

impl AddressRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    // ── Mutations ──────────────────────────────────────────────

    /// Register or refresh an address for a peer.
    ///
    /// If the address already exists the TTL is extended and `source` /
    /// `scope` are updated; failure state is **not** reset (only an
    /// explicit [`record_success`] does that).
    pub fn register(
        &mut self,
        peer_id: &str,
        addr: &str,
        source: AddressSource,
        scope: AddressScope,
    ) {
        let now = Utc::now();
        let ttl = TimeDelta::seconds(DEFAULT_TTL_SECS);
        let key = (peer_id.to_owned(), addr.to_owned());

        if let Some(rec) = self.entries.get_mut(&key) {
            // Refresh TTL and metadata but keep failure state.
            rec.source = source;
            rec.scope = scope;
            rec.expires_at = now + ttl;
        } else {
            self.entries.insert(
                key,
                AddressRecord {
                    addr: addr.to_owned(),
                    source,
                    scope,
                    observed_at: now,
                    expires_at: now + ttl,
                    last_dial_at: None,
                    last_error: None,
                    next_dial_at: now, // immediately available
                    success_count: 0,
                    failure_count: 0,
                },
            );
        }
    }

    /// Record a successful dial — resets failure count and cooldown.
    pub fn record_success(&mut self, peer_id: &str, addr: &str) {
        let key = (peer_id.to_owned(), addr.to_owned());
        if let Some(rec) = self.entries.get_mut(&key) {
            let now = Utc::now();
            rec.last_dial_at = Some(now);
            rec.last_error = None;
            rec.next_dial_at = now; // no cooldown
            rec.success_count = rec.success_count.saturating_add(1);
            rec.failure_count = 0;
            // Extend TTL on success.
            rec.expires_at = now + TimeDelta::seconds(DEFAULT_TTL_SECS);
        }
    }

    /// Record a failed dial — increments failure count and sets cooldown.
    pub fn record_failure(&mut self, peer_id: &str, addr: &str, error: &str) {
        let key = (peer_id.to_owned(), addr.to_owned());
        if let Some(rec) = self.entries.get_mut(&key) {
            let now = Utc::now();
            rec.last_dial_at = Some(now);
            rec.last_error = Some(error.to_owned());
            rec.failure_count = rec.failure_count.saturating_add(1);

            let cooldown_secs = compute_cooldown_secs(rec.failure_count);
            rec.next_dial_at = now + TimeDelta::seconds(cooldown_secs);
        }
    }

    /// Remove all addresses for a peer (e.g. when mDNS reports it lost).
    pub fn remove_peer(&mut self, peer_id: &str) {
        self.entries.retain(|(pid, _), _| pid != peer_id);
    }

    /// Garbage-collect expired entries.  Returns the number of removed records.
    pub fn gc(&mut self) -> usize {
        let now = Utc::now();
        let before = self.entries.len();
        self.entries.retain(|_, rec| !rec.is_expired(now));
        before - self.entries.len()
    }

    // ── Queries ────────────────────────────────────────────────

    /// Return candidate addresses for a peer, sorted by effective priority
    /// (ascending).  Addresses that are expired or still cooling down are
    /// excluded.
    pub fn candidates_for(&self, peer_id: &str) -> Vec<&AddressRecord> {
        let now = Utc::now();
        let mut candidates: Vec<&AddressRecord> = self
            .entries
            .iter()
            .filter(|((pid, _), rec)| {
                pid == peer_id && !rec.is_expired(now) && !rec.is_cooling_down(now)
            })
            .map(|(_, rec)| rec)
            .collect();

        candidates.sort_by_key(|r| r.effective_priority());
        candidates
    }

    /// Return *all* addresses for a peer (including expired / cooling-down),
    /// useful for diagnostics.
    pub fn all_for(&self, peer_id: &str) -> Vec<&AddressRecord> {
        self.entries
            .iter()
            .filter(|((pid, _), _)| pid == peer_id)
            .map(|(_, rec)| rec)
            .collect()
    }

    /// Total number of records in the registry (all peers).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ── Helpers ────────────────────────────────────────────────────

/// Detect whether a multiaddr string looks like a QUIC transport.
fn is_quic_addr(addr: &str) -> bool {
    addr.contains("/quic") || addr.contains("/quic-v1")
}

/// Exponential-back-off cooldown, capped at [`COOLDOWN_MAX_SECS`].
fn compute_cooldown_secs(failure_count: u32) -> i64 {
    // 60, 120, 240, 300 (capped)
    let raw = COOLDOWN_BASE_SECS * (1i64 << failure_count.saturating_sub(1).min(4));
    raw.min(COOLDOWN_MAX_SECS)
}

// ── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_registry() -> AddressRegistry {
        AddressRegistry::new()
    }

    #[test]
    fn register_and_query_candidates() {
        let mut reg = make_registry();
        reg.register(
            "peer-1",
            "/ip4/192.168.1.5/udp/9000/quic-v1",
            AddressSource::Mdns,
            AddressScope::Lan,
        );
        reg.register(
            "peer-1",
            "/ip4/192.168.1.5/tcp/9000",
            AddressSource::Mdns,
            AddressScope::Lan,
        );

        let candidates = reg.candidates_for("peer-1");
        assert_eq!(candidates.len(), 2);
        // QUIC should come first (priority 10 vs 15).
        assert!(candidates[0].addr.contains("quic"));
    }

    #[test]
    fn expired_addresses_excluded_from_candidates() {
        let mut reg = make_registry();
        reg.register(
            "peer-1",
            "/ip4/10.0.0.1/tcp/8000",
            AddressSource::Mdns,
            AddressScope::Lan,
        );

        // Force expiry.
        let key = ("peer-1".to_owned(), "/ip4/10.0.0.1/tcp/8000".to_owned());
        reg.entries.get_mut(&key).unwrap().expires_at = Utc::now() - TimeDelta::seconds(1);

        assert!(reg.candidates_for("peer-1").is_empty());
    }

    #[test]
    fn failure_cooldown_excludes_address() {
        let mut reg = make_registry();
        reg.register(
            "peer-1",
            "/ip4/10.0.0.1/tcp/8000",
            AddressSource::Mdns,
            AddressScope::Lan,
        );
        reg.record_failure("peer-1", "/ip4/10.0.0.1/tcp/8000", "connection refused");

        // Immediately after failure the address should be cooling down.
        assert!(reg.candidates_for("peer-1").is_empty());
    }

    #[test]
    fn success_resets_cooldown() {
        let mut reg = make_registry();
        reg.register(
            "peer-1",
            "/ip4/10.0.0.1/tcp/8000",
            AddressSource::Mdns,
            AddressScope::Lan,
        );
        reg.record_failure("peer-1", "/ip4/10.0.0.1/tcp/8000", "timeout");
        assert!(reg.candidates_for("peer-1").is_empty());

        reg.record_success("peer-1", "/ip4/10.0.0.1/tcp/8000");
        assert_eq!(reg.candidates_for("peer-1").len(), 1);
    }

    #[test]
    fn gc_removes_expired_entries() {
        let mut reg = make_registry();
        reg.register(
            "peer-1",
            "/ip4/10.0.0.1/tcp/8000",
            AddressSource::Mdns,
            AddressScope::Lan,
        );

        // Force expiry.
        let key = ("peer-1".to_owned(), "/ip4/10.0.0.1/tcp/8000".to_owned());
        reg.entries.get_mut(&key).unwrap().expires_at = Utc::now() - TimeDelta::seconds(1);

        let removed = reg.gc();
        assert_eq!(removed, 1);
        assert!(reg.is_empty());
    }

    #[test]
    fn remove_peer_clears_all_addresses() {
        let mut reg = make_registry();
        reg.register(
            "peer-1",
            "/ip4/10.0.0.1/tcp/8000",
            AddressSource::Mdns,
            AddressScope::Lan,
        );
        reg.register(
            "peer-1",
            "/ip4/10.0.0.1/udp/9000/quic-v1",
            AddressSource::Mdns,
            AddressScope::Lan,
        );
        reg.register(
            "peer-2",
            "/ip4/10.0.0.2/tcp/8000",
            AddressSource::Mdns,
            AddressScope::Lan,
        );

        reg.remove_peer("peer-1");
        assert!(reg.all_for("peer-1").is_empty());
        assert_eq!(reg.all_for("peer-2").len(), 1);
    }

    #[test]
    fn priority_ordering_lan_before_wan() {
        let mut reg = make_registry();
        reg.register(
            "peer-1",
            "/ip4/10.0.0.1/tcp/8000",
            AddressSource::Mdns,
            AddressScope::Lan,
        );
        reg.register(
            "peer-1",
            "/ip4/203.0.113.1/tcp/8000",
            AddressSource::Manual,
            AddressScope::Wan,
        );

        let candidates = reg.candidates_for("peer-1");
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].scope, AddressScope::Lan);
        assert_eq!(candidates[1].scope, AddressScope::Wan);
    }

    #[test]
    fn register_refreshes_ttl_without_resetting_failure() {
        let mut reg = make_registry();
        reg.register(
            "peer-1",
            "/ip4/10.0.0.1/tcp/8000",
            AddressSource::Mdns,
            AddressScope::Lan,
        );
        reg.record_failure("peer-1", "/ip4/10.0.0.1/tcp/8000", "refused");

        let key = ("peer-1".to_owned(), "/ip4/10.0.0.1/tcp/8000".to_owned());
        let old_failure_count = reg.entries[&key].failure_count;

        // Re-register (e.g. mDNS refresh).
        reg.register(
            "peer-1",
            "/ip4/10.0.0.1/tcp/8000",
            AddressSource::Mdns,
            AddressScope::Lan,
        );

        // Failure count should be preserved.
        assert_eq!(reg.entries[&key].failure_count, old_failure_count);
    }

    #[test]
    fn cooldown_escalation() {
        assert_eq!(compute_cooldown_secs(1), 60);
        assert_eq!(compute_cooldown_secs(2), 120);
        assert_eq!(compute_cooldown_secs(3), 240);
        assert_eq!(compute_cooldown_secs(4), 300); // capped
        assert_eq!(compute_cooldown_secs(10), 300); // still capped
    }

    #[test]
    fn quic_detection() {
        assert!(is_quic_addr("/ip4/192.168.1.5/udp/9000/quic-v1"));
        assert!(is_quic_addr("/ip4/192.168.1.5/udp/9000/quic"));
        assert!(!is_quic_addr("/ip4/192.168.1.5/tcp/9000"));
    }

    #[test]
    fn effective_priority_values() {
        let mut reg = make_registry();
        reg.register(
            "p",
            "/ip4/1.2.3.4/udp/9/quic-v1",
            AddressSource::Mdns,
            AddressScope::Lan,
        );
        reg.register(
            "p",
            "/ip4/1.2.3.4/tcp/9",
            AddressSource::Mdns,
            AddressScope::Lan,
        );
        reg.register(
            "p",
            "/ip4/5.6.7.8/udp/9/quic-v1",
            AddressSource::Manual,
            AddressScope::Wan,
        );
        reg.register(
            "p",
            "/ip4/5.6.7.8/tcp/9",
            AddressSource::Manual,
            AddressScope::Wan,
        );

        let cs = reg.candidates_for("p");
        let priorities: Vec<u8> = cs.iter().map(|r| r.effective_priority()).collect();
        assert_eq!(priorities, vec![10, 15, 30, 35]);
    }
}
