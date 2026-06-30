use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::Path;
use tracing::{debug, debug_span, warn, Instrument};
use uc_core::blob::ports::{BlobContentIngestPort, BlobWriterPort, IngestedBlob};
use uc_core::ports::ClockPort;
use uc_core::BlobId;
use uc_core::ContentHash;

use crate::blob::hashing::stream_hash_file;
use crate::blob::{Blob, BlobRepositoryPort, BlobStorageLocator, BlobStorePort, StoredPathBlob};

pub struct BlobWriter<B, BR, C>
where
    B: BlobStorePort,
    BR: BlobRepositoryPort,
    C: ClockPort,
{
    blob_store: B,
    blob_repo: BR,
    clock: C,
}

impl<B, BR, C> BlobWriter<B, BR, C>
where
    B: BlobStorePort,
    BR: BlobRepositoryPort,
    C: ClockPort,
{
    pub fn new(blob_store: B, blob_repo: BR, clock: C) -> Self {
        BlobWriter {
            blob_store,
            blob_repo,
            clock,
        }
    }

    /// Stream a path-backed file into blob storage, deduplicating by content
    /// hash, and surface the storage handle together with the content hash and
    /// byte size computed during the single storing pass.
    ///
    /// Shared core for both `BlobWriterPort::write_path_if_absent` (which
    /// discards everything but the `BlobId`) and `BlobContentIngestPort::ingest_path`.
    ///
    /// The content hash is derived by the store from the exact bytes it persists
    /// (a single pass), so the recorded `content_hash`/`size_bytes` can never
    /// diverge from the stored blob — there is no separate hash pass that a
    /// concurrent source rewrite could race against. Deduplication therefore runs
    /// *after* the write, against the authoritative hash; on a dedup hit the
    /// freshly written blob is dropped and the existing record is reused.
    async fn ingest_path_inner(&self, source_path: &Path) -> Result<IngestedBlob> {
        // No source_path field: a clipboard file path is user content (it can
        // carry usernames / sensitive filenames). The inner logs identify the
        // op by blob_id and size instead.
        let span = debug_span!("infra.blob.ingest_path");
        let source = source_path.to_path_buf();

        async move {
            // 1. 落盘:store 在写入的同一遍里算出权威 content_hash + size,二者与实际
            //    持有的字节同源(hardlink 时哈希 dest,copy/加密时哈希落盘字节流)。
            let blob_id = BlobId::new();
            let StoredPathBlob {
                storage_path,
                content_hash,
                size_bytes,
                compressed_size,
            } = match self.blob_store.put_from_path(&blob_id, &source).await {
                Ok(stored) => stored,
                Err(err) => {
                    // A partial write (mid-copy read error, post-link hash
                    // failure, …) may have left a file under this blob_id. Clean
                    // it up so a failed ingest never leaks an orphan blob.
                    self.discard_blob(&blob_id, "put_from_path failed").await;
                    return Err(err);
                }
            };
            debug!(
                content_hash = %content_hash,
                file_size = size_bytes,
                "Persisted path-backed blob; recording by authoritative content hash"
            );

            // 2. 按权威 hash 去重:已有同内容 blob → 丢弃刚落盘的副本,复用既有记录。
            //    blob 此刻已落盘但尚无 DB 记录,因此每一条 post-write 失败退出路径都必须
            //    先 discard,否则会留下无记录引用的孤儿 blob。
            match self.blob_repo.find_by_hash(&content_hash).await {
                Ok(Some(existing)) => {
                    debug!(
                        content_hash = %content_hash,
                        blob_id = %existing.blob_id,
                        "Path ingest: dedup hit, dropping freshly written blob and reusing existing"
                    );
                    self.discard_blob(&blob_id, "dedup hit").await;
                    return Ok(IngestedBlob {
                        blob_id: existing.blob_id,
                        content_hash,
                        size_bytes,
                    });
                }
                Ok(None) => {}
                Err(err) => {
                    self.discard_blob(&blob_id, "dedup lookup failed").await;
                    return Err(err);
                }
            }

            let created_at_ms = self.clock.now_ms();
            let blob_storage_locator = BlobStorageLocator::new_local_fs(storage_path);
            let record = Blob::new(
                blob_id.clone(),
                blob_storage_locator,
                size_bytes as i64,
                content_hash.clone(),
                created_at_ms,
                compressed_size,
            );

            if let Err(insert_err) = self.blob_repo.insert_blob(&record).await {
                // Insert most likely lost the content_hash UNIQUE race with a
                // concurrent ingest; reuse the winner's record if it is present.
                if let Ok(Some(existing)) = self.blob_repo.find_by_hash(&content_hash).await {
                    debug!(
                        error = %insert_err,
                        content_hash = %content_hash,
                        "Path ingest insert raced with existing blob; dropping freshly written blob and returning existing record",
                    );
                    self.discard_blob(&blob_id, "insert race").await;
                    return Ok(IngestedBlob {
                        blob_id: existing.blob_id,
                        content_hash,
                        size_bytes,
                    });
                }
                // No existing record (or the re-check itself errored): the stored
                // blob has no owning row, so drop it before surfacing the error.
                self.discard_blob(&blob_id, "insert failed").await;
                return Err(insert_err);
            }
            Ok(IngestedBlob {
                blob_id,
                content_hash,
                size_bytes,
            })
        }
        .instrument(span)
        .await
    }

    /// Remove a blob that was written but will not be referenced by any record
    /// (dedup hit, insert race, or a failed write that left a partial file). A
    /// failed cleanup is logged but not propagated: the redundant blob is an
    /// orphan, not a correctness fault for the ingest the caller asked for.
    async fn discard_blob(&self, blob_id: &BlobId, reason: &str) {
        if let Err(err) = self.blob_store.delete(blob_id).await {
            warn!(
                error = %err,
                blob_id = %blob_id,
                reason,
                "Failed to remove redundant blob during path ingest cleanup"
            );
        }
    }
}

