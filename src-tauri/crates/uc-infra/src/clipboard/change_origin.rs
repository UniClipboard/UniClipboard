use async_trait::async_trait;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::debug;
use uc_core::ports::clipboard::ClipboardChangeOriginPort;
use uc_core::ClipboardChangeOrigin;

pub struct InMemoryClipboardChangeOrigin {
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

impl InMemoryClipboardChangeOrigin {
    pub fn new() -> Self {
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
impl ClipboardChangeOriginPort for InMemoryClipboardChangeOrigin {
    async fn set_next_origin(&self, origin: ClipboardChangeOrigin, ttl: Duration) {
        let now = Instant::now();
        let expires_at = now.checked_add(ttl).unwrap_or(now);
        let mut state = self.state.lock().await;
        Self::prune_expired(&mut state, now);
        state.next_origin = Some(OriginState { origin, expires_at });
    }

    async fn consume_origin_or_default(
        &self,
        default_origin: ClipboardChangeOrigin,
    ) -> ClipboardChangeOrigin {
        let mut state = self.state.lock().await;
        let now = Instant::now();
        Self::prune_expired(&mut state, now);
        if let Some(stored) = state.next_origin.take() {
            if now <= stored.expires_at {
                return stored.origin;
            }
        }
        default_origin
    }

    async fn has_pending_origin(&self) -> bool {
        let mut state = self.state.lock().await;
        let now = Instant::now();
        Self::prune_expired(&mut state, now);
        state.next_origin.is_some() || !state.snapshot_origins.is_empty()
    }

    async fn remember_remote_snapshot_hash(&self, snapshot_hash: String, ttl: Duration) {
        let now = Instant::now();
        let expires_at = now.checked_add(ttl).unwrap_or(now);
        let mut state = self.state.lock().await;
        Self::prune_expired(&mut state, now);
        debug!(
            snapshot_hash = %snapshot_hash,
            ttl_ms = ttl.as_millis(),
            "change_origin remember remote snapshot guard"
        );
        Self::remember_snapshot_origin(
            &mut state,
            snapshot_hash,
            ClipboardChangeOrigin::RemotePush,
            expires_at,
        );
    }

    async fn remember_local_snapshot_hash(&self, snapshot_hash: String, ttl: Duration) {
        let now = Instant::now();
        let expires_at = now.checked_add(ttl).unwrap_or(now);
        let mut state = self.state.lock().await;
        Self::prune_expired(&mut state, now);
        debug!(
            snapshot_hash = %snapshot_hash,
            ttl_ms = ttl.as_millis(),
            "change_origin remember local snapshot guard"
        );
        Self::remember_snapshot_origin(
            &mut state,
            snapshot_hash,
            ClipboardChangeOrigin::LocalRestore,
            expires_at,
        );
    }

    async fn consume_origin_for_snapshot_or_default(
        &self,
        snapshot_hash: &str,
        default_origin: ClipboardChangeOrigin,
    ) -> ClipboardChangeOrigin {
        let mut state = self.state.lock().await;
        let now = Instant::now();
        Self::prune_expired(&mut state, now);

        if let Some(idx) = state
            .snapshot_origins
            .iter()
            .position(|s| s.snapshot_hash == snapshot_hash)
        {
            if let Some(stored) = state.snapshot_origins.remove(idx) {
                debug!(
                    snapshot_hash = %snapshot_hash,
                    resolved_origin = ?stored.origin,
                    "change_origin snapshot guard matched"
                );
                return stored.origin;
            }
        }

        if let Some(stored) = state.next_origin.take() {
            if now <= stored.expires_at {
                debug!(
                    snapshot_hash = %snapshot_hash,
                    resolved_origin = ?stored.origin,
                    "change_origin next-origin fallback matched"
                );
                return stored.origin;
            }
        }

        debug!(
            snapshot_hash = %snapshot_hash,
            resolved_origin = ?default_origin,
            "change_origin no guard matched; using default origin"
        );

        default_origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn origin_is_consumed_once() {
        let port = InMemoryClipboardChangeOrigin::new();
        port.set_next_origin(ClipboardChangeOrigin::LocalRestore, Duration::from_secs(1))
            .await;
        let first = port
            .consume_origin_or_default(ClipboardChangeOrigin::LocalCapture)
            .await;
        let second = port
            .consume_origin_or_default(ClipboardChangeOrigin::LocalCapture)
            .await;
        assert_eq!(first, ClipboardChangeOrigin::LocalRestore);
        assert_eq!(second, ClipboardChangeOrigin::LocalCapture);
    }

    #[tokio::test]
    async fn matching_remote_snapshot_hash_maps_to_remote_push_once() {
        let port = InMemoryClipboardChangeOrigin::new();
        port.remember_remote_snapshot_hash("h1".to_string(), Duration::from_secs(10))
            .await;

        let first = port
            .consume_origin_for_snapshot_or_default("h1", ClipboardChangeOrigin::LocalCapture)
            .await;
        let second = port
            .consume_origin_for_snapshot_or_default("h1", ClipboardChangeOrigin::LocalCapture)
            .await;

        assert_eq!(first, ClipboardChangeOrigin::RemotePush);
        assert_eq!(second, ClipboardChangeOrigin::LocalCapture);
    }

    #[tokio::test]
    async fn matching_local_snapshot_hash_maps_to_local_restore_once() {
        let port = InMemoryClipboardChangeOrigin::new();
        port.remember_local_snapshot_hash("h1".to_string(), Duration::from_secs(10))
            .await;

        let first = port
            .consume_origin_for_snapshot_or_default("h1", ClipboardChangeOrigin::LocalCapture)
            .await;
        let second = port
            .consume_origin_for_snapshot_or_default("h1", ClipboardChangeOrigin::LocalCapture)
            .await;

        assert_eq!(first, ClipboardChangeOrigin::LocalRestore);
        assert_eq!(second, ClipboardChangeOrigin::LocalCapture);
    }

    #[tokio::test]
    async fn has_pending_origin_returns_true_when_set() {
        let port = InMemoryClipboardChangeOrigin::new();
        assert!(!port.has_pending_origin().await);

        port.set_next_origin(ClipboardChangeOrigin::LocalRestore, Duration::from_secs(1))
            .await;
        assert!(port.has_pending_origin().await);

        // has_pending_origin is non-destructive — origin should still be there
        assert!(port.has_pending_origin().await);

        // Consume it, then it should be gone
        let _ = port
            .consume_origin_or_default(ClipboardChangeOrigin::LocalCapture)
            .await;
        assert!(!port.has_pending_origin().await);
    }

    #[tokio::test]
    async fn has_pending_origin_returns_true_for_local_snapshot_guard() {
        let port = InMemoryClipboardChangeOrigin::new();
        assert!(!port.has_pending_origin().await);

        port.remember_local_snapshot_hash("h1".to_string(), Duration::from_secs(10))
            .await;
        assert!(port.has_pending_origin().await);

        let _ = port
            .consume_origin_for_snapshot_or_default("h1", ClipboardChangeOrigin::LocalCapture)
            .await;
        assert!(!port.has_pending_origin().await);
    }

    #[tokio::test]
    async fn matching_snapshot_hash_has_priority_over_next_origin() {
        let port = InMemoryClipboardChangeOrigin::new();
        port.remember_remote_snapshot_hash("h1".to_string(), Duration::from_secs(10))
            .await;
        port.set_next_origin(ClipboardChangeOrigin::LocalRestore, Duration::from_secs(10))
            .await;

        let first = port
            .consume_origin_for_snapshot_or_default("h1", ClipboardChangeOrigin::LocalCapture)
            .await;
        let second = port
            .consume_origin_for_snapshot_or_default("h1", ClipboardChangeOrigin::LocalCapture)
            .await;

        assert_eq!(first, ClipboardChangeOrigin::RemotePush);
        assert_eq!(second, ClipboardChangeOrigin::LocalRestore);
    }

    #[tokio::test]
    async fn matching_local_snapshot_hash_has_priority_over_next_origin() {
        let port = InMemoryClipboardChangeOrigin::new();
        port.remember_local_snapshot_hash("h1".to_string(), Duration::from_secs(10))
            .await;
        port.set_next_origin(ClipboardChangeOrigin::RemotePush, Duration::from_secs(10))
            .await;

        let first = port
            .consume_origin_for_snapshot_or_default("h1", ClipboardChangeOrigin::LocalCapture)
            .await;
        let second = port
            .consume_origin_or_default(ClipboardChangeOrigin::LocalCapture)
            .await;

        assert_eq!(first, ClipboardChangeOrigin::LocalRestore);
        assert_eq!(second, ClipboardChangeOrigin::RemotePush);
    }

    #[tokio::test]
    async fn unrelated_snapshot_hash_does_not_consume_local_restore_guard() {
        let port = InMemoryClipboardChangeOrigin::new();
        port.remember_local_snapshot_hash("expected".to_string(), Duration::from_secs(10))
            .await;

        let first = port
            .consume_origin_for_snapshot_or_default("other", ClipboardChangeOrigin::LocalCapture)
            .await;
        let second = port
            .consume_origin_for_snapshot_or_default("expected", ClipboardChangeOrigin::LocalCapture)
            .await;

        assert_eq!(first, ClipboardChangeOrigin::LocalCapture);
        assert_eq!(second, ClipboardChangeOrigin::LocalRestore);
    }
}
