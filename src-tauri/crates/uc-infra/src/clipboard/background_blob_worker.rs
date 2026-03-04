//! Background worker to materialize blobs from staged representations.
//! 从暂存表示中异步生成 blob 的后台工作者。

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{error, info_span, warn, Instrument};
use uc_core::clipboard::PayloadAvailability;
use uc_core::clipboard::{ThumbnailMetadata, TimestampMs};
use uc_core::ids::RepresentationId;
use uc_core::ports::clipboard::{
    ProcessingUpdateOutcome, ThumbnailGeneratorPort, ThumbnailRepositoryPort,
};
use uc_core::ports::{
    BlobWriterPort, ClipboardRepresentationRepositoryPort, ClockPort, ContentHashPort,
};

use crate::clipboard::{RepresentationCache, SpoolManager};

/// Background worker that materializes blob data from cache/spool.
/// 从缓存/磁盘缓存中物化 blob 数据的后台工作者。
pub struct BackgroundBlobWorker {
    worker_rx: mpsc::Receiver<RepresentationId>,
    cache: Arc<RepresentationCache>,
    spool: Arc<SpoolManager>,
    repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    blob_writer: Arc<dyn BlobWriterPort>,
    hasher: Arc<dyn ContentHashPort>,
    thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
    thumbnail_generator: Arc<dyn ThumbnailGeneratorPort>,
    clock: Arc<dyn ClockPort>,
    retry_max_attempts: u32,
    retry_backoff: Duration,
}

