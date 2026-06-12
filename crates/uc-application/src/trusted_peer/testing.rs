//! In-memory test double for `TrustedPeerRepositoryPort`.
//!
//! Kept `pub(crate)` and compiled only under `#[cfg(test)]` so it is
//! available to every unit test inside the `trusted_peer` module without
//! being exposed to downstream crates.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use uc_core::{DeviceId, TrustedPeer, TrustedPeerError, TrustedPeerRepositoryPort};

pub(crate) struct InMemoryTrustedPeerRepository {
    inner: Mutex<HashMap<String, TrustedPeer>>,
}

impl InMemoryTrustedPeerRepository {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl TrustedPeerRepositoryPort for InMemoryTrustedPeerRepository {
    async fn get(
        &self,
        peer_device_id: &DeviceId,
    ) -> Result<Option<TrustedPeer>, TrustedPeerError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .get(peer_device_id.as_str())
            .cloned())
    }

    async fn list(&self) -> Result<Vec<TrustedPeer>, TrustedPeerError> {
        Ok(self.inner.lock().unwrap().values().cloned().collect())
    }

    async fn save(&self, trusted_peer: &TrustedPeer) -> Result<(), TrustedPeerError> {
        self.inner.lock().unwrap().insert(
            trusted_peer.peer_device_id.as_str().to_string(),
            trusted_peer.clone(),
        );
        Ok(())
    }

    async fn remove(&self, peer_device_id: &DeviceId) -> Result<bool, TrustedPeerError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .remove(peer_device_id.as_str())
            .is_some())
    }
}
