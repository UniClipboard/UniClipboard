//! Blob Store Port (infra-internal).
//!
//! Abstracts the on-disk (or encrypted-on-disk) blob storage backend.
//! Exposes the low-level put/get operations used by `BlobWriter` and by
//! use cases that need to read raw blob bytes.
//!
//! This port lives in `uc-infra` rather than `uc-core` because its `put`
//! contract returns a `PathBuf` and an optional on-disk compressed size —
//! both are storage-implementation concerns with no domain meaning.

use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uc_core::{BlobId, ContentHash};

/// Outcome of persisting a path-backed file into blob storage.
///
/// The `content_hash` and `size_bytes` are derived from the exact bytes the
/// store persisted (computed in the same pass that wrote them), so a caller can
/// record an identity that cannot diverge from the stored blob — even if the
/// source file is rewritten immediately after the call returns.
#[derive(Debug, Clone)]
pub struct StoredPathBlob {
    /// On-disk location of the stored blob.
    pub storage_path: PathBuf,
    /// blake3 content hash of the source file's plaintext bytes.
    pub content_hash: ContentHash,
    /// Plaintext byte size of the source file.
    pub size_bytes: u64,
    /// On-disk byte count after any compression+encryption. `None` if the store
    /// does not track compressed size (e.g., raw filesystem).
    pub compressed_size: Option<i64>,
}

#[async_trait]
pub trait BlobStorePort: Send + Sync {
    /// Write bytes into blob storage, returning (storage_path, compressed_size).
    ///
    /// The `Option<i64>` is the on-disk byte count after any compression+encryption.
    /// Returns `None` if the store does not track compressed size (e.g., raw filesystem).
    async fn put(&self, blob_id: &BlobId, data: &[u8]) -> Result<(PathBuf, Option<i64>)>;

    /// Read bytes from blob storage.
    async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>>;

    /// 把 `source_path` 上的本地文件登记进 blob 存储。
    ///
    /// 推荐实现策略:先尝试 `hard_link`(O(1) 不占额外磁盘),失败(典型 EXDEV — 跨卷 /
    /// 跨文件系统)再回退到流式 copy。
    ///
    /// 返回的 [`StoredPathBlob`] 携带由 store **实际落盘的同一份字节**单遍算出的
    /// `content_hash` 与 `size_bytes`——调用方据此记录身份,即可保证 DB 记的 hash/size
    /// 与 blob 持有的字节永不分叉(消除 hash 与落盘之间的 TOCTOU 窗口)。
    ///
    /// 实现不得改动 source 文件;若 store 是带加密 decorator,该接口会被 decorator
    /// 重写为"读 → 加密 → 写新文件",而不是直接 hardlink(因为加密产物字节不同),
    /// 但 `content_hash` 始终是源文件明文的 hash。
    async fn put_from_path(&self, blob_id: &BlobId, source_path: &Path) -> Result<StoredPathBlob>;

    /// Remove a stored blob's bytes from storage.
    ///
    /// Idempotent: removing a blob that is already absent is not an error. Used
    /// to drop a freshly written blob when content-hash deduplication finds an
    /// existing record for the same bytes.
    async fn delete(&self, blob_id: &BlobId) -> Result<()>;
}

#[async_trait]
impl<T: BlobStorePort + ?Sized> BlobStorePort for Arc<T> {
    async fn put(&self, blob_id: &BlobId, data: &[u8]) -> Result<(PathBuf, Option<i64>)> {
        (**self).put(blob_id, data).await
    }

    async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>> {
        (**self).get(blob_id).await
    }

    async fn put_from_path(&self, blob_id: &BlobId, source_path: &Path) -> Result<StoredPathBlob> {
        (**self).put_from_path(blob_id, source_path).await
    }

    async fn delete(&self, blob_id: &BlobId) -> Result<()> {
        (**self).delete(blob_id).await
    }
}
