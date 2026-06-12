//! Peer address repository port (Slice 2 Phase 1).
//!
//! Persists the last-observed transport address for each paired device, so
//! F1 `ensure_reachable_all` can dial every member after `start_network`
//! without depending on rendezvous resolution or mDNS.
//!
//! Domain-neutral design: the stored bytes are opaque to core. Infra
//! adapters (e.g. iroh) encode whatever native address format they need
//! (`iroh::NodeAddr` postcard-encoded) into `addr_blob`; core / application
//! never inspect the bytes.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::ids::DeviceId;

/// One device's last-observed transport address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerAddressRecord {
    pub device_id: DeviceId,
    /// Opaque adapter-defined encoding. Core does not parse this.
    pub addr_blob: Vec<u8>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum PeerAddressError {
    #[error("internal: {0}")]
    Internal(String),
}

#[async_trait]
pub trait PeerAddressRepositoryPort: Send + Sync {
    async fn get(&self, device: &DeviceId) -> Result<Option<PeerAddressRecord>, PeerAddressError>;

    /// Upsert semantics: last-write-wins on `(device_id)`.
    async fn upsert(&self, record: &PeerAddressRecord) -> Result<(), PeerAddressError>;

    async fn list(&self) -> Result<Vec<PeerAddressRecord>, PeerAddressError>;

    /// Idempotent: removing a non-existent record succeeds.
    async fn remove(&self, device: &DeviceId) -> Result<(), PeerAddressError>;
}
