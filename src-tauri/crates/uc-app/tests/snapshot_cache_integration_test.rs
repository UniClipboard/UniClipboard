//! Snapshot cache integration tests.
//!
//! These tests exercise capture + cache/spool + background worker flow.

use std::collections::HashMap;
use std::io::Write;
use std::str::FromStr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Result};
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};

use tracing_subscriber::EnvFilter;
use uc_app::usecases::internal::capture_clipboard::CaptureClipboardUseCase;
use uc_core::clipboard::SelectRepresentationPolicyV1;
use uc_core::clipboard::{
    ClipboardEntry, ClipboardEvent, ClipboardSelectionDecision, ObservedClipboardRepresentation,
    PayloadAvailability, PersistedClipboardRepresentation, SystemClipboardSnapshot,
    ThumbnailMetadata,
};
use uc_core::ids::{EntryId, EventId, FormatId, RepresentationId};
use uc_core::ports::clipboard::{
    GeneratedThumbnail, ProcessingUpdateOutcome, RepresentationCachePort, SpoolQueuePort,
    ThumbnailGeneratorPort, ThumbnailRepositoryPort,
};
use uc_core::ports::BlobWriterPort;
use uc_core::ports::ClockPort;
use uc_core::ports::{
    ClipboardEntryRepositoryPort, ClipboardEventWriterPort, ClipboardRepresentationNormalizerPort,
    ClipboardRepresentationRepositoryPort, DeviceIdentityPort, SelectRepresentationPolicyPort,
};
use uc_core::DeviceId;
use uc_core::{Blob, BlobId, ContentHash, MimeType};
use uc_infra::clipboard::{
    BackgroundBlobWorker, ClipboardRepresentationNormalizer, MpscSpoolQueue, RepresentationCache,
    SpoolManager,
};
use uc_infra::config::ClipboardStorageConfig;
use uc_infra::security::Blake3Hasher;

#[derive(Clone)]
struct SharedLogBuffer {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedLogBuffer {
    type Writer = SharedLogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        SharedLogWriter {
            buffer: self.buffer.clone(),
        }
    }
}

