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
use uc_core::BlobId;

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
    /// 跨文件系统)再回退到流式 copy。返回 `(storage_path, compressed_size)`,语义与
    /// `put` 一致。
    ///
    /// 实现不得改动 source 文件;若 store 是带加密 decorator,该接口会被 decorator
    /// 重写为"读 → 加密 → 写新文件",而不是直接 hardlink(因为加密产物字节不同)。
    async fn put_from_path(
        &self,
        blob_id: &BlobId,
        source_path: &Path,
    ) -> Result<(PathBuf, Option<i64>)>;
}

#[async_trait]
impl<T: BlobStorePort + ?Sized> BlobStorePort for Arc<T> {
    async fn put(&self, blob_id: &BlobId, data: &[u8]) -> Result<(PathBuf, Option<i64>)> {
        (**self).put(blob_id, data).await
    }

    async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>> {
        (**self).get(blob_id).await
    }

    async fn put_from_path(
        &self,
        blob_id: &BlobId,
        source_path: &Path,
    ) -> Result<(PathBuf, Option<i64>)> {
        (**self).put_from_path(blob_id, source_path).await
    }
}
