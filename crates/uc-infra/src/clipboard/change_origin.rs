use async_trait::async_trait;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::debug;
use uc_core::ports::clipboard::{SelfWriteAttribution, SelfWriteLedgerPort, SelfWriteMatch};
use uc_core::ClipboardChangeOrigin;

/// In-memory [`SelfWriteLedgerPort`] implementation.
///
/// Attribution is event-driven: a content-keyed record is consumed the moment a
/// change with the matching hash is observed, and a next-change fallback is
/// consumed by the very next observed change. The per-record `expires_at` is a
/// pure garbage-collection backstop — it reclaims a record whose echo never
/// arrives (identical content, or a failed write), and never overrides the
/// next-event consumption above.
///
/// Records store only the local-vs-remote [`SelfWriteAttribution`], not a
/// synthesized [`ClipboardChangeOrigin`]: the attribution is the real datum the
/// ledger tracks, it carries no peer identity that could split one snapshot into
/// two records under an equality check, and the mapping to a domain origin is
/// applied once, at read time.
pub(crate) struct InMemorySelfWriteLedger {
    state: Mutex<OriginStore>,
}

/// A content-keyed self-write: matched when a later change carries `snapshot_hash`.
struct ContentRecord {
    snapshot_hash: String,
    attribution: SelfWriteAttribution,
    expires_at: Instant,
}

/// A next-change fallback: matched by the very next observed change whose hash
/// did not resolve against a content record (covers bytes re-encoded between
/// write and echo, where no content key can be relied on).
struct NextChangeRecord {
    /// Content guard key of the write this fallback backs. Pairs the fallback to
    /// its write so a content match retires exactly the redundant one (not a
    /// concurrent write's), and keys arm-time de-duplication. Never consulted
    /// when the fallback is consumed by an unmatched change — consumption stays
    /// content-agnostic to catch re-encoded echoes under an unknown hash.
    guard_key: String,
    attribution: SelfWriteAttribution,
    expires_at: Instant,
}

struct OriginStore {
    /// FIFO of pending next-change fallbacks. A queue (not a single slot) so two
    /// concurrent writes do not clobber each other's fallback — same-attribution
    /// fallbacks are interchangeable, so consuming the front is always correct.
    next_changes: VecDeque<NextChangeRecord>,
    content_records: VecDeque<ContentRecord>,
}

/// Cap on retained content records; oldest evicted past this.
const CONTENT_RECORD_MAX: usize = 256;
/// Cap on pending next-change fallbacks; oldest evicted past this. Far above the
/// realistic number of in-flight programmatic writes — a runaway backstop only.
const NEXT_CHANGE_MAX: usize = 64;

fn attribution_to_origin(attribution: SelfWriteAttribution) -> ClipboardChangeOrigin {
    match attribution {
        SelfWriteAttribution::Local => ClipboardChangeOrigin::LocalRestore,
        // Remote resolves to the anonymous variant: the ledger tracks no peer
        // identity, so `from_device` is always `None`.
        SelfWriteAttribution::Remote => ClipboardChangeOrigin::remote_push_anonymous(),
    }
}

impl InMemorySelfWriteLedger {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(OriginStore {
                next_changes: VecDeque::new(),
                content_records: VecDeque::new(),
            }),
        }
    }

    /// Drop every record whose backstop has elapsed. `retain` (not
    /// pop-front-while-expired) is required because records carry different TTLs
    /// (local vs remote budgets), so insertion order is not expiry order.
    fn prune_expired(store: &mut OriginStore, now: Instant) {
        store.next_changes.retain(|r| now <= r.expires_at);
        store.content_records.retain(|r| now <= r.expires_at);
    }

    fn remember_content_record(
        store: &mut OriginStore,
        snapshot_hash: String,
        attribution: SelfWriteAttribution,
        expires_at: Instant,
    ) {
        if let Some(existing) = store
            .content_records
            .iter_mut()
            .find(|r| r.snapshot_hash == snapshot_hash && r.attribution == attribution)
        {
            existing.expires_at = expires_at;
            return;
        }

        store.content_records.push_back(ContentRecord {
            snapshot_hash,
            attribution,
            expires_at,
        });
        while store.content_records.len() > CONTENT_RECORD_MAX {
            store.content_records.pop_front();
        }
    }
}

