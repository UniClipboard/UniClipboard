//! Control-WS lease registry (ADR-008 P5-L L3).
//!
//! Every authenticated control WebSocket connection holds exactly ONE lease for
//! the whole lifetime of that connection. The lease is acquired near the top of
//! the connection handler and released via [`Drop`] when the handler's future
//! ends — which happens on EVERY exit path:
//!
//! - clean `Close` frame from the client,
//! - abrupt TCP reset / `kill -9` of the *client* (the receive loop sees
//!   `Some(Err(_))` or `None` and falls through to cleanup),
//! - heartbeat-stale eviction.
//!
//! A dangling lease therefore can never outlive its connection: either the
//! handler future ends (any path above) and `Drop` decrements the count, or the
//! *daemon process itself* dies — a handler panic aborts the process under the
//! release profile's `panic = "abort"`, and an OS kill of the daemon is the same
//! — in which case the in-process counter dies with it, equivalent to releasing
//! every lease. (Under dev's unwinding panics `Drop` still runs.)
//!
//! HTTP request/response handlers (`/health`, `/status`, …) NEVER take a lease —
//! only a live, long-lived WS connection does. The lease COUNT
//! ([`ControlLeaseRegistry::active_leases`]) is the daemon-side liveness signal
//! that a LATER sub-step (P5-L L4) will consult to decide when an `Oneshot`
//! daemon may self-terminate.
//!
//! L3 adds NO consumer: the count is observed/logged only. This is
//! production-behaviour-neutral — nothing acts on the count yet, and the lease
//! is tracked in ALL run modes (the registry op is a cheap atomic). L4 gates the
//! *consumption* of the count on the `Oneshot` residency, not the tracking.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// Connection-bound lease registry for authenticated control WebSocket
/// connections (ADR-008 P5-L L3).
///
/// Clone is cheap and shares the same counters: the registry is `Arc`-backed, so
/// every clone — including every `DaemonApiState` clone handed to the router and
/// each handler — observes the SAME active-lease count and the SAME monotonic
/// lease-id source. See the [module docs](self) for the lifecycle and L4 intent.
#[derive(Clone)]
pub struct ControlLeaseRegistry {
    /// Number of currently-held leases (== number of live control-WS
    /// connections). This is the L4 self-terminate signal; observed/logged only
    /// until then.
    active: Arc<AtomicUsize>,
    /// Monotonic source for lease ids, used purely for log correlation between
    /// the "acquired" and "released" events of a single connection.
    next_id: Arc<AtomicU64>,
}

impl ControlLeaseRegistry {
    /// Create an empty registry (zero active leases, lease ids starting at 0).
    pub fn new() -> Self {
        Self {
            active: Arc::new(AtomicUsize::new(0)),
            next_id: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Acquire a connection-bound lease, returning an RAII [`ControlLease`] guard.
    ///
    /// The active count is incremented for as long as the returned guard lives;
    /// it is decremented automatically when the guard is dropped (see
    /// [`ControlLease`]). Each acquire mints a fresh monotonic lease id used only
    /// to correlate the acquire/release log lines for one connection.
    pub fn acquire(&self) -> ControlLease {
        let lease_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        // fetch_add returns the PREVIOUS value, so the new active count is +1.
        let active_leases = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        tracing::debug!(lease_id, active_leases, "control-WS lease acquired");
        ControlLease {
            lease_id,
            active: Arc::clone(&self.active),
        }
    }

    /// Number of currently-held control-WS leases.
    ///
    /// This is the daemon-side liveness signal that ADR-008 P5-L **L4** will
    /// consult to decide when an `Oneshot` daemon may self-terminate (zero leases
    /// after the startup grace window). In L3 it is observed/logged only — NO
    /// consumer reads it to drive behaviour yet.
    pub fn active_leases(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }
}

impl Default for ControlLeaseRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard for a single control-WS lease (ADR-008 P5-L L3).
///
/// Bound near the top of the WS connection handler and held for the connection's
/// whole lifetime. Dropping it — clean close, abrupt TCP reset / `kill -9`, or
/// heartbeat-stale eviction — decrements the registry's active count exactly
/// once. (A handler panic aborts the process under the release `panic = "abort"`
/// profile, so the in-process count dies with it rather than being decremented.)
pub struct ControlLease {
    /// Lease id minted at acquire time; used only for acquire/release log
    /// correlation.
    lease_id: u64,
    /// Shared handle to the registry's active-lease counter.
    active: Arc<AtomicUsize>,
}

impl Drop for ControlLease {
    fn drop(&mut self) {
        // fetch_sub returns the PREVIOUS value, so the remaining count is -1.
        let remaining = self.active.fetch_sub(1, Ordering::SeqCst) - 1;
        tracing::debug!(
            lease_id = self.lease_id,
            active_leases = remaining,
            "control-WS lease released"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_increments_and_drop_releases() {
        let registry = ControlLeaseRegistry::new();
        assert_eq!(registry.active_leases(), 0);

        let lease = registry.acquire();
        assert_eq!(registry.active_leases(), 1);

        drop(lease);
        assert_eq!(registry.active_leases(), 0);
    }

    #[test]
    fn multiple_guards_counted_and_released_independently() {
        let registry = ControlLeaseRegistry::new();

        let a = registry.acquire();
        let b = registry.acquire();
        let c = registry.acquire();
        assert_eq!(registry.active_leases(), 3);

        drop(b);
        assert_eq!(registry.active_leases(), 2);

        drop(a);
        assert_eq!(registry.active_leases(), 1);

        drop(c);
        assert_eq!(registry.active_leases(), 0);
    }

    #[test]
    fn count_is_shared_across_clones() {
        // Proves every `DaemonApiState` clone sees the same Arc-backed counter:
        // a lease acquired through one clone is visible through the other.
        let registry = ControlLeaseRegistry::new();
        let cloned = registry.clone();

        let lease = registry.acquire();
        assert_eq!(cloned.active_leases(), 1);

        // A lease acquired via the clone is likewise visible from the original.
        let lease2 = cloned.acquire();
        assert_eq!(registry.active_leases(), 2);

        drop(lease);
        assert_eq!(cloned.active_leases(), 1);

        drop(lease2);
        assert_eq!(registry.active_leases(), 0);
    }

    #[test]
    fn lease_ids_are_monotonic_and_distinct() {
        let registry = ControlLeaseRegistry::new();

        let a = registry.acquire();
        let b = registry.acquire();
        let c = registry.acquire();

        assert_eq!(a.lease_id, 0);
        assert_eq!(b.lease_id, 1);
        assert_eq!(c.lease_id, 2);
        assert!(a.lease_id < b.lease_id && b.lease_id < c.lease_id);

        // Releasing and re-acquiring does NOT reuse ids — the id source is
        // monotonic, independent of the active count.
        drop(a);
        drop(b);
        drop(c);
        let d = registry.acquire();
        assert_eq!(d.lease_id, 3);
    }
}
