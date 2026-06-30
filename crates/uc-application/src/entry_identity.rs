//! `EntryIdentityCoordinator` — serializes all writers that create or replace a
//! clipboard entry keyed by its content identity (`snapshot_hash`).
//!
//! Two channels (push/dispatch and active-clipboard pull) and the local capture
//! path can all try to persist the same content concurrently. Without
//! serialization, two of them can each observe "no entry for this hash yet" and
//! both create one, producing duplicate cards — or one can create while another
//! is mid-replace. This coordinator hands out a per-identity async lock so that
//! "look up the entry for this hash, then create or replace it" runs as one
//! atomic section across every writer that shares the coordinator instance.
//!
//! It is process-local in-memory state with no external dependencies; the
//! daemon is a single writer process (one per profile), so an in-process lock is
//! sufficient for correctness — no database constraint is required.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use tokio::sync::{Mutex, OwnedMutexGuard};

/// Number of lock stripes. A given `snapshot_hash` always maps to the same
/// stripe, so same-identity writers always serialize; different identities only
/// contend on the rare hash collision into the same stripe (harmless extra
/// serialization, never a correctness issue). Striping keeps memory bounded
/// (no per-hash map growth) without any cleanup bookkeeping.
const STRIPE_COUNT: usize = 64;

pub struct EntryIdentityCoordinator {
    stripes: Vec<Arc<Mutex<()>>>,
}

impl Default for EntryIdentityCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl EntryIdentityCoordinator {
    pub fn new() -> Self {
        let stripes = (0..STRIPE_COUNT)
            .map(|_| Arc::new(Mutex::new(())))
            .collect();
        Self { stripes }
    }

    /// Acquire the lock guarding `snapshot_hash`. Hold the returned guard across
    /// the entire "find entry by hash → create / replace / skip" section; drop
    /// it (let it fall out of scope) once the entry is committed. Best-effort
    /// side work such as the OS-clipboard write or register advance should run
    /// after the guard is dropped, not under it.
    pub async fn lock(&self, snapshot_hash: &str) -> OwnedMutexGuard<()> {
        let idx = self.stripe_index(snapshot_hash);
        Arc::clone(&self.stripes[idx]).lock_owned().await
    }

    fn stripe_index(&self, key: &str) -> usize {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        (hasher.finish() % self.stripes.len() as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn same_hash_maps_to_same_stripe() {
        let c = EntryIdentityCoordinator::new();
        assert_eq!(
            c.stripe_index("blake3v1:abc"),
            c.stripe_index("blake3v1:abc")
        );
    }

    #[tokio::test]
    async fn same_hash_serializes_critical_sections() {
        // Two tasks racing on the same identity must not overlap their critical
        // sections: the in-flight counter must never exceed 1.
        let coordinator = Arc::new(EntryIdentityCoordinator::new());
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let coordinator = Arc::clone(&coordinator);
            let in_flight = Arc::clone(&in_flight);
            let max_seen = Arc::clone(&max_seen);
            handles.push(tokio::spawn(async move {
                let _guard = coordinator.lock("blake3v1:same").await;
                let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                max_seen.fetch_max(now, Ordering::SeqCst);
                tokio::task::yield_now().await;
                in_flight.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        assert_eq!(
            max_seen.load(Ordering::SeqCst),
            1,
            "same-identity critical sections must be mutually exclusive"
        );
    }

    #[tokio::test]
    async fn different_hashes_can_proceed_concurrently() {
        // Distinct identities that land on distinct stripes do not block each
        // other; hold one lock and acquire another for a different hash.
        let coordinator = EntryIdentityCoordinator::new();
        let a = "blake3v1:aaaa";
        // Find a key on a different stripe than `a`.
        let mut b = String::new();
        for i in 0..1000 {
            let cand = format!("blake3v1:{i:04}");
            if coordinator.stripe_index(&cand) != coordinator.stripe_index(a) {
                b = cand;
                break;
            }
        }
        assert!(
            !b.is_empty(),
            "expected to find a key on a different stripe"
        );
        let _ga = coordinator.lock(a).await;
        // Must not deadlock / block.
        let _gb = coordinator.lock(&b).await;
    }
}
