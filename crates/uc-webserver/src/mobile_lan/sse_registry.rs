//! Per-listener registry of live SSE connections, keyed by the stable
//! `MobileDeviceId` (not `username`, which is user-editable and would let a
//! rename split one device's connections across two buckets, defeating the
//! cap).
//!
//! Enforces the per-device connection cap (design allows 1–2; this project
//! uses 2 so a network-switch reconnect can briefly overlap with the
//! connection it is replacing instead of the new one evicting itself).
//! Reaching the cap evicts the oldest connection for that device by
//! cancelling its registered [`CancellationToken`] — this is also how a
//! credential rotation naturally kicks out a stale connection still using
//! the old password: the next successful connect with the new credential
//! registers under the same `device_id` and evicts the old one.

use std::collections::{HashMap, VecDeque};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use uc_core::mobile_sync::MobileDeviceId;

/// Registered connections per device must never exceed this. See module docs
/// for why 2 rather than 1.
const MAX_CONNECTIONS_PER_DEVICE: usize = 2;

#[derive(Default)]
pub(crate) struct SseConnectionRegistry {
    inner: Mutex<HashMap<MobileDeviceId, VecDeque<(Uuid, CancellationToken)>>>,
}

impl SseConnectionRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Register a new connection's cancel token under `device_id`, evicting
    /// the oldest registered connection for that device first if already at
    /// the cap. Returns the `Uuid` handle the caller must pass back to
    /// [`unregister`](Self::unregister) when the connection ends.
    pub(crate) async fn register(
        &self,
        device_id: MobileDeviceId,
        token: CancellationToken,
    ) -> Uuid {
        let mut map = self.inner.lock().await;
        let bucket = map.entry(device_id).or_default();
        while bucket.len() >= MAX_CONNECTIONS_PER_DEVICE {
            if let Some((_, oldest)) = bucket.pop_front() {
                oldest.cancel();
            }
        }
        let conn_id = Uuid::new_v4();
        bucket.push_back((conn_id, token));
        conn_id
    }

    /// Remove a connection's entry once its stream has ended (normally or by
    /// early client disconnect — see [`super::routes::sse`]'s RAII guard).
    /// Idempotent: unregistering an already-absent handle is a no-op, since
    /// eviction in [`register`](Self::register) may have already removed it.
    pub(crate) async fn unregister(&self, device_id: &MobileDeviceId, conn_id: Uuid) {
        let mut map = self.inner.lock().await;
        if let Some(bucket) = map.get_mut(device_id) {
            bucket.retain(|(id, _)| *id != conn_id);
            if bucket.is_empty() {
                map.remove(device_id);
            }
        }
    }

    #[cfg(test)]
    pub(crate) async fn connection_count(&self, device_id: &MobileDeviceId) -> usize {
        self.inner
            .lock()
            .await
            .get(device_id)
            .map(VecDeque::len)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str) -> MobileDeviceId {
        MobileDeviceId::new(id)
    }

    #[tokio::test]
    async fn register_under_cap_does_not_evict() {
        let registry = SseConnectionRegistry::new();
        let a = CancellationToken::new();
        let b = CancellationToken::new();

        registry.register(dev("d1"), a.clone()).await;
        registry.register(dev("d1"), b.clone()).await;

        assert_eq!(registry.connection_count(&dev("d1")).await, 2);
        assert!(!a.is_cancelled());
        assert!(!b.is_cancelled());
    }

    #[tokio::test]
    async fn third_connection_evicts_the_oldest() {
        let registry = SseConnectionRegistry::new();
        let a = CancellationToken::new();
        let b = CancellationToken::new();
        let c = CancellationToken::new();

        registry.register(dev("d1"), a.clone()).await;
        registry.register(dev("d1"), b.clone()).await;
        registry.register(dev("d1"), c.clone()).await;

        assert!(a.is_cancelled(), "oldest connection must be evicted");
        assert!(!b.is_cancelled());
        assert!(!c.is_cancelled());
        assert_eq!(registry.connection_count(&dev("d1")).await, 2);
    }

    #[tokio::test]
    async fn different_devices_do_not_share_a_cap() {
        let registry = SseConnectionRegistry::new();
        let a1 = CancellationToken::new();
        let a2 = CancellationToken::new();
        let b1 = CancellationToken::new();

        registry.register(dev("d1"), a1.clone()).await;
        registry.register(dev("d1"), a2.clone()).await;
        registry.register(dev("d2"), b1.clone()).await;

        assert!(!a1.is_cancelled());
        assert!(!a2.is_cancelled());
        assert!(!b1.is_cancelled());
        assert_eq!(registry.connection_count(&dev("d1")).await, 2);
        assert_eq!(registry.connection_count(&dev("d2")).await, 1);
    }

    #[tokio::test]
    async fn unregister_removes_the_entry_and_empties_the_bucket() {
        let registry = SseConnectionRegistry::new();
        let a = CancellationToken::new();
        let id = registry.register(dev("d1"), a.clone()).await;

        registry.unregister(&dev("d1"), id).await;

        assert_eq!(registry.connection_count(&dev("d1")).await, 0);
        assert!(!a.is_cancelled(), "unregister must not itself cancel");
    }

    #[tokio::test]
    async fn unregister_is_idempotent_for_an_already_evicted_handle() {
        let registry = SseConnectionRegistry::new();
        let a = CancellationToken::new();
        let b = CancellationToken::new();
        let c = CancellationToken::new();

        let id_a = registry.register(dev("d1"), a.clone()).await;
        registry.register(dev("d1"), b.clone()).await;
        registry.register(dev("d1"), c.clone()).await; // evicts `a`

        // `a`'s connection already vanished from the bucket via eviction;
        // its RAII guard still fires `unregister` on drop — must not panic
        // or remove an unrelated entry.
        registry.unregister(&dev("d1"), id_a).await;

        assert_eq!(registry.connection_count(&dev("d1")).await, 2);
    }
}