#[async_trait]
impl SelfWriteLedgerPort for InMemorySelfWriteLedger {
    async fn record_self_write(
        &self,
        matching: SelfWriteMatch,
        attribution: SelfWriteAttribution,
        ttl: Duration,
    ) {
        let now = Instant::now();
        let expires_at = now.checked_add(ttl).unwrap_or(now);
        let mut state = self.state.lock().await;
        Self::prune_expired(&mut state, now);
        match matching {
            SelfWriteMatch::ByContent(snapshot_hash) => {
                debug!(
                    snapshot_hash = %snapshot_hash,
                    ?attribution,
                    ttl_ms = ttl.as_millis(),
                    "self_write_ledger record content guard"
                );
                Self::remember_content_record(&mut state, snapshot_hash, attribution, expires_at);
            }
            SelfWriteMatch::ByNextChange(guard_key) => {
                debug!(
                    ?attribution,
                    ttl_ms = ttl.as_millis(),
                    "self_write_ledger record next-change fallback"
                );
                // De-duplicate by (guard_key, attribution). One write may arm
                // its fallback more than once — the same snapshot written twice
                // coalesces into a single OS echo, so a duplicate fallback would
                // linger past that lone echo and swallow the next genuine change.
                // Same key+attribution is the same write, so refreshing the
                // existing record's backstop is correct; a different key (a
                // concurrent write) keeps its own independent fallback.
                if let Some(idx) = state
                    .next_changes
                    .iter()
                    .position(|r| r.guard_key == guard_key && r.attribution == attribution)
                {
                    state.next_changes[idx].expires_at = expires_at;
                } else {
                    state.next_changes.push_back(NextChangeRecord {
                        guard_key,
                        attribution,
                        expires_at,
                    });
                    while state.next_changes.len() > NEXT_CHANGE_MAX {
                        state.next_changes.pop_front();
                    }
                }
            }
        }
    }