#[async_trait]
impl<B, BR, C> BlobWriterPort for BlobWriter<B, BR, C>
where
    B: BlobStorePort,
    BR: BlobRepositoryPort,
    C: ClockPort,
{
    async fn write_if_absent(
        &self,
        content_id: &ContentHash,
        plaintext_bytes: &[u8],
    ) -> Result<BlobId> {
        let span = debug_span!(
            "infra.blob.write_if_absent",
            size_bytes = plaintext_bytes.len(),
            content_hash = %content_id,
        );
        async {
            if let Some(existing) = self.blob_repo.find_by_hash(content_id).await? {
                return Ok(existing.blob_id);
            }

            let blob_id = BlobId::new();

            // Encryption is handled by the injected BlobStorePort decorator (if any).
            let (storage_path, compressed_size) =
                self.blob_store.put(&blob_id, plaintext_bytes).await?;

            let created_at_ms = self.clock.now_ms();
            let blob_storage_locator = BlobStorageLocator::new_local_fs(storage_path);
            let record = Blob::new(
                blob_id.clone(),
                blob_storage_locator,
                plaintext_bytes.len() as i64,
                content_id.clone(),
                created_at_ms,
                compressed_size,
            );

            if let Err(err) = self.blob_repo.insert_blob(&record).await {
                if let Some(existing) = self.blob_repo.find_by_hash(content_id).await? {
                    debug!(
                        error = %err,
                        content_hash = %content_id,
                        "Insert raced with existing blob; returning existing record",
                    );
                    return Ok(existing.blob_id);
                }
                return Err(err);
            }
            Ok(blob_id)
        }
        .instrument(span)
        .await
    }

    async fn write_path_if_absent(&self, source_path: &Path) -> Result<BlobId> {
        Ok(self.ingest_path_inner(source_path).await?.blob_id)
    }
}

