use anyhow::{Ok, Result};
use async_trait::async_trait;
use tracing::{debug, debug_span, Instrument};
use uc_core::blob::BlobStorageLocator;
use uc_core::ports::ClockPort;
use uc_core::ports::{BlobRepositoryPort, BlobStorePort, BlobWriterPort};
use uc_core::ContentHash;
use uc_core::{Blob, BlobId};

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
    ) -> Result<Blob> {
        let span = debug_span!(
            "infra.blob.write_if_absent",
            size_bytes = plaintext_bytes.len(),
            content_hash = %content_id,
        );
        async {
            if let Some(blob) = self.blob_repo.find_by_hash(content_id).await? {
                return Ok(blob);
            }

            let blob_id = BlobId::new();

            // Encryption is handled by the injected BlobStorePort decorator (if any).
            let (storage_path, compressed_size) =
                self.blob_store.put(&blob_id, plaintext_bytes).await?;

            let created_at_ms = self.clock.now_ms();
            let blob_storage_locator = BlobStorageLocator::new_local_fs(storage_path);
            let result = Blob::new(
                blob_id,
                blob_storage_locator,
                plaintext_bytes.len() as i64,
                content_id.clone(),
                created_at_ms,
                compressed_size,
            );

            if let Err(err) = self.blob_repo.insert_blob(&result).await {
                if let Some(existing) = self.blob_repo.find_by_hash(content_id).await? {
                    debug!(
                        error = %err,
                        content_hash = %content_id,
                        "Insert raced with existing blob; returning existing record",
                    );
                    return Ok(existing);
                }
                return Err(err);
            }
            Ok(result)
        }
        .instrument(span)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::path::PathBuf;
    use uc_core::blob::BlobStorageLocator;

    /// Mock BlobStorePort
    struct MockBlobStore {
        storage_path: PathBuf,
    }

    impl MockBlobStore {
        fn new() -> Self {
            Self {
                storage_path: PathBuf::from("/test/storage"),
            }
        }
    }

    #[async_trait]
    impl BlobStorePort for MockBlobStore {
        async fn put(&self, _blob_id: &BlobId, _data: &[u8]) -> Result<(PathBuf, Option<i64>)> {
            Ok((self.storage_path.clone(), None))
        }

        async fn get(&self, _blob_id: &BlobId) -> Result<Vec<u8>> {
            Ok(vec![])
        }
    }

    /// Mock BlobRepositoryPort
    struct MockBlobRepo {
        existing_blob: Option<Blob>,
        should_fail_insert: bool,
    }

    impl MockBlobRepo {
        fn new() -> Self {
            Self {
                existing_blob: None,
                should_fail_insert: false,
            }
        }

        fn with_existing_blob(mut self, blob: Blob) -> Self {
            self.existing_blob = Some(blob);
            self
        }

        fn with_insert_failure(mut self) -> Self {
            self.should_fail_insert = true;
            self
        }
    }

    #[async_trait]
    impl BlobRepositoryPort for MockBlobRepo {
        async fn insert_blob(&self, _blob: &Blob) -> Result<()> {
            if self.should_fail_insert {
                Err(anyhow::anyhow!("Insert failed"))
            } else {
                Ok(())
            }
        }

        async fn find_by_hash(&self, _content_hash: &ContentHash) -> Result<Option<Blob>> {
            Ok(self.existing_blob.clone())
        }
    }

    /// Mock ClockPort
    struct MockClock;

    impl MockClock {
        fn new() -> Self {
            Self
        }
    }

    impl ClockPort for MockClock {
        fn now_ms(&self) -> i64 {
            1234567890
        }
    }

    #[tokio::test]
    async fn test_write_if_absent_creates_new_blob() {
        let blob_store = MockBlobStore::new();
        let blob_repo = MockBlobRepo::new();
        let clock = MockClock::new();

        let writer = BlobWriter::new(blob_store, blob_repo, clock);

        // ContentHash format: blake3v1:hex_bytes (64 hex chars for 32 bytes)
        let content_id = ContentHash::from(
            "blake3v1:0000000000000000000000000000000000000000000000000000000000000000",
        );
        let data = b"test data";

        let result = writer.write_if_absent(&content_id, data).await;

        assert!(result.is_ok(), "write_if_absent should succeed");
        let blob = result.unwrap();
        assert_eq!(blob.size_bytes, data.len() as i64);
        assert_eq!(blob.created_at_ms, 1234567890);
    }

    #[tokio::test]
    async fn test_write_if_absent_returns_existing_blob() {
        let blob_store = MockBlobStore::new();
        let existing_blob = Blob::new(
            BlobId::new(),
            BlobStorageLocator::new_local_fs(PathBuf::from("/existing")),
            100,
            ContentHash::from(
                "blake3v1:1111111111111111111111111111111111111111111111111111111111111111",
            ),
            1111111111,
            None,
        );
        let blob_repo = MockBlobRepo::new().with_existing_blob(existing_blob);
        let clock = MockClock::new();

        let writer = BlobWriter::new(blob_store, blob_repo, clock);

        let content_id = ContentHash::from(
            "blake3v1:1111111111111111111111111111111111111111111111111111111111111111",
        );
        let data = b"test data";

        let result = writer.write_if_absent(&content_id, data).await;

        assert!(result.is_ok(), "write_if_absent should succeed");
        let blob = result.unwrap();
        assert_eq!(blob.size_bytes, 100);
        assert_eq!(blob.created_at_ms, 1111111111);
    }

    #[tokio::test]
    async fn test_write_if_absent_handles_race_condition() {
        let blob_store = MockBlobStore::new();
        let existing_blob = Blob::new(
            BlobId::new(),
            BlobStorageLocator::new_local_fs(PathBuf::from("/existing")),
            200,
            ContentHash::from(
                "blake3v1:2222222222222222222222222222222222222222222222222222222222222222",
            ),
            2222222222,
            None,
        );
        let blob_repo = MockBlobRepo::new()
            .with_existing_blob(existing_blob)
            .with_insert_failure();
        let clock = MockClock::new();

        let writer = BlobWriter::new(blob_store, blob_repo, clock);

        let content_id = ContentHash::from(
            "blake3v1:2222222222222222222222222222222222222222222222222222222222222222",
        );
        let data = b"race data";

        let result = writer.write_if_absent(&content_id, data).await;

        // Should succeed because existing blob is returned after insert failure
        assert!(
            result.is_ok(),
            "write_if_absent should succeed with existing blob"
        );
        let blob = result.unwrap();
        assert_eq!(blob.size_bytes, 200);
        assert_eq!(blob.created_at_ms, 2222222222);
    }

    #[tokio::test]
    async fn test_write_if_absent_fails_when_insert_fails_without_existing() {
        let blob_store = MockBlobStore::new();
        let blob_repo = MockBlobRepo::new().with_insert_failure();
        let clock = MockClock::new();

        let writer = BlobWriter::new(blob_store, blob_repo, clock);

        let content_id = ContentHash::from(
            "blake3v1:3333333333333333333333333333333333333333333333333333333333333333",
        );
        let data = b"fail data";

        let result = writer.write_if_absent(&content_id, data).await;

        assert!(
            result.is_err(),
            "write_if_absent should fail when insert fails and no existing blob"
        );
    }
}
