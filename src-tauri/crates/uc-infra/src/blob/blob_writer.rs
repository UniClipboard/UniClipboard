use anyhow::{Context, Result};
use async_trait::async_trait;
use std::io::Read;
use std::path::Path;
use tracing::{debug, debug_span, Instrument};
use uc_core::blob::ports::BlobWriterPort;
use uc_core::ports::ClockPort;
use uc_core::BlobId;
use uc_core::ContentHash;

use crate::blob::{Blob, BlobRepositoryPort, BlobStorageLocator, BlobStorePort};

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
        let span = debug_span!(
            "infra.blob.write_path_if_absent",
            source_path = %source_path.display(),
        );
        let source = source_path.to_path_buf();

        async move {
            // 1. 流式读取源文件计算 ContentHash —— 不把整文件载入内存,以支持任意大小。
            let hash_source = source.clone();
            let (content_id, file_size) =
                tokio::task::spawn_blocking(move || stream_hash_file(&hash_source))
                    .await
                    .context("hash join failed")??;
            debug!(
                content_hash = %content_id,
                file_size,
                "Computed content hash for path-backed ingest"
            );

            // 2. 已有同 hash blob → 直接复用,不再做盘 IO。
            if let Some(existing) = self.blob_repo.find_by_hash(&content_id).await? {
                debug!(
                    content_hash = %content_id,
                    blob_id = %existing.blob_id,
                    "Path ingest: dedup hit, reusing existing blob"
                );
                return Ok(existing.blob_id);
            }

            // 3. 未命中 → 走 BlobStorePort.put_from_path(hardlink 优先,跨卷 fallback copy;
            //    加密 decorator 会 override 为读 → 加密 → 写)。
            let blob_id = BlobId::new();
            let (storage_path, compressed_size) =
                self.blob_store.put_from_path(&blob_id, &source).await?;

            let created_at_ms = self.clock.now_ms();
            let blob_storage_locator = BlobStorageLocator::new_local_fs(storage_path);
            let record = Blob::new(
                blob_id.clone(),
                blob_storage_locator,
                file_size as i64,
                content_id.clone(),
                created_at_ms,
                compressed_size,
            );

            if let Err(err) = self.blob_repo.insert_blob(&record).await {
                if let Some(existing) = self.blob_repo.find_by_hash(&content_id).await? {
                    debug!(
                        error = %err,
                        content_hash = %content_id,
                        "Path ingest insert raced with existing blob; returning existing record",
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
}

/// 对 `path` 流式做 blake3 哈希,返回 (ContentHash, file_size_bytes)。
/// 64 KiB 缓冲,常驻内存与文件大小无关。
fn stream_hash_file(path: &Path) -> Result<(ContentHash, u64)> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open {} for hashing", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = file.read(&mut buf).context("read failed during hashing")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total = total.saturating_add(n as u64);
    }
    let hash = hasher.finalize();
    Ok((ContentHash::from(hash.as_bytes()), total))
}
