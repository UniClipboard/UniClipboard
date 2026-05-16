//! Blob Writer Port.
//!
//! Write-side abstraction for blob storage with content-hash deduplication.
//! Callers either pass plaintext bytes already in memory, or pass a path to a
//! local file whose contents should be streamed into the store without loading
//! the full payload into memory. Encryption (if any) is handled by the
//! infrastructure layer via decorator.
//!
//! Lives in `uc-core` because the contract speaks only in domain types
//! (`ContentHash` in, `BlobId` out; or `Path` in, `BlobId` out). Concrete
//! implementations live in `uc-infra`.

use std::path::Path;

use crate::{BlobId, ContentHash};

#[async_trait::async_trait]
pub trait BlobWriterPort: Send + Sync {
    /// Write plaintext bytes if the content hash isn't already stored.
    ///
    /// # Atomic semantics
    /// - If `content_id` already exists → return the existing `BlobId`
    /// - If `content_id` doesn't exist → write and return the new `BlobId`
    ///
    /// # Idempotence guarantee
    /// - Multiple concurrent calls with same `content_id` return the same `BlobId`
    /// - Data is written only once per `content_id`
    async fn write_if_absent(
        &self,
        content_id: &ContentHash,
        plaintext_bytes: &[u8],
    ) -> anyhow::Result<BlobId>;

    /// Ingest a file at `source_path` into the blob store, deduplicating by content hash.
    ///
    /// The implementation streams the file to compute its `ContentHash` without
    /// loading the full payload into memory, so the call is suitable for
    /// arbitrarily large files. Once the hash is known the behaviour matches
    /// `write_if_absent`:
    ///
    /// # Atomic semantics
    /// - If the computed `ContentHash` already maps to a blob → return that `BlobId`
    /// - Otherwise materialise the file into storage and return the new `BlobId`
    ///
    /// # Idempotence guarantee
    /// - Multiple concurrent calls with the same file content return the same `BlobId`
    /// - Data is written only once per content hash, even across distinct source paths
    ///
    /// The source file is left untouched; the implementation must not mutate
    /// the caller's file in place.
    async fn write_path_if_absent(&self, source_path: &Path) -> anyhow::Result<BlobId>;
}