impl BackgroundBlobWorker {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        worker_rx: mpsc::Receiver<RepresentationId>,
        cache: Arc<RepresentationCache>,
        spool: Arc<SpoolManager>,
        repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
        blob_writer: Arc<dyn BlobWriterPort>,
        hasher: Arc<dyn ContentHashPort>,
        thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
        thumbnail_generator: Arc<dyn ThumbnailGeneratorPort>,
        clock: Arc<dyn ClockPort>,
        retry_max_attempts: u32,
        retry_backoff: Duration,
    ) -> Self {
        Self {
            worker_rx,
            cache,
            spool,
            repo,
            blob_writer,
            hasher,
            thumbnail_repo,
            thumbnail_generator,
            clock,
            retry_max_attempts,
            retry_backoff,
        }
    }

    /// Run the worker loop until the channel is closed.
    /// 运行工作循环，直到通道关闭。
    pub async fn run(mut self) {
        while let Some(rep_id) = self.worker_rx.recv().await {
            let span = info_span!(
                "infra.background_blob_worker",
                representation_id = %rep_id,
            );
            let result = self.process_with_retry(rep_id).instrument(span).await;
            if let Err(err) = result {
                error!(error = %err, "Failed to process representation");
            }
        }
    }

    async fn process_with_retry(&self, rep_id: RepresentationId) -> Result<()> {
        let mut attempt: u32 = 1;
        loop {
            match self.process_once(&rep_id).await {
                Ok(ProcessResult::Completed) => return Ok(()),
                Ok(ProcessResult::MissingBytes) => return Ok(()),
                Err(err) => {
                    if attempt >= self.retry_max_attempts {
                        let last_error = format!("worker failed after {attempt} attempts: {err}");
                        self.mark_failed(&rep_id, &last_error).await?;
                        return Err(err);
                    }

                    warn!(
                        attempt,
                        max_attempts = self.retry_max_attempts,
                        error = %err,
                        "Processing failed; retrying"
                    );
                    let backoff = self.retry_backoff.mul_f32(attempt as f32);
                    sleep(backoff).await;
                    attempt = attempt.saturating_add(1);
                }
            }
        }
    }

    async fn process_once(&self, rep_id: &RepresentationId) -> Result<ProcessResult> {
        // Transition to Processing (idempotent for staged/processing).
        match self
            .repo
            .update_processing_result(
                rep_id,
                &[PayloadAvailability::Staged, PayloadAvailability::Processing],
                None,
                PayloadAvailability::Processing,
                None,
            )
            .await
        {
            Ok(ProcessingUpdateOutcome::Updated(_)) => {}
            Ok(ProcessingUpdateOutcome::StateMismatch) => {
                warn!(
                    representation_id = %rep_id,
                    "Skipping processing due to state mismatch"
                );
                return Ok(ProcessResult::Completed);
            }
            Ok(ProcessingUpdateOutcome::NotFound) => {
                warn!(representation_id = %rep_id, "Representation missing");
                return Ok(ProcessResult::Completed);
            }
            Err(err) => {
                // Propagate error to trigger retry in process_with_retry
                return Err(err);
            }
        }

        let cached = self.cache.get(rep_id).await;

        let raw_bytes = if let Some(bytes) = cached {
            tracing::debug!(representation_id = %rep_id, "Worker cache hit");
            bytes
        } else {
            match self.spool.read(rep_id).await? {
                Some(bytes) => {
                    tracing::debug!(representation_id = %rep_id, "Worker spool hit");
                    bytes
                }
                None => {
                    let last_error = "cache/spool miss: bytes not available";
                    warn!(
                        representation_id = %rep_id,
                        cache_hit = false,
                        "Bytes missing in cache and spool; returning representation to Staged"
                    );
                    match self
                        .repo
                        .update_processing_result(
                            rep_id,
                            &[PayloadAvailability::Processing],
                            None,
                            PayloadAvailability::Staged,
                            Some(last_error),
                        )
                        .await
                    {
                        Ok(ProcessingUpdateOutcome::Updated(_)) => {}
                        Ok(ProcessingUpdateOutcome::StateMismatch) => {
                            warn!(
                                representation_id = %rep_id,
                                "Skipping revert to Staged due to state mismatch"
                            );
                        }
                        Ok(ProcessingUpdateOutcome::NotFound) => {
                            warn!(representation_id = %rep_id, "Representation missing");
                        }
                        Err(err) => {
                            warn!(
                                representation_id = %rep_id,
                                error = %err,
                                "Failed to revert representation to Staged after cache/spool miss"
                            );
                        }
                    }
                    return Ok(ProcessResult::MissingBytes);
                }
            }
        };

        let content_hash = self
            .hasher
            .hash_bytes(&raw_bytes)
            .context("failed to hash representation bytes")?;

        // BlobWriterPort should handle deduplication; data is passed as-is.
        let blob = self
            .blob_writer
            .write_if_absent(&content_hash, &raw_bytes)
            .await
            .context("failed to write blob")?;

        let updated = self
            .repo
            .update_processing_result(
                rep_id,
                &[PayloadAvailability::Processing],
                Some(&blob.blob_id),
                PayloadAvailability::BlobReady,
                None,
            )
            .await;

        match updated {
            Ok(ProcessingUpdateOutcome::Updated(_)) => {
                if let Err(err) = self.spool.delete(rep_id).await {
                    warn!(
                        representation_id = %rep_id,
                        error = %err,
                        "Failed to delete spool entry after blob materialization"
                    );
                }
                self.try_generate_thumbnail(rep_id, &raw_bytes).await;
                Ok(ProcessResult::Completed)
            }
            Ok(ProcessingUpdateOutcome::StateMismatch) => {
                warn!(
                    representation_id = %rep_id,
                    "Skipping update due to state mismatch"
                );
                Ok(ProcessResult::Completed)
            }
            Ok(ProcessingUpdateOutcome::NotFound) => {
                warn!(representation_id = %rep_id, "Representation missing");
                Ok(ProcessResult::Completed)
            }
            Err(err) => {
                warn!(
                    representation_id = %rep_id,
                    error = %err,
                    "Failed to update representation state after blob write"
                );
                Err(err)
            }
        }
    }

    async fn mark_failed(&self, rep_id: &RepresentationId, last_error: &str) -> Result<()> {
        match self
            .repo
            .update_processing_result(
                rep_id,
                &[PayloadAvailability::Processing, PayloadAvailability::Staged],
                None,
                PayloadAvailability::Failed {
                    last_error: last_error.to_string(),
                },
                Some(last_error),
            )
            .await
        {
            Ok(ProcessingUpdateOutcome::Updated(_)) => {}
            Ok(ProcessingUpdateOutcome::StateMismatch) => {
                warn!(
                    representation_id = %rep_id,
                    "Skipping mark_failed due to state mismatch"
                );
            }
            Ok(ProcessingUpdateOutcome::NotFound) => {
                warn!(representation_id = %rep_id, "Representation missing");
            }
            Err(err) => {
                error!(
                    representation_id = %rep_id,
                    error = %err,
                    "Failed to mark representation as Failed"
                );
            }
        }
        Ok(())
    }

    async fn try_generate_thumbnail(&self, rep_id: &RepresentationId, raw_bytes: &[u8]) {
        if let Err(err) = self.generate_thumbnail(rep_id, raw_bytes).await {
            error!(
                representation_id = %rep_id,
                error = %err,
                "Failed to generate thumbnail"
            );
        }
    }

    async fn generate_thumbnail(&self, rep_id: &RepresentationId, raw_bytes: &[u8]) -> Result<()> {
        let rep = match self.repo.get_representation_by_id(rep_id).await? {
            Some(rep) => rep,
            None => {
                warn!(
                    representation_id = %rep_id,
                    "Representation missing while generating thumbnail"
                );
                return Ok(());
            }
        };

        if rep.inline_data.is_some() {
            return Ok(());
        }

        let is_image = rep
            .mime_type
            .as_ref()
            .map(|mime| mime.as_str().starts_with("image/"))
            .unwrap_or(false);
        if !is_image {
            return Ok(());
        }

        if self
            .thumbnail_repo
            .get_by_representation_id(rep_id)
            .await?
            .is_some()
        {
            return Ok(());
        }

        let generated = self
            .thumbnail_generator
            .generate_thumbnail(raw_bytes)
            .await
            .context("failed to generate thumbnail")?;

        let thumbnail_hash = self
            .hasher
            .hash_bytes(&generated.thumbnail_bytes)
            .context("failed to hash thumbnail bytes")?;

        let thumbnail_blob = self
            .blob_writer
            .write_if_absent(&thumbnail_hash, &generated.thumbnail_bytes)
            .await
            .context("failed to write thumbnail blob")?;

        let created_at_ms = TimestampMs::from_epoch_millis(self.clock.now_ms());
        let thumbnail_size_bytes = thumbnail_blob.size_bytes;
        let metadata = ThumbnailMetadata::new(
            rep_id.clone(),
            thumbnail_blob.blob_id,
            generated.thumbnail_mime_type,
            generated.original_width,
            generated.original_height,
            thumbnail_size_bytes,
            Some(created_at_ms),
        );
        self.thumbnail_repo
            .insert_thumbnail(&metadata)
            .await
            .context("failed to insert thumbnail metadata")?;

        Ok(())
    }
}

