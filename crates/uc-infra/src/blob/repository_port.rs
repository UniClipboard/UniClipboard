//! Infra-internal blob repository port.
//!
//! `BlobRepositoryPort` is not a core domain capability: it only exists so that
//! the `BlobWriter` adapter can depend on the SQLite-backed blob row store via
//! an abstraction, keeping the two infra components swappable and testable.
//!
//! Consumers outside `uc-infra` should depend on `BlobWriterPort` instead.

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use uc_core::ContentHash;

use crate::blob::Blob;

#[async_trait]
pub trait BlobRepositoryPort: Send + Sync {
    async fn insert_blob(&self, blob: &Blob) -> Result<()>;
    async fn find_by_hash(&self, content_hash: &ContentHash) -> Result<Option<Blob>>;
}

#[async_trait]
impl<T: BlobRepositoryPort + ?Sized> BlobRepositoryPort for Arc<T> {
    async fn insert_blob(&self, blob: &Blob) -> Result<()> {
        (**self).insert_blob(blob).await
    }

    async fn find_by_hash(&self, content_hash: &ContentHash) -> Result<Option<Blob>> {
        (**self).find_by_hash(content_hash).await
    }
}