    async fn attribute_observed_change(&self, snapshot_hash: &str) -> ClipboardChangeOrigin {
        let mut state = self.state.lock().await;
        let now = Instant::now();
        Self::prune_expired(&mut state, now);

        if let Some(idx) = state
            .content_records
            .iter()
            .position(|r| r.snapshot_hash == snapshot_hash)
        {
            if let Some(stored) = state.content_records.remove(idx) {
                // The content match resolved this write's echo, so the fallback
                // paired to the SAME write is now redundant and would otherwise
                // misclassify the next genuine user action. Match by guard_key
                // (the paired write's content key), not merely attribution, so a
                // concurrent write's independent fallback — which may still need
                // to absorb a re-encoded echo — survives.
                if let Some(fidx) = state.next_changes.iter().position(|r| {
                    r.guard_key == snapshot_hash && r.attribution == stored.attribution
                }) {
                    state.next_changes.remove(fidx);
                }
                debug!(
                    snapshot_hash = %snapshot_hash,
                    ?stored.attribution,
                    "self_write_ledger content guard matched"
                );
                return attribution_to_origin(stored.attribution);
            }
        }

        // Pruning above already dropped expired fallbacks, so the front (if any)
        // is live.
        if let Some(stored) = state.next_changes.pop_front() {
            debug!(
                snapshot_hash = %snapshot_hash,
                ?stored.attribution,
                "self_write_ledger next-change fallback matched"
            );
            return attribution_to_origin(stored.attribution);
        }

        debug!(
            snapshot_hash = %snapshot_hash,
            "self_write_ledger no guard matched; treating as local capture"
        );

        ClipboardChangeOrigin::LocalCapture
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: Duration = Duration::from_secs(60);

    #[tokio::test]
    async fn content_match_resolves_and_consumes() {
        let ledger = InMemorySelfWriteLedger::new();
        ledger
            .record_self_write(
                SelfWriteMatch::ByContent("h1".into()),
                SelfWriteAttribution::Remote,
                LONG,
            )
            .await;

        assert_eq!(
            ledger.attribute_observed_change("h1").await,
            ClipboardChangeOrigin::remote_push_anonymous()
        );
        // Consumed: a second observation of the same hash is a fresh capture.
        assert_eq!(
            ledger.attribute_observed_change("h1").await,
            ClipboardChangeOrigin::LocalCapture
        );
    }

    #[tokio::test]
    async fn local_attribution_maps_to_local_restore() {
        let ledger = InMemorySelfWriteLedger::new();
        ledger
            .record_self_write(
                SelfWriteMatch::ByContent("h1".into()),
                SelfWriteAttribution::Local,
                LONG,
            )
            .await;
        assert_eq!(
            ledger.attribute_observed_change("h1").await,
            ClipboardChangeOrigin::LocalRestore
        );
    }

    #[tokio::test]
    async fn no_record_resolves_to_local_capture() {
        let ledger = InMemorySelfWriteLedger::new();
        assert_eq!(
            ledger.attribute_observed_change("whatever").await,
            ClipboardChangeOrigin::LocalCapture
        );
    }

    #[tokio::test]
    async fn next_change_fallback_matches_any_hash() {
        let ledger = InMemorySelfWriteLedger::new();
        ledger
            .record_self_write(
                SelfWriteMatch::ByNextChange("h1".into()),
                SelfWriteAttribution::Remote,
                LONG,
            )
            .await;
        // A re-encoded echo arrives under a hash the content record never saw.
        assert_eq!(
            ledger.attribute_observed_change("re-encoded-hash").await,
            ClipboardChangeOrigin::remote_push_anonymous()
        );
        // Consumed once.
        assert_eq!(
            ledger.attribute_observed_change("anything").await,
            ClipboardChangeOrigin::LocalCapture
        );
    }

    #[tokio::test]
    async fn content_match_clears_one_paired_fallback() {
        let ledger = InMemorySelfWriteLedger::new();
        // One write arms both a content record and its next-change fallback.
        ledger
            .record_self_write(
                SelfWriteMatch::ByContent("h1".into()),
                SelfWriteAttribution::Local,
                LONG,
            )
            .await;
        ledger
            .record_self_write(
                SelfWriteMatch::ByNextChange("h1".into()),
                SelfWriteAttribution::Local,
                LONG,
            )
            .await;

        // Content matched (no re-encode), so the paired fallback must be dropped
        // and NOT linger to swallow the user's next genuine copy.
        assert_eq!(
            ledger.attribute_observed_change("h1").await,
            ClipboardChangeOrigin::LocalRestore
        );
        assert_eq!(
            ledger.attribute_observed_change("a-real-user-copy").await,
            ClipboardChangeOrigin::LocalCapture
        );
    }

    /// Regression: two concurrent remote writes must not clobber each other's
    /// fallback. Under the old single-slot design, write2 overwrote write1's
    /// override and the content match then cleared it, so write2's re-encoded
    /// echo fell through to `LocalCapture` and bounced back to the sender.
    #[tokio::test]
    async fn concurrent_remote_writes_keep_independent_fallbacks() {
        let ledger = InMemorySelfWriteLedger::new();
        // write1
        ledger
            .record_self_write(
                SelfWriteMatch::ByContent("h1".into()),
                SelfWriteAttribution::Remote,
                LONG,
            )
            .await;
        ledger
            .record_self_write(
                SelfWriteMatch::ByNextChange("h1".into()),
                SelfWriteAttribution::Remote,
                LONG,
            )
            .await;
        // write2 (interleaved before either echo lands)
        ledger
            .record_self_write(
                SelfWriteMatch::ByContent("h2".into()),
                SelfWriteAttribution::Remote,
                LONG,
            )
            .await;
        ledger
            .record_self_write(
                SelfWriteMatch::ByNextChange("h2".into()),
                SelfWriteAttribution::Remote,
                LONG,
            )
            .await;

        // write1 echoes back unchanged (content match) → drops one fallback.
        assert_eq!(
            ledger.attribute_observed_change("h1").await,
            ClipboardChangeOrigin::remote_push_anonymous()
        );
        // write2 echoes back RE-ENCODED (hash differs) → must still resolve to
        // remote via the surviving fallback, not LocalCapture.
        assert_eq!(
            ledger.attribute_observed_change("h2-reencoded").await,
            ClipboardChangeOrigin::remote_push_anonymous()
        );
    }

    /// Regression: an inbound sync that writes the SAME snapshot twice (e.g. an
    /// apply followed by an active-state rebroadcast) arms two content records
    /// and two fallbacks under one attribution. The OS coalesces the two
    /// identical writes into a single observed echo, so only one content match
    /// fires. Under the old design the second fallback leaked and the next
    /// genuine user copy — arbitrary unrelated content — was swallowed as a
    /// RemotePush echo, producing zero capture entries.
    #[tokio::test]
    async fn double_write_same_content_does_not_leak_fallback() {
        let ledger = InMemorySelfWriteLedger::new();
        // First write of the snapshot: content guard + paired fallback.
        ledger
            .record_self_write(
                SelfWriteMatch::ByContent("h".into()),
                SelfWriteAttribution::Remote,
                LONG,
            )
            .await;
        ledger
            .record_self_write(
                SelfWriteMatch::ByNextChange("h".into()),
                SelfWriteAttribution::Remote,
                LONG,
            )
            .await;
        // Second write of the IDENTICAL snapshot (same hash, same attribution).
        ledger
            .record_self_write(
                SelfWriteMatch::ByContent("h".into()),
                SelfWriteAttribution::Remote,
                LONG,
            )
            .await;
        ledger
            .record_self_write(
                SelfWriteMatch::ByNextChange("h".into()),
                SelfWriteAttribution::Remote,
                LONG,
            )
            .await;

        // The OS merged both writes into one observed echo.
        assert_eq!(
            ledger.attribute_observed_change("h").await,
            ClipboardChangeOrigin::remote_push_anonymous()
        );
        // A genuine, unrelated user copy must resolve to a fresh capture, NOT be
        // eaten by a leaked fallback from the duplicated write.
        assert_eq!(
            ledger.attribute_observed_change("a-real-user-copy").await,
            ClipboardChangeOrigin::LocalCapture
        );
    }

    #[tokio::test]
    async fn expired_records_are_pruned_regardless_of_ttl_order() {
        let ledger = InMemorySelfWriteLedger::new();
        // Short-TTL record inserted FIRST, long-TTL record SECOND: insertion
        // order is not expiry order, so pop-front-while-expired would be wrong.
        ledger
            .record_self_write(
                SelfWriteMatch::ByContent("short".into()),
                SelfWriteAttribution::Local,
                Duration::from_millis(1),
            )
            .await;
        ledger
            .record_self_write(
                SelfWriteMatch::ByContent("long".into()),
                SelfWriteAttribution::Remote,
                LONG,
            )
            .await;

        tokio::time::sleep(Duration::from_millis(20)).await;

        // The short record expired; the long one behind it must survive.
        assert_eq!(
            ledger.attribute_observed_change("short").await,
            ClipboardChangeOrigin::LocalCapture
        );
        assert_eq!(
            ledger.attribute_observed_change("long").await,
            ClipboardChangeOrigin::remote_push_anonymous()
        );
    }
}