enum ProcessResult {
    Completed,
    MissingBytes,
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tokio::sync::Mutex as TokioMutex;
    use uc_core::blob::BlobStorageLocator;
    use uc_core::clipboard::{PersistedClipboardRepresentation, ThumbnailMetadata, TimestampMs};
    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::ports::clipboard::{
        GeneratedThumbnail, ThumbnailGeneratorPort, ThumbnailRepositoryPort,
    };
    use uc_core::ports::ClockPort;
    use uc_core::{Blob, BlobId, ContentHash, HashAlgorithm, MimeType};

    struct MockHasher;

    impl ContentHashPort for MockHasher {
        fn hash_bytes(&self, bytes: &[u8]) -> Result<ContentHash> {
            let hash = blake3::hash(bytes);
            Ok(ContentHash {
                alg: HashAlgorithm::Blake3V1,
                bytes: *hash.as_bytes(),
            })
        }
    }

    struct MockBlobWriter {
        blobs: TokioMutex<HashMap<ContentHash, Blob>>,
    }

    impl MockBlobWriter {
        fn new() -> Self {
            Self {
                blobs: TokioMutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl BlobWriterPort for MockBlobWriter {
        async fn write_if_absent(
            &self,
            content_id: &ContentHash,
            encrypted_bytes: &[u8],
        ) -> Result<Blob> {
            let mut blobs = self.blobs.lock().await;
            if let Some(existing) = blobs.get(content_id) {
                return Ok(existing.clone());
            }

            let blob = Blob::new(
                BlobId::new(),
                BlobStorageLocator::new_local_fs(PathBuf::from("/tmp/mock")),
                encrypted_bytes.len() as i64,
                content_id.clone(),
                0,
                None,
            );
            blobs.insert(content_id.clone(), blob.clone());
            Ok(blob)
        }
    }

    struct MockRepresentationRepo {
        reps: TokioMutex<HashMap<RepresentationId, PersistedClipboardRepresentation>>,
    }

    impl MockRepresentationRepo {
        fn new(reps: HashMap<RepresentationId, PersistedClipboardRepresentation>) -> Self {
            Self {
                reps: TokioMutex::new(reps),
            }
        }

        async fn get(&self, rep_id: &RepresentationId) -> Option<PersistedClipboardRepresentation> {
            let reps = self.reps.lock().await;
            reps.get(rep_id).cloned()
        }
    }

    struct MockThumbnailRepo {
        thumbnails: TokioMutex<HashMap<RepresentationId, ThumbnailMetadata>>,
    }

    impl MockThumbnailRepo {
        fn new() -> Self {
            Self {
                thumbnails: TokioMutex::new(HashMap::new()),
            }
        }

        fn clone_metadata(metadata: &ThumbnailMetadata) -> ThumbnailMetadata {
            ThumbnailMetadata::new(
                metadata.representation_id.clone(),
                metadata.thumbnail_blob_id.clone(),
                metadata.thumbnail_mime_type.clone(),
                metadata.original_width,
                metadata.original_height,
                metadata.original_size_bytes,
                metadata.created_at_ms,
            )
        }

        async fn get(&self, rep_id: &RepresentationId) -> Option<ThumbnailMetadata> {
            let thumbnails = self.thumbnails.lock().await;
            thumbnails
                .get(rep_id)
                .map(|metadata| Self::clone_metadata(metadata))
        }
    }

    #[async_trait]
    impl ThumbnailRepositoryPort for MockThumbnailRepo {
        async fn get_by_representation_id(
            &self,
            representation_id: &RepresentationId,
        ) -> Result<Option<ThumbnailMetadata>> {
            Ok(self.get(representation_id).await)
        }

        async fn insert_thumbnail(&self, metadata: &ThumbnailMetadata) -> Result<()> {
            let mut thumbnails = self.thumbnails.lock().await;
            thumbnails.insert(
                metadata.representation_id.clone(),
                Self::clone_metadata(metadata),
            );
            Ok(())
        }
    }

    struct MockThumbnailGenerator {
        generated: Option<GeneratedThumbnail>,
        fail: bool,
        calls: TokioMutex<u32>,
    }

    impl MockThumbnailGenerator {
        fn new_success(generated: GeneratedThumbnail) -> Self {
            Self {
                generated: Some(generated),
                fail: false,
                calls: TokioMutex::new(0),
            }
        }

        fn new_failure() -> Self {
            Self {
                generated: None,
                fail: true,
                calls: TokioMutex::new(0),
            }
        }

        async fn call_count(&self) -> u32 {
            *self.calls.lock().await
        }

        fn clone_generated(&self) -> Result<GeneratedThumbnail> {
            let generated = self
                .generated
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("missing generated thumbnail"))?;
            Ok(GeneratedThumbnail {
                thumbnail_bytes: generated.thumbnail_bytes.clone(),
                thumbnail_mime_type: generated.thumbnail_mime_type.clone(),
                original_width: generated.original_width,
                original_height: generated.original_height,
            })
        }
    }

    #[async_trait]
    impl ThumbnailGeneratorPort for MockThumbnailGenerator {
        async fn generate_thumbnail(&self, _image_bytes: &[u8]) -> Result<GeneratedThumbnail> {
            let mut calls = self.calls.lock().await;
            *calls += 1;
            drop(calls);

            if self.fail {
                return Err(anyhow::anyhow!("thumbnail generator failed"));
            }
            self.clone_generated()
        }
    }

    struct FixedClock {
        now_ms: i64,
    }

    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.now_ms
        }
    }

