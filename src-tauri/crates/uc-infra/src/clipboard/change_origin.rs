use async_trait::async_trait;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use uc_core::ports::clipboard::ClipboardChangeOriginPort;
use uc_core::ClipboardChangeOrigin;

pub struct InMemoryClipboardChangeOrigin {
    state: Mutex<OriginStore>,
}

struct OriginState {
    origin: ClipboardChangeOrigin,
    expires_at: Instant,
}

struct RemoteSnapshotState {
    snapshot_hash: String,
    expires_at: Instant,
}

struct OriginStore {
    next_origin: Option<OriginState>,
    remote_snapshots: VecDeque<RemoteSnapshotState>,
}

const REMOTE_SNAPSHOT_MAX: usize = 256;

impl InMemoryClipboardChangeOrigin {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(OriginStore {
                next_origin: None,
                remote_snapshots: VecDeque::new(),
            }),
        }
    }

    fn prune_expired(store: &mut OriginStore, now: Instant) {
        if let Some(stored) = &store.next_origin {
            if now > stored.expires_at {
                store.next_origin = None;
            }
        }

        while let Some(front) = store.remote_snapshots.front() {
            if now > front.expires_at {
                store.remote_snapshots.pop_front();
            } else {
                break;
            }
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

    async fn remember_remote_snapshot_hash(&self, snapshot_hash: String, ttl: Duration) {
        let now = Instant::now();
        let expires_at = now.checked_add(ttl).unwrap_or(now);
        let mut state = self.state.lock().await;
        Self::prune_expired(&mut state, now);

        if let Some(existing) = state
            .remote_snapshots
            .iter_mut()
            .find(|s| s.snapshot_hash == snapshot_hash)
        {
            existing.expires_at = expires_at;
            return;
        }

        state.remote_snapshots.push_back(RemoteSnapshotState {
            snapshot_hash,
            expires_at,
        });
        while state.remote_snapshots.len() > REMOTE_SNAPSHOT_MAX {
            state.remote_snapshots.pop_front();
        }
    }

    async fn consume_origin_for_snapshot_or_default(
        &self,
        snapshot_hash: &str,
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

        if let Some(idx) = state
            .remote_snapshots
            .iter()
            .position(|s| s.snapshot_hash == snapshot_hash)
        {
            state.remote_snapshots.remove(idx);
            return ClipboardChangeOrigin::RemotePush;
        }

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
    async fn explicit_next_origin_has_priority_over_remote_hash_match() {
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

        assert_eq!(first, ClipboardChangeOrigin::LocalRestore);
        assert_eq!(second, ClipboardChangeOrigin::RemotePush);
    }
}