struct SharedLogWriter {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl Write for SharedLogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut guard = self.buffer.lock().unwrap();
        guard.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

static LOG_BUFFER: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();

fn init_test_tracing() -> Arc<Mutex<Vec<u8>>> {
    LOG_BUFFER
        .get_or_init(|| {
            let buffer = Arc::new(Mutex::new(Vec::new()));
            let writer = SharedLogBuffer {
                buffer: buffer.clone(),
            };
            let subscriber = tracing_subscriber::fmt()
                .with_ansi(false)
                .with_env_filter(EnvFilter::new("warn"))
                .with_writer(writer)
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .expect("set global tracing subscriber");
            buffer
        })
        .clone()
}

struct InMemoryDeviceIdentity;

impl DeviceIdentityPort for InMemoryDeviceIdentity {
    fn current_device_id(&self) -> DeviceId {
        DeviceId::new("device-test")
    }
}

struct InMemoryThumbnailRepo {
    thumbnails: Mutex<HashMap<RepresentationId, ThumbnailMetadata>>,
}

impl InMemoryThumbnailRepo {
    fn new() -> Self {
        Self {
            thumbnails: Mutex::new(HashMap::new()),
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
}

#[async_trait::async_trait]
impl ThumbnailRepositoryPort for InMemoryThumbnailRepo {
    async fn get_by_representation_id(
        &self,
        representation_id: &RepresentationId,
    ) -> Result<Option<ThumbnailMetadata>> {
        Ok(self
            .thumbnails
            .lock()
            .unwrap()
            .get(representation_id)
            .map(Self::clone_metadata))
    }

    async fn insert_thumbnail(&self, metadata: &ThumbnailMetadata) -> Result<()> {
        self.thumbnails.lock().unwrap().insert(
            metadata.representation_id.clone(),
            Self::clone_metadata(metadata),
        );
        Ok(())
    }
}

struct NoopThumbnailGenerator;

#[async_trait::async_trait]
impl ThumbnailGeneratorPort for NoopThumbnailGenerator {
    async fn generate_thumbnail(&self, _image_bytes: &[u8]) -> Result<GeneratedThumbnail> {
        Ok(GeneratedThumbnail {
            thumbnail_bytes: vec![1],
            thumbnail_mime_type: MimeType("image/webp".to_string()),
            original_width: 1,
            original_height: 1,
        })
    }

    async fn generate_thumbnail_from_rgba(
        &self,
        _rgba_bytes: &[u8],
        _width: u32,
        _height: u32,
    ) -> Result<GeneratedThumbnail> {
        self.generate_thumbnail(&[]).await
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

#[derive(Default)]
struct InMemoryEntryRepo {
    entries: Mutex<HashMap<EntryId, ClipboardEntry>>,
    selections: Mutex<HashMap<EntryId, ClipboardSelectionDecision>>,
}

#[async_trait::async_trait]
impl ClipboardEntryRepositoryPort for InMemoryEntryRepo {
    async fn save_entry_and_selection(
        &self,
        entry: &ClipboardEntry,
        selection: &ClipboardSelectionDecision,
    ) -> Result<()> {
        self.entries
            .lock()
            .unwrap()
            .insert(entry.entry_id.clone(), entry.clone());
        self.selections
            .lock()
            .unwrap()
            .insert(entry.entry_id.clone(), selection.clone());
        Ok(())
    }

    async fn get_entry(&self, entry_id: &EntryId) -> Result<Option<ClipboardEntry>> {
        Ok(self.entries.lock().unwrap().get(entry_id).cloned())
    }

    async fn list_entries(&self, _limit: usize, _offset: usize) -> Result<Vec<ClipboardEntry>> {
        Ok(self.entries.lock().unwrap().values().cloned().collect())
    }

    async fn delete_entry(&self, entry_id: &EntryId) -> Result<()> {
        self.entries.lock().unwrap().remove(entry_id);
        self.selections.lock().unwrap().remove(entry_id);
        Ok(())
    }
}

#[derive(Default)]
struct InMemoryRepresentationRepo {
    representations: Mutex<HashMap<RepresentationId, PersistedClipboardRepresentation>>,
}

impl InMemoryRepresentationRepo {
    fn insert_all(&self, reps: &[PersistedClipboardRepresentation]) {
        let mut guard = self.representations.lock().unwrap();
        for rep in reps {
            guard.insert(rep.id.clone(), rep.clone());
        }
    }

    fn get_by_id(&self, rep_id: &RepresentationId) -> Option<PersistedClipboardRepresentation> {
        self.representations.lock().unwrap().get(rep_id).cloned()
    }
}

#[async_trait::async_trait]
impl ClipboardRepresentationRepositoryPort for InMemoryRepresentationRepo {
    async fn get_representation(
        &self,
        _event_id: &EventId,
        representation_id: &RepresentationId,
    ) -> Result<Option<PersistedClipboardRepresentation>> {
        Ok(self.get_by_id(representation_id))
    }

    async fn get_representation_by_id(
        &self,
        representation_id: &RepresentationId,
    ) -> Result<Option<PersistedClipboardRepresentation>> {
        Ok(self.get_by_id(representation_id))
    }

    async fn get_representation_by_blob_id(
        &self,
        _blob_id: &BlobId,
    ) -> Result<Option<PersistedClipboardRepresentation>> {
        Ok(None)
    }

    async fn update_blob_id(
        &self,
        representation_id: &RepresentationId,
        blob_id: &BlobId,
    ) -> Result<()> {
        let mut guard = self.representations.lock().unwrap();
        if let Some(rep) = guard.get_mut(representation_id) {
            rep.blob_id = Some(blob_id.clone());
        }
        Ok(())
    }

    async fn update_blob_id_if_none(
        &self,
        representation_id: &RepresentationId,
        blob_id: &BlobId,
    ) -> Result<bool> {
        let mut guard = self.representations.lock().unwrap();
        if let Some(rep) = guard.get_mut(representation_id) {
            if rep.blob_id.is_none() {
                rep.blob_id = Some(blob_id.clone());
                return Ok(true);
            }
        }
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
        let mut guard = self.representations.lock().unwrap();
        let rep = match guard.get_mut(rep_id) {
            Some(rep) => rep,
            None => return Ok(ProcessingUpdateOutcome::NotFound),
        };

        if !expected_states.contains(&rep.payload_state) {
            return Ok(ProcessingUpdateOutcome::StateMismatch);
        }

        rep.payload_state = new_state;
        rep.blob_id = blob_id.cloned();
        rep.last_error = last_error.map(|s| s.to_string());

        Ok(ProcessingUpdateOutcome::Updated(rep.clone()))
    }
}

struct InMemoryEventWriter {
    rep_repo: Arc<InMemoryRepresentationRepo>,
}

#[async_trait::async_trait]
impl ClipboardEventWriterPort for InMemoryEventWriter {
    async fn insert_event(
        &self,
        _event: &ClipboardEvent,
        representations: &Vec<PersistedClipboardRepresentation>,
    ) -> Result<()> {
        self.rep_repo.insert_all(representations);
        Ok(())
    }

    async fn delete_event_and_representations(&self, _event_id: &EventId) -> Result<()> {
        Ok(())
    }
}

struct InMemoryBlobWriter {
    blobs: Mutex<HashMap<ContentHash, Blob>>,
}

impl InMemoryBlobWriter {
    fn new() -> Self {
        Self {
            blobs: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl BlobWriterPort for InMemoryBlobWriter {
    async fn write_if_absent(
        &self,
        content_id: &ContentHash,
        plaintext_bytes: &[u8],
    ) -> Result<Blob> {
        let mut guard = self.blobs.lock().unwrap();
        if let Some(existing) = guard.get(content_id) {
            return Ok(existing.clone());
        }

        let blob_id = BlobId::new();
        let locator = uc_core::blob::BlobStorageLocator::new_local_fs(std::path::PathBuf::from(
            format!("/tmp/blob/{}", blob_id),
        ));
        let blob = Blob::new(
            blob_id,
            locator,
            plaintext_bytes.len() as i64,
            content_id.clone(),
            0,
            None,
        );
        guard.insert(content_id.clone(), blob.clone());
        Ok(blob)
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn build_snapshot(rep_id: RepresentationId, bytes: Vec<u8>, mime: &str) -> SystemClipboardSnapshot {
    SystemClipboardSnapshot {
        ts_ms: now_ms(),
        representations: vec![ObservedClipboardRepresentation::new(
            rep_id,
            FormatId::from(mime),
            Some(MimeType::from_str(mime).unwrap()),
            bytes,
        )],
    }
}

#[tokio::test]
async fn test_capture_succeeds_even_when_spool_queue_closed() -> Result<()> {
    // Spool failures are non-fatal: the entry must still be created so the user
    // sees their clipboard content immediately. Bytes are kept in the in-memory
    // cache for the background blob worker to use.
    let rep_id = RepresentationId::new();
    let bytes = vec![0u8; 32 * 1024];
    let snapshot = build_snapshot(rep_id.clone(), bytes.clone(), "image/png");

    let config = Arc::new(ClipboardStorageConfig {
        inline_threshold_bytes: 16,
        ..ClipboardStorageConfig::defaults()
    });
    let normalizer: Arc<dyn ClipboardRepresentationNormalizerPort> =
        Arc::new(ClipboardRepresentationNormalizer::new(config));

    let rep_cache = Arc::new(RepresentationCache::new(10, 1_000_000));
    let rep_cache_port: Arc<dyn RepresentationCachePort> = rep_cache.clone();
    let rep_repo = Arc::new(InMemoryRepresentationRepo::default());
    let event_writer: Arc<dyn ClipboardEventWriterPort> =
        Arc::new(InMemoryEventWriter { rep_repo });
    let entry_repo: Arc<dyn ClipboardEntryRepositoryPort> = Arc::new(InMemoryEntryRepo::default());
    let policy: Arc<dyn SelectRepresentationPolicyPort> =
        Arc::new(SelectRepresentationPolicyV1::new());

    let (spool_tx, spool_rx) = mpsc::channel(1);
    let (worker_tx, _worker_rx) = mpsc::channel(1);
    drop(spool_rx);
    worker_tx.try_send(RepresentationId::new())?;
    let spool_queue: Arc<dyn SpoolQueuePort> = Arc::new(MpscSpoolQueue::new(spool_tx));

    let usecase = CaptureClipboardUseCase::new(
        entry_repo,
        event_writer,
        policy,
        normalizer,
        Arc::new(InMemoryDeviceIdentity),
        rep_cache_port,
        spool_queue,
    );
    let result = timeout(Duration::from_millis(200), usecase.execute(snapshot)).await?;
    assert!(
        result.is_ok(),
        "capture must succeed even when spool queue is closed"
    );

    // Bytes must still be in the in-memory cache for the blob worker.
    let cached = rep_cache.get(&rep_id).await;
    assert_eq!(cached, Some(bytes));
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn test_capture_logs_on_spool_queue_closed() -> Result<()> {
    // When the spool queue is closed, capture must still succeed (non-fatal)
    // but must log a warning containing the representation ID.
    let rep_id = RepresentationId::new();
    let bytes = vec![0u8; 32 * 1024];
    let snapshot = build_snapshot(rep_id.clone(), bytes, "image/png");

    let log_buffer = init_test_tracing();
    let start_len = log_buffer.lock().unwrap().len();

    let config = Arc::new(ClipboardStorageConfig {
        inline_threshold_bytes: 16,
        ..ClipboardStorageConfig::defaults()
    });
    let normalizer: Arc<dyn ClipboardRepresentationNormalizerPort> =
        Arc::new(ClipboardRepresentationNormalizer::new(config));

    let rep_cache = Arc::new(RepresentationCache::new(10, 1_000_000));
    let rep_cache_port: Arc<dyn RepresentationCachePort> = rep_cache.clone();
    let rep_repo = Arc::new(InMemoryRepresentationRepo::default());
    let event_writer: Arc<dyn ClipboardEventWriterPort> =
        Arc::new(InMemoryEventWriter { rep_repo });
    let entry_repo: Arc<dyn ClipboardEntryRepositoryPort> = Arc::new(InMemoryEntryRepo::default());
    let policy: Arc<dyn SelectRepresentationPolicyPort> =
        Arc::new(SelectRepresentationPolicyV1::new());

    let (spool_tx, spool_rx) = mpsc::channel(1);
    drop(spool_rx);
    let spool_queue: Arc<dyn SpoolQueuePort> = Arc::new(MpscSpoolQueue::new(spool_tx));

    let usecase = CaptureClipboardUseCase::new(
        entry_repo,
        event_writer,
        policy,
        normalizer,
        Arc::new(InMemoryDeviceIdentity),
        rep_cache_port,
        spool_queue,
    );
    let result = usecase.execute(snapshot).await;
    assert!(
        result.is_ok(),
        "capture must succeed even when spool queue is closed"
    );

    // Spool writes are spawned as a background task; yield to let it run and log.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let guard = log_buffer.lock().unwrap();
    let (_, new_bytes) = guard.split_at(start_len);
    let output = String::from_utf8_lossy(new_bytes);
    let rep_id_str = rep_id.to_string();
    assert!(
        output.contains("Failed to enqueue spool request") && output.contains(&rep_id_str),
        "log output: {output}"
    );
    Ok(())
}

#[tokio::test]
async fn test_worker_reverts_to_staged_on_cache_spool_miss() -> Result<()> {
    let rep_id = RepresentationId::new();
    let rep_repo = Arc::new(InMemoryRepresentationRepo::default());
    let rep = PersistedClipboardRepresentation::new_with_state(
        rep_id.clone(),
        FormatId::new(),
        Some(MimeType::from_str("image/png").unwrap()),
        1024,
        None,
        None,
        PayloadAvailability::Processing,
        None,
    )?;
    rep_repo.insert_all(&[rep]);

    let rep_cache = Arc::new(RepresentationCache::new(10, 1024));
    let spool_dir = tempfile::tempdir()?;
    let spool = Arc::new(SpoolManager::new(spool_dir.path(), 1_000_000)?);
    let (worker_tx, worker_rx) = mpsc::channel(4);
    let thumbnail_repo: Arc<dyn ThumbnailRepositoryPort> = Arc::new(InMemoryThumbnailRepo::new());
    let thumbnail_generator: Arc<dyn ThumbnailGeneratorPort> = Arc::new(NoopThumbnailGenerator);
    let clock: Arc<dyn ClockPort> = Arc::new(FixedClock { now_ms: 1 });

    let worker = BackgroundBlobWorker::new(
        worker_rx,
        rep_cache,
        spool,
        rep_repo.clone(),
        Arc::new(InMemoryBlobWriter::new()),
        Arc::new(Blake3Hasher),
        thumbnail_repo,
        thumbnail_generator,
        clock,
        1,
        Duration::from_millis(10),
    );
    let handle = tokio::spawn(async move {
        worker.run().await;
    });

    worker_tx.send(rep_id.clone()).await?;
    drop(worker_tx);

    let deadline = Duration::from_secs(1);
    let mut elapsed = Duration::from_millis(0);
    let step = Duration::from_millis(20);
    loop {
        if let Some(rep) = rep_repo.get_by_id(&rep_id) {
            if rep.payload_state == PayloadAvailability::Staged {
                assert_eq!(
                    rep.last_error.as_deref(),
                    Some("cache/spool miss: bytes not available")
                );
                break;
            }
        }
        if elapsed >= deadline {
            return Err(anyhow!("timed out waiting for miss revert to Staged"));
        }
        sleep(step).await;
        elapsed += step;
    }

    handle.await?;
    Ok(())
}

#[tokio::test]
async fn test_worker_materializes_after_spool_eviction_with_cache_hit() -> Result<()> {
    let config = Arc::new(ClipboardStorageConfig {
        inline_threshold_bytes: 1,
        ..ClipboardStorageConfig::defaults()
    });
    let normalizer: Arc<dyn ClipboardRepresentationNormalizerPort> =
        Arc::new(ClipboardRepresentationNormalizer::new(config));

    let rep_cache = Arc::new(RepresentationCache::new(10, 1024));
    let rep_cache_port: Arc<dyn RepresentationCachePort> = rep_cache.clone();
    let rep_repo = Arc::new(InMemoryRepresentationRepo::default());
    let event_writer: Arc<dyn ClipboardEventWriterPort> = Arc::new(InMemoryEventWriter {
        rep_repo: rep_repo.clone(),
    });
    let entry_repo: Arc<dyn ClipboardEntryRepositoryPort> = Arc::new(InMemoryEntryRepo::default());
    let policy: Arc<dyn SelectRepresentationPolicyPort> =
        Arc::new(SelectRepresentationPolicyV1::new());

    let spool_dir = tempfile::tempdir()?;
    let spool_root = spool_dir.path().to_path_buf();
    let spool = Arc::new(SpoolManager::new(&spool_root, 4)?);
    let (spool_tx, spool_rx) = mpsc::channel(8);
    let spool_queue: Arc<dyn SpoolQueuePort> = Arc::new(MpscSpoolQueue::new(spool_tx));
    let (worker_tx, worker_rx) = mpsc::channel(8);
    let thumbnail_repo: Arc<dyn ThumbnailRepositoryPort> = Arc::new(InMemoryThumbnailRepo::new());
    let thumbnail_generator: Arc<dyn ThumbnailGeneratorPort> = Arc::new(NoopThumbnailGenerator);
    let clock: Arc<dyn ClockPort> = Arc::new(FixedClock { now_ms: 1 });

    let spooler = uc_infra::clipboard::SpoolerTask::new(
        spool_rx,
        spool.clone(),
        worker_tx.clone(),
        rep_cache.clone(),
    );
    let spooler_handle = tokio::spawn(async move {
        spooler.run().await;
    });

    let usecase = CaptureClipboardUseCase::new(
        entry_repo,
        event_writer,
        policy,
        normalizer,
        Arc::new(InMemoryDeviceIdentity),
        rep_cache_port,
        spool_queue.clone(),
    );

    let rep_id_a = RepresentationId::new();
    let rep_id_b = RepresentationId::new();
    usecase
        .execute(build_snapshot(rep_id_a.clone(), vec![1, 2, 3], "image/png"))
        .await?;
    usecase
        .execute(build_snapshot(rep_id_b.clone(), vec![4, 5, 6], "image/png"))
        .await?;

    drop(usecase);
    drop(spool_queue);
    spooler_handle.await?;

    let spool_path_a = spool_root.join(rep_id_a.to_string());
    let spool_path_b = spool_root.join(rep_id_b.to_string());
    if spool_path_a.exists() && spool_path_b.exists() {
        return Err(anyhow!("expected at least one spool file to be evicted"));
    }

    let blob_writer = Arc::new(InMemoryBlobWriter::new());
    let worker = BackgroundBlobWorker::new(
        worker_rx,
        rep_cache,
        spool,
        rep_repo.clone(),
        blob_writer,
        Arc::new(Blake3Hasher),
        thumbnail_repo,
        thumbnail_generator,
        clock,
        2,
        Duration::from_millis(50),
    );
    let worker_handle = tokio::spawn(async move {
        worker.run().await;
    });
    drop(worker_tx);

    let deadline = Duration::from_secs(2);
    let mut elapsed = Duration::from_millis(0);
    let step = Duration::from_millis(20);
    loop {
        let ready_a = rep_repo
            .get_by_id(&rep_id_a)
            .map(|rep| rep.payload_state == PayloadAvailability::BlobReady && rep.blob_id.is_some())
            .unwrap_or(false);
        let ready_b = rep_repo
            .get_by_id(&rep_id_b)
            .map(|rep| rep.payload_state == PayloadAvailability::BlobReady && rep.blob_id.is_some())
            .unwrap_or(false);
        if ready_a && ready_b {
            break;
        }
        if elapsed >= deadline {
            return Err(anyhow!("timed out waiting for blob materialization"));
        }
        sleep(step).await;
        elapsed += step;
    }

    worker_handle.await?;
    Ok(())
}

#[tokio::test]
async fn test_worker_materializes_blob_from_cache() -> Result<()> {
    let rep_id = RepresentationId::new();
    let bytes = vec![7u8; 64 * 1024];
    let snapshot = build_snapshot(rep_id.clone(), bytes.clone(), "image/png");

    let config = Arc::new(ClipboardStorageConfig {
        inline_threshold_bytes: 16,
        ..ClipboardStorageConfig::defaults()
    });
    let normalizer: Arc<dyn ClipboardRepresentationNormalizerPort> =
        Arc::new(ClipboardRepresentationNormalizer::new(config));

    let rep_cache = Arc::new(RepresentationCache::new(10, 1_000_000));
    let rep_cache_port: Arc<dyn RepresentationCachePort> = rep_cache.clone();
    let rep_repo = Arc::new(InMemoryRepresentationRepo::default());
    let event_writer: Arc<dyn ClipboardEventWriterPort> = Arc::new(InMemoryEventWriter {
        rep_repo: rep_repo.clone(),
    });
    let entry_repo: Arc<dyn ClipboardEntryRepositoryPort> = Arc::new(InMemoryEntryRepo::default());
    let policy: Arc<dyn SelectRepresentationPolicyPort> =
        Arc::new(SelectRepresentationPolicyV1::new());

    let spool_dir = tempfile::tempdir()?;
    let spool = Arc::new(SpoolManager::new(spool_dir.path(), 1_000_000)?);
    let (spool_tx, spool_rx) = mpsc::channel(8);
    let spool_queue: Arc<dyn SpoolQueuePort> = Arc::new(MpscSpoolQueue::new(spool_tx));
    let (worker_tx, worker_rx) = mpsc::channel(8);
    let thumbnail_repo: Arc<dyn ThumbnailRepositoryPort> = Arc::new(InMemoryThumbnailRepo::new());
    let thumbnail_generator: Arc<dyn ThumbnailGeneratorPort> = Arc::new(NoopThumbnailGenerator);
    let clock: Arc<dyn ClockPort> = Arc::new(FixedClock { now_ms: 1 });

    let spooler = uc_infra::clipboard::SpoolerTask::new(
        spool_rx,
        spool.clone(),
        worker_tx.clone(),
        rep_cache.clone(),
    );
    tokio::spawn(async move {
        spooler.run().await;
    });

    let worker = BackgroundBlobWorker::new(
        worker_rx,
        rep_cache.clone(),
        spool.clone(),
        rep_repo.clone(),
        Arc::new(InMemoryBlobWriter::new()),
        Arc::new(Blake3Hasher),
        thumbnail_repo,
        thumbnail_generator,
        clock,
        3,
        Duration::from_millis(50),
    );
    tokio::spawn(async move {
        worker.run().await;
    });

    let usecase = CaptureClipboardUseCase::new(
        entry_repo,
        event_writer,
        policy,
        normalizer,
        Arc::new(InMemoryDeviceIdentity),
        rep_cache_port,
        spool_queue,
    );

    usecase.execute(snapshot).await?;

    let deadline = Duration::from_secs(2);
    let mut elapsed = Duration::from_millis(0);
    let step = Duration::from_millis(20);
    loop {
        if let Some(rep) = rep_repo.get_by_id(&rep_id) {
            if rep.payload_state == PayloadAvailability::BlobReady && rep.blob_id.is_some() {
                break;
            }
        }
        if elapsed >= deadline {
            return Err(anyhow!("timed out waiting for blob materialization"));
        }
        sleep(step).await;
        elapsed += step;
    }

    Ok(())
}

#[tokio::test]
#[ignore = "Requires SpoolScanner recovery flow (Task 14)"]
async fn test_spool_recovers_after_restart() -> Result<()> {
    // TODO(Task 14): Add SpoolScanner and recovery assertions.
    Ok(())
}