    #[async_trait]
    impl ClipboardRepresentationRepositoryPort for MockRepresentationRepo {
        async fn get_representation(
            &self,
            _event_id: &uc_core::ids::EventId,
            _representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn get_representation_by_id(
            &self,
            representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(self.get(representation_id).await)
        }

        async fn get_representation_by_blob_id(
            &self,
            _blob_id: &BlobId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn update_blob_id(
            &self,
            _representation_id: &RepresentationId,
            _blob_id: &BlobId,
        ) -> Result<()> {
            Ok(())
        }

        async fn update_blob_id_if_none(
            &self,
            _representation_id: &RepresentationId,
            _blob_id: &BlobId,
        ) -> Result<bool> {
            Ok(false)
        }

        async fn update_processing_result(
            &self,
            rep_id: &RepresentationId,
            expected_states: &[PayloadAvailability],
            blob_id: Option<&BlobId>,
            new_state: PayloadAvailability,
            last_error: Option<&str>,
        ) -> Result<ProcessingUpdateOutcome> {
            let mut reps = self.reps.lock().await;
            let current = match reps.get_mut(rep_id) {
                Some(rep) => rep,
                None => return Ok(ProcessingUpdateOutcome::NotFound),
            };

            let expected_state_strs: Vec<&str> =
                expected_states.iter().map(|s| s.as_str()).collect();
            if !expected_state_strs.contains(&current.payload_state.as_str()) {
                return Ok(ProcessingUpdateOutcome::StateMismatch);
            }

            current.payload_state = new_state.clone();
            current.last_error = last_error.map(|value| value.to_string());

            if let Some(blob_id) = blob_id {
                current.blob_id = Some(blob_id.clone());
            }

            Ok(ProcessingUpdateOutcome::Updated(current.clone()))
        }
    }

    fn create_representation(rep_id: &RepresentationId) -> PersistedClipboardRepresentation {
        PersistedClipboardRepresentation::new_staged(
            rep_id.clone(),
            FormatId::new(),
            Some(MimeType("image/png".to_string())),
            1024,
        )
    }

    fn default_thumbnail_deps() -> (
        Arc<MockThumbnailRepo>,
        Arc<MockThumbnailGenerator>,
        Arc<FixedClock>,
    ) {
        let repo = Arc::new(MockThumbnailRepo::new());
        let generator = Arc::new(MockThumbnailGenerator::new_success(GeneratedThumbnail {
            thumbnail_bytes: vec![1, 2, 3],
            thumbnail_mime_type: MimeType("image/webp".to_string()),
            original_width: 1,
            original_height: 1,
        }));
        let clock = Arc::new(FixedClock { now_ms: 1 });
        (repo, generator, clock)
    }

    #[tokio::test]
    async fn test_worker_generates_thumbnail() -> Result<()> {
        let rep_id = RepresentationId::new();
        let rep = create_representation(&rep_id);

        let mut reps = HashMap::new();
        reps.insert(rep_id.clone(), rep);

        let repo = Arc::new(MockRepresentationRepo::new(reps));
        let cache = Arc::new(RepresentationCache::new(10, 10_000));
        cache.put(&rep_id, vec![1, 2, 3, 4]).await;
        let spool = Arc::new(SpoolManager::new(tempfile::tempdir()?.path(), 10_000)?);
        let blob_writer = Arc::new(MockBlobWriter::new());
        let hasher = Arc::new(MockHasher);
        let thumbnail_repo = Arc::new(MockThumbnailRepo::new());
        let thumbnail_bytes = vec![8, 9, 10];
        let thumbnail_generator =
            Arc::new(MockThumbnailGenerator::new_success(GeneratedThumbnail {
                thumbnail_bytes: thumbnail_bytes.clone(),
                thumbnail_mime_type: MimeType("image/webp".to_string()),
                original_width: 120,
                original_height: 80,
            }));
        let clock = Arc::new(FixedClock { now_ms: 123 });

        let (tx, rx) = mpsc::channel(4);
        let worker = BackgroundBlobWorker::new(
            rx,
            cache,
            spool,
            repo.clone(),
            blob_writer,
            hasher,
            thumbnail_repo.clone(),
            thumbnail_generator.clone(),
            clock,
            3,
            Duration::from_millis(1),
        );

        let handle = tokio::spawn(worker.run());
        tx.send(rep_id.clone()).await?;
        drop(tx);
        handle.await?;

        let thumbnail = thumbnail_repo
            .get(&rep_id)
            .await
            .expect("thumbnail missing");
        assert_eq!(thumbnail.thumbnail_mime_type.as_str(), "image/webp");
        assert_eq!(thumbnail.original_width, 120);
        assert_eq!(thumbnail.original_height, 80);
        assert_eq!(thumbnail.original_size_bytes, thumbnail_bytes.len() as i64);
        assert_eq!(
            thumbnail.created_at_ms,
            Some(TimestampMs::from_epoch_millis(123))
        );
        assert_eq!(thumbnail_generator.call_count().await, 1);
        Ok(())
    }

    #[tokio::test]
    async fn test_worker_skips_thumbnail_when_existing() -> Result<()> {
        let rep_id = RepresentationId::new();
        let rep = create_representation(&rep_id);

        let mut reps = HashMap::new();
        reps.insert(rep_id.clone(), rep);

        let repo = Arc::new(MockRepresentationRepo::new(reps));
        let cache = Arc::new(RepresentationCache::new(10, 10_000));
        cache.put(&rep_id, vec![1, 2, 3, 4]).await;
        let spool = Arc::new(SpoolManager::new(tempfile::tempdir()?.path(), 10_000)?);
        let blob_writer = Arc::new(MockBlobWriter::new());
        let hasher = Arc::new(MockHasher);
        let thumbnail_repo = Arc::new(MockThumbnailRepo::new());
        let existing = ThumbnailMetadata::new(
            rep_id.clone(),
            BlobId::new(),
            MimeType("image/webp".to_string()),
            120,
            80,
            1024,
            Some(TimestampMs::from_epoch_millis(1)),
        );
        thumbnail_repo.insert_thumbnail(&existing).await?;
        let thumbnail_generator =
            Arc::new(MockThumbnailGenerator::new_success(GeneratedThumbnail {
                thumbnail_bytes: vec![8, 9, 10],
                thumbnail_mime_type: MimeType("image/webp".to_string()),
                original_width: 120,
                original_height: 80,
            }));
        let clock = Arc::new(FixedClock { now_ms: 123 });

        let (tx, rx) = mpsc::channel(4);
        let worker = BackgroundBlobWorker::new(
            rx,
            cache,
            spool,
            repo.clone(),
            blob_writer,
            hasher,
            thumbnail_repo.clone(),
            thumbnail_generator.clone(),
            clock,
            3,
            Duration::from_millis(1),
        );

        let handle = tokio::spawn(worker.run());
        tx.send(rep_id.clone()).await?;
        drop(tx);
        handle.await?;

        let thumbnail = thumbnail_repo
            .get(&rep_id)
            .await
            .expect("thumbnail missing");
        assert_eq!(thumbnail.thumbnail_blob_id, existing.thumbnail_blob_id);
        assert_eq!(thumbnail_generator.call_count().await, 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_worker_does_not_insert_thumbnail_on_generator_failure() -> Result<()> {
        let rep_id = RepresentationId::new();
        let rep = create_representation(&rep_id);

        let mut reps = HashMap::new();
        reps.insert(rep_id.clone(), rep);

        let repo = Arc::new(MockRepresentationRepo::new(reps));
        let cache = Arc::new(RepresentationCache::new(10, 10_000));
        cache.put(&rep_id, vec![1, 2, 3, 4]).await;
        let spool = Arc::new(SpoolManager::new(tempfile::tempdir()?.path(), 10_000)?);
        let blob_writer = Arc::new(MockBlobWriter::new());
        let hasher = Arc::new(MockHasher);
        let thumbnail_repo = Arc::new(MockThumbnailRepo::new());
        let thumbnail_generator = Arc::new(MockThumbnailGenerator::new_failure());
        let clock = Arc::new(FixedClock { now_ms: 123 });

        let (tx, rx) = mpsc::channel(4);
        let worker = BackgroundBlobWorker::new(
            rx,
            cache,
            spool,
            repo.clone(),
            blob_writer,
            hasher,
            thumbnail_repo.clone(),
            thumbnail_generator.clone(),
            clock,
            3,
            Duration::from_millis(1),
        );

        let handle = tokio::spawn(worker.run());
        tx.send(rep_id.clone()).await?;
        drop(tx);
        handle.await?;

        let thumbnail = thumbnail_repo.get(&rep_id).await;
        assert!(thumbnail.is_none());

        let updated = repo.get(&rep_id).await;
        let updated = updated.expect("representation missing");
        assert_eq!(updated.payload_state(), PayloadAvailability::BlobReady);
        assert_eq!(thumbnail_generator.call_count().await, 1);
        Ok(())
    }

    #[tokio::test]
    async fn test_worker_processes_staged_representations() -> Result<()> {
        let rep_id = RepresentationId::new();
        let rep = create_representation(&rep_id);

        let mut reps = HashMap::new();
        reps.insert(rep_id.clone(), rep);

        let repo = Arc::new(MockRepresentationRepo::new(reps));
        let cache = Arc::new(RepresentationCache::new(10, 10_000));
        cache.put(&rep_id, vec![1, 2, 3]).await;
        let spool = Arc::new(SpoolManager::new(tempfile::tempdir()?.path(), 10_000)?);
        let blob_writer = Arc::new(MockBlobWriter::new());
        let hasher = Arc::new(MockHasher);
        let (thumbnail_repo, thumbnail_generator, clock) = default_thumbnail_deps();

        let (tx, rx) = mpsc::channel(4);
        let worker = BackgroundBlobWorker::new(
            rx,
            cache,
            spool,
            repo.clone(),
            blob_writer,
            hasher,
            thumbnail_repo,
            thumbnail_generator,
            clock,
            3,
            Duration::from_millis(1),
        );

        let handle = tokio::spawn(worker.run());
        tx.send(rep_id.clone()).await?;
        drop(tx);
        handle.await?;

        let updated = repo.get(&rep_id).await;
        let updated = updated.expect("representation missing");
        assert_eq!(updated.payload_state(), PayloadAvailability::BlobReady);
        assert!(updated.blob_id.is_some());
        Ok(())
    }

    #[tokio::test]
    async fn test_worker_falls_back_to_spool() -> Result<()> {
        let rep_id = RepresentationId::new();
        let rep = create_representation(&rep_id);

        let mut reps = HashMap::new();
        reps.insert(rep_id.clone(), rep);

        let repo = Arc::new(MockRepresentationRepo::new(reps));
        let cache = Arc::new(RepresentationCache::new(10, 10_000));
        let temp_dir = tempfile::tempdir()?;
        let spool = Arc::new(SpoolManager::new(temp_dir.path(), 10_000)?);
        spool.write(&rep_id, &[9, 9, 9]).await?;

        let blob_writer = Arc::new(MockBlobWriter::new());
        let hasher = Arc::new(MockHasher);
        let (thumbnail_repo, thumbnail_generator, clock) = default_thumbnail_deps();

        let (tx, rx) = mpsc::channel(4);
        let worker = BackgroundBlobWorker::new(
            rx,
            cache,
            spool,
            repo.clone(),
            blob_writer,
            hasher,
            thumbnail_repo,
            thumbnail_generator,
            clock,
            3,
            Duration::from_millis(1),
        );

        let handle = tokio::spawn(worker.run());
        tx.send(rep_id.clone()).await?;
        drop(tx);
        handle.await?;

        let updated = repo.get(&rep_id).await;
        let updated = updated.expect("representation missing");
        assert_eq!(updated.payload_state(), PayloadAvailability::BlobReady);
        Ok(())
    }

    #[tokio::test]
    async fn test_worker_does_not_mark_lost_on_cache_miss() -> Result<()> {
        let rep_id = RepresentationId::new();
        let rep = create_representation(&rep_id);

        let mut reps = HashMap::new();
        reps.insert(rep_id.clone(), rep);

        let repo = Arc::new(MockRepresentationRepo::new(reps));
        let cache = Arc::new(RepresentationCache::new(10, 10_000));
        let spool = Arc::new(SpoolManager::new(tempfile::tempdir()?.path(), 10_000)?);
        let blob_writer = Arc::new(MockBlobWriter::new());
        let hasher = Arc::new(MockHasher);
        let (thumbnail_repo, thumbnail_generator, clock) = default_thumbnail_deps();

        let (tx, rx) = mpsc::channel(4);
        let worker = BackgroundBlobWorker::new(
            rx,
            cache,
            spool,
            repo.clone(),
            blob_writer,
            hasher,
            thumbnail_repo,
            thumbnail_generator,
            clock,
            3,
            Duration::from_millis(1),
        );

        let handle = tokio::spawn(worker.run());
        tx.send(rep_id.clone()).await?;
        drop(tx);
        handle.await?;

        let updated = repo.get(&rep_id).await;
        let updated = updated.expect("representation missing");
        assert_eq!(updated.payload_state(), PayloadAvailability::Staged);
        assert_eq!(
            updated.last_error.as_deref(),
            Some("cache/spool miss: bytes not available")
        );
        Ok(())
    }

    struct FlakyBlobWriter {
        attempts: TokioMutex<u32>,
    }

    impl FlakyBlobWriter {
        fn new() -> Self {
            Self {
                attempts: TokioMutex::new(0),
            }
        }
    }

    #[async_trait]
    impl BlobWriterPort for FlakyBlobWriter {
        async fn write_if_absent(
            &self,
            content_id: &ContentHash,
            encrypted_bytes: &[u8],
        ) -> Result<Blob> {
            let mut attempts = self.attempts.lock().await;
            *attempts += 1;
            if *attempts == 1 {
                return Err(anyhow::anyhow!("transient error"));
            }
            Ok(Blob::new(
                BlobId::new(),
                BlobStorageLocator::new_local_fs(PathBuf::from("/tmp/mock")),
                encrypted_bytes.len() as i64,
                content_id.clone(),
                0,
                None,
            ))
        }
    }

    #[tokio::test]
    async fn test_worker_retries_on_transient_error() -> Result<()> {
        let rep_id = RepresentationId::new();
        let rep = create_representation(&rep_id);

        let mut reps = HashMap::new();
        reps.insert(rep_id.clone(), rep);

        let repo = Arc::new(MockRepresentationRepo::new(reps));
        let cache = Arc::new(RepresentationCache::new(10, 10_000));
        cache.put(&rep_id, vec![7, 7, 7]).await;
        let spool = Arc::new(SpoolManager::new(tempfile::tempdir()?.path(), 10_000)?);
        let blob_writer = Arc::new(FlakyBlobWriter::new());
        let hasher = Arc::new(MockHasher);
        let (thumbnail_repo, thumbnail_generator, clock) = default_thumbnail_deps();

        let (tx, rx) = mpsc::channel(4);
        let worker = BackgroundBlobWorker::new(
            rx,
            cache,
            spool,
            repo.clone(),
            blob_writer,
            hasher,
            thumbnail_repo,
            thumbnail_generator,
            clock,
            2,
            Duration::from_millis(1),
        );

        let handle = tokio::spawn(worker.run());
        tx.send(rep_id.clone()).await?;
        drop(tx);
        handle.await?;

        let updated = repo.get(&rep_id).await;
        let updated = updated.expect("representation missing");
        assert_eq!(updated.payload_state(), PayloadAvailability::BlobReady);
        Ok(())
    }
}
