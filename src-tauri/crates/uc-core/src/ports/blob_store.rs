use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

use crate::BlobId;

#[async_trait]
pub trait BlobStorePort: Send + Sync {
    /// Write bytes into blob storage, returning (storage_path, compressed_size).
    ///
    /// The `Option<i64>` is the on-disk byte count after any compression+encryption.
    /// Returns `None` if the store does not track compressed size (e.g., raw filesystem).
    async fn put(&self, blob_id: &BlobId, data: &[u8]) -> Result<(PathBuf, Option<i64>)>;

    // Read bytes from blob storage
    async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>>;
}

#[async_trait]
impl<T: BlobStorePort + ?Sized> BlobStorePort for Arc<T> {
    async fn put(&self, blob_id: &BlobId, data: &[u8]) -> Result<(PathBuf, Option<i64>)> {
        (**self).put(blob_id, data).await
    }

    async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>> {
        (**self).get(blob_id).await
    }
}
