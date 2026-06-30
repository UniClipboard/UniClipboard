//! Blob content-ingest port.
//!
//! Write-side abstraction that materializes a local file into blob storage
//! and surfaces its content identity (the device-independent content hash)
//! in the same streaming pass.
//!
//! Distinct from [`super::writer::BlobWriterPort`], whose `write_path_if_absent`
//! returns only a storage handle: a caller that must derive a snapshot's
//! cross-device identity from file content needs the `ContentHash` itself, not
//! just the opaque `BlobId`. Concrete implementations live in `uc-infra`.

use std::path::Path;

use crate::{BlobId, ContentHash};

/// Outcome of ingesting a local file into blob storage.
///
/// Carries the storage handle alongside the content identity computed while
/// streaming the file's bytes, so the caller can both reference the stored
/// blob and use the content hash as a device-independent identity.
#[derive(Debug, Clone)]
pub struct IngestedBlob {
    /// Storage handle for the materialized blob.
    pub blob_id: BlobId,
    /// blake3 content hash of the file's bytes. Stable across devices for
    /// identical content; the same value an independent ingest of the same
    /// bytes would produce.
    pub content_hash: ContentHash,
    /// Size of the ingested file in bytes.
    pub size_bytes: u64,
}

#[async_trait::async_trait]
pub trait BlobContentIngestPort: Send + Sync {
    /// Ingest the file at `source_path`, deduplicating by content hash, and
    /// return its storage handle together with the content hash and byte size.
    ///
    /// The implementation streams the file to compute its `ContentHash`
    /// without loading the full payload into memory, so the call is suitable
    /// for arbitrarily large files. The source file is left untouched.
    ///
    /// # Idempotence guarantee
    /// - Repeated calls for identical content return the same `content_hash`
    ///   and an equivalent `blob_id`; data is written only once per content
    ///   hash, even across distinct source paths.
    async fn ingest_path(&self, source_path: &Path) -> anyhow::Result<IngestedBlob>;

    /// Compute the device-independent content hash of the file at `source_path`
    /// without writing it to storage.
    ///
    /// Streams the file to derive its blake3 `ContentHash` with bounded memory,
    /// so the call is suitable for arbitrarily large files; no blob is
    /// materialized and the source file is left untouched. For identical bytes
    /// this returns the same `content_hash` value that [`Self::ingest_path`]
    /// surfaces, without paying the storage write.
    async fn hash_path(&self, source_path: &Path) -> anyhow::Result<ContentHash>;
}
