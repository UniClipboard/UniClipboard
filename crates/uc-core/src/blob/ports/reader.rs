//! Blob Reader Port.
//!
//! Read-side abstraction for blob storage. Returns plaintext bytes by `BlobId`.
//!
//! Lives in `uc-core` because the contract is purely domain-semantic
//! (`BlobId` in, `Vec<u8>` out) — no storage paths, sizes, or implementation
//! details leak across the boundary. Use cases in `uc-app` depend on this
//! trait; concrete implementations (filesystem, encrypted-decorator) live
//! in `uc-infra`.

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

use crate::BlobId;

#[async_trait]
pub trait BlobReaderPort: Send + Sync {
    async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>>;
}

#[async_trait]
impl<T: BlobReaderPort + ?Sized> BlobReaderPort for Arc<T> {
    async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>> {
        (**self).get(blob_id).await
    }
}