#[async_trait]
impl<B, BR, C> BlobContentIngestPort for BlobWriter<B, BR, C>
where
    B: BlobStorePort,
    BR: BlobRepositoryPort,
    C: ClockPort,
{
    async fn ingest_path(&self, source_path: &Path) -> Result<IngestedBlob> {
        self.ingest_path_inner(source_path).await
    }

    async fn hash_path(&self, source_path: &Path) -> Result<ContentHash> {
        let span = debug_span!(
            "infra.blob.hash_path",
            source_path = %source_path.display(),
        );
        let source = source_path.to_path_buf();
        async move {
            // Stream the file to compute its ContentHash without loading it into
            // memory or writing any blob — identity only, no materialization.
            let (content_id, file_size) =
                tokio::task::spawn_blocking(move || stream_hash_file(&source))
                    .await
                    .context("hash join failed")??;
            debug!(
                content_hash = %content_id,
                file_size,
                "Computed content hash for path (identity only, no materialization)"
            );
            Ok(content_id)
        }
        .instrument(span)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blob::FilesystemBlobStore;
    use std::sync::{Arc, Mutex};

    struct FixedClock(i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    /// In-memory blob repository that enforces the same `content_hash UNIQUE`
    /// constraint the SQLite-backed repository relies on, so the dedup/insert-race
    /// paths exercise realistic behaviour.
    #[derive(Default)]
    struct InMemoryBlobRepo {
        rows: Mutex<Vec<Blob>>,
    }

    #[async_trait]
    impl BlobRepositoryPort for InMemoryBlobRepo {
        async fn insert_blob(&self, blob: &Blob) -> Result<()> {
            let mut rows = self.rows.lock().unwrap();
            if rows.iter().any(|b| b.content_hash == blob.content_hash) {
                anyhow::bail!("UNIQUE constraint failed: blob.content_hash");
            }
            rows.push(blob.clone());
            Ok(())
        }

        async fn find_by_hash(&self, content_hash: &ContentHash) -> Result<Option<Blob>> {
            let rows = self.rows.lock().unwrap();
            Ok(rows
                .iter()
                .find(|b| &b.content_hash == content_hash)
                .cloned())
        }
    }

    fn hash_of(bytes: &[u8]) -> ContentHash {
        ContentHash::from(blake3::hash(bytes).as_bytes())
    }

    fn count_blob_files(dir: &Path) -> usize {
        std::fs::read_dir(dir).map(|rd| rd.count()).unwrap_or(0)
    }

    #[tokio::test]
    async fn ingest_records_hash_matching_stored_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(FilesystemBlobStore::new(tmp.path().join("blobs")));
        let repo = Arc::new(InMemoryBlobRepo::default());
        let writer = BlobWriter::new(store.clone(), repo, FixedClock(123));

        let src = tmp.path().join("a.bin");
        let content = b"writer ingest: identity matches stored bytes".to_vec();
        tokio::fs::write(&src, &content).await.unwrap();

        let ingested = writer.ingest_path(&src).await.unwrap();
        assert_eq!(ingested.content_hash, hash_of(&content));
        assert_eq!(ingested.size_bytes, content.len() as u64);

        let stored_bytes = BlobStorePort::get(store.as_ref(), &ingested.blob_id)
            .await
            .unwrap();
        assert_eq!(hash_of(&stored_bytes), ingested.content_hash);
    }

    #[tokio::test]
    async fn second_ingest_of_same_content_dedups_without_orphan() {
        let tmp = tempfile::tempdir().unwrap();
        let blob_dir = tmp.path().join("blobs");
        let store = Arc::new(FilesystemBlobStore::new(blob_dir.clone()));
        let repo = Arc::new(InMemoryBlobRepo::default());
        let writer = BlobWriter::new(store.clone(), repo.clone(), FixedClock(0));

        // Two distinct source paths carrying identical bytes.
        let content = b"dedupe me across two paths".to_vec();
        let src1 = tmp.path().join("one.bin");
        let src2 = tmp.path().join("two.bin");
        tokio::fs::write(&src1, &content).await.unwrap();
        tokio::fs::write(&src2, &content).await.unwrap();

        let first = writer.ingest_path(&src1).await.unwrap();
        let second = writer.ingest_path(&src2).await.unwrap();

        // Same content → same identity, and the existing blob is reused.
        assert_eq!(first.content_hash, second.content_hash);
        assert_eq!(first.blob_id, second.blob_id);
        // The redundant second write was discarded: exactly one blob on disk and
        // one row in the repository, no orphan left behind.
        assert_eq!(count_blob_files(&blob_dir), 1);
        assert_eq!(repo.rows.lock().unwrap().len(), 1);
    }
}
