use async_trait::async_trait;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::debug;
use uc_core::ports::clipboard::{SelfWriteAttribution, SelfWriteLedgerPort, SelfWriteMatch};
use uc_core::ClipboardChangeOrigin;

/// In-memory [`SelfWriteLedgerPort`] implementation.
///
/// Attribution is event-driven: a content-keyed record is consumed the moment
/// a change with the matching hash is observed, and the next-change fallback is
/// consumed by the very next observed change. The per-record `expires_at` is a
/// pure garbage-collection backstop — it reclaims a record whose echo never
/// arrives (identical content, or a failed write), and never overrides the
/// next-event consumption above.
pub(crate) struct InMemorySelfWriteLedger {
    state: Mutex<OriginStore>,
}

struct OriginState {
    origin: ClipboardChangeOrigin,
    expires_at: Instant,
}

struct SnapshotOriginState {
    snapshot_hash: String,
    origin: ClipboardChangeOrigin,
    expires_at: Instant,
}

struct OriginStore {
    next_origin: Option<OriginState>,
    snapshot_origins: VecDeque<SnapshotOriginState>,
}

const SNAPSHOT_ORIGIN_MAX: usize = 256;

impl InMemorySelfWriteLedger {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(OriginStore {
                next_origin: None,
                snapshot_origins: VecDeque::new(),
            }),
        }
    }

    fn prune_expired(store: &mut OriginStore, now: Instant) {
        if let Some(stored) = &store.next_origin {
            if now > stored.expires_at {
                store.next_origin = None;
            }
        }

        while let Some(front) = store.snapshot_origins.front() {
            if now > front.expires_at {
                store.snapshot_origins.pop_front();
            } else {
                break;
            }
        }
    }

    fn remember_snapshot_origin(
        store: &mut OriginStore,
        snapshot_hash: String,
        origin: ClipboardChangeOrigin,
        expires_at: Instant,
    ) {
        if let Some(existing) = store
            .snapshot_origins
            .iter_mut()
            .find(|s| s.snapshot_hash == snapshot_hash && s.origin == origin)
        {
            existing.expires_at = expires_at;
            return;
        }

        store.snapshot_origins.push_back(SnapshotOriginState {
            snapshot_hash,
            origin,
            expires_at,
        });
        while store.snapshot_origins.len() > SNAPSHOT_ORIGIN_MAX {
            store.snapshot_origins.pop_front();
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
        // Attribution → stored origin. Remote uses the anonymous variant so the
        // `from_device` field never enters the dedup comparison
        // (`s.origin == origin`), which would otherwise split one snapshot into
        // two records when the device id differs.
        let origin = match attribution {
            SelfWriteAttribution::Local => ClipboardChangeOrigin::LocalRestore,
            SelfWriteAttribution::Remote => ClipboardChangeOrigin::remote_push_anonymous(),
        };
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
                Self::remember_snapshot_origin(&mut state, snapshot_hash, origin, expires_at);
            }
            SelfWriteMatch::ByNextChange => {
                debug!(
                    ?attribution,
                    ttl_ms = ttl.as_millis(),
                    "self_write_ledger record next-change fallback"
                );
                state.next_origin = Some(OriginState { origin, expires_at });
            }
        }
    }

    async fn attribute_observed_change(&self, snapshot_hash: &str) -> ClipboardChangeOrigin {
        let mut state = self.state.lock().await;
        let now = Instant::now();
        Self::prune_expired(&mut state, now);

        if let Some(idx) = state
            .snapshot_origins
            .iter()
            .position(|s| s.snapshot_hash == snapshot_hash)
        {
            if let Some(stored) = state.snapshot_origins.remove(idx) {
                // When the content guard matches, clear the next-change fallback.
                // The echo was already handled by the content match, so the
                // fallback is no longer needed and would misclassify the next
                // real user action.
                state.next_origin = None;
                debug!(
                    snapshot_hash = %snapshot_hash,
                    resolved_origin = ?stored.origin,
                    "self_write_ledger content guard matched"
                );
                return stored.origin;
            }
        }

        if let Some(stored) = state.next_origin.take() {
            if now <= stored.expires_at {
                debug!(
                    snapshot_hash = %snapshot_hash,
                    resolved_origin = ?stored.origin,
                    "self_write_ledger next-change fallback matched"
                );
                return stored.origin;
            }
        }

        debug!(
            snapshot_hash = %snapshot_hash,
            "self_write_ledger no guard matched; treating as local capture"
        );

        ClipboardChangeOrigin::LocalCapture
    }
}
