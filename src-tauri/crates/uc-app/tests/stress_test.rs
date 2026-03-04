//! Stress tests for snapshot cache + worker pipeline.

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{anyhow, Result};
use tokio::sync::mpsc;
use tokio::time::sleep;

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

static TRACE_INIT: Once = Once::new();

fn init_tracing() {
    TRACE_INIT.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_test_writer()
            .try_init();
    });
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

    fn count(&self) -> usize {
        self.blobs.lock().unwrap().len()
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
        representations: vec![ObservedClipboardRepresentation {
            id: rep_id,
            format_id: FormatId::from(mime),
            mime: Some(MimeType::from_str(mime).unwrap()),
            bytes,
        }],
    }
}

#[tokio::test]
#[ignore] // Run with: cargo test -- --ignored
async fn stress_test_100_large_images() -> Result<()> {
    init_tracing();
    let config = Arc::new(ClipboardStorageConfig {
        inline_threshold_bytes: 16,
        ..ClipboardStorageConfig::defaults()
    });
    let normalizer: Arc<dyn ClipboardRepresentationNormalizerPort> =
        Arc::new(ClipboardRepresentationNormalizer::new(config));

    let rep_cache = Arc::new(RepresentationCache::new(10, 20 * 1024 * 1024));
    let rep_cache_port: Arc<dyn RepresentationCachePort> = rep_cache.clone();
    let rep_repo = Arc::new(InMemoryRepresentationRepo::default());
    let event_writer: Arc<dyn ClipboardEventWriterPort> = Arc::new(InMemoryEventWriter {
        rep_repo: rep_repo.clone(),
    });
    let entry_repo: Arc<dyn ClipboardEntryRepositoryPort> = Arc::new(InMemoryEntryRepo::default());
    let policy: Arc<dyn SelectRepresentationPolicyPort> =
        Arc::new(SelectRepresentationPolicyV1::new());

    let spool_dir = tempfile::tempdir()?;
    let spool_root: PathBuf = spool_dir.path().to_path_buf();
    let spool = Arc::new(SpoolManager::new(&spool_root, 1_000_000_000)?);
    let (spool_tx, spool_rx) = mpsc::channel(256);
    let spool_queue: Arc<dyn SpoolQueuePort> = Arc::new(MpscSpoolQueue::new(spool_tx));
    let (worker_tx, worker_rx) = mpsc::channel(256);

    let spooler = uc_infra::clipboard::SpoolerTask::new(
        spool_rx,
        spool.clone(),
        worker_tx.clone(),
        rep_cache.clone(),
    );
    tokio::spawn(async move {
        spooler.run().await;
    });

    let blob_writer = Arc::new(InMemoryBlobWriter::new());
    let thumbnail_repo: Arc<dyn ThumbnailRepositoryPort> = Arc::new(InMemoryThumbnailRepo::new());
    let thumbnail_generator: Arc<dyn ThumbnailGeneratorPort> = Arc::new(NoopThumbnailGenerator);
    let clock: Arc<dyn ClockPort> = Arc::new(FixedClock { now_ms: 1 });
    let worker = BackgroundBlobWorker::new(
        worker_rx,
        rep_cache.clone(),
        spool.clone(),
        rep_repo.clone(),
        blob_writer.clone(),
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

    let mut rep_ids = Vec::new();
    let mut snapshots = Vec::new();
    for idx in 0..100u8 {
        let rep_id = RepresentationId::new();
        let bytes = vec![idx; 5 * 1024];
        snapshots.push(build_snapshot(rep_id.clone(), bytes, "image/png"));
        rep_ids.push(rep_id);
    }

    let start = Instant::now();
    let mut total_capture_ms: u128 = 0;
    for (idx, snapshot) in snapshots.into_iter().enumerate() {
        let capture_start = Instant::now();
        usecase.execute(snapshot).await?;
        let capture_elapsed = capture_start.elapsed();
        total_capture_ms = total_capture_ms.saturating_add(capture_elapsed.as_millis());
        if (idx + 1) % 5 == 0 {
            println!(
                "capture progress: {}/100, last={:?}, avg={}ms",
                idx + 1,
                capture_elapsed,
                total_capture_ms / (idx as u128 + 1)
            );
        }
    }
    let elapsed = start.elapsed();
    println!("capture total elapsed: {:?}", elapsed);

    // Capture includes snapshot_hash over raw bytes, so it scales with payload size.
    assert!(
        elapsed < Duration::from_millis(8_000),
        "capture took too long: {elapsed:?}"
    );

    let wait_start = Instant::now();
    let mut last_log = Instant::now();
    let deadline = Duration::from_secs(10);
    loop {
        let mut ready = 0;
        let mut staged = 0;
        let mut processing = 0;
        let mut failed = 0;
        let mut lost = 0;
        for rep_id in &rep_ids {
            if let Some(rep) = rep_repo.get_by_id(rep_id) {
                if rep.payload_state == PayloadAvailability::BlobReady && rep.blob_id.is_some() {
                    ready += 1;
                } else {
                    match rep.payload_state {
                        PayloadAvailability::Staged => staged += 1,
                        PayloadAvailability::Processing => processing += 1,
                        PayloadAvailability::Failed { .. } => failed += 1,
                        PayloadAvailability::Lost => lost += 1,
                        _ => {}
                    }
                }
            }
        }

        if last_log.elapsed() >= Duration::from_secs(1) {
            let spool_files = std::fs::read_dir(&spool_root)
                .map(|entries| entries.count())
                .unwrap_or(0);
            println!(
                "materialize progress: ready={ready}, staged={staged}, processing={processing}, failed={failed}, lost={lost}, spool_files={spool_files}"
            );
            let mut sampled = 0;
            for rep_id in &rep_ids {
                if sampled >= 3 {
                    break;
                }
                if let Some(rep) = rep_repo.get_by_id(rep_id) {
                    if rep.payload_state == PayloadAvailability::Lost {
                        let cache_hit = rep_cache.get(rep_id).await.is_some();
                        let spool_path = spool_root.join(rep_id.to_string());
                        let spool_exists = spool_path.exists();
                        println!(
                            "lost sample: rep_id={rep_id}, cache_hit={cache_hit}, spool_exists={spool_exists}"
                        );
                        sampled += 1;
                    }
                }
            }
            last_log = Instant::now();
        }

        if ready == rep_ids.len() {
            break;
        }
        if wait_start.elapsed() >= deadline {
            return Err(anyhow!(
                "timed out waiting for blob materialization: {ready}/{}",
                rep_ids.len()
            ));
        }
        sleep(Duration::from_millis(50)).await;
    }

    assert_eq!(blob_writer.count(), rep_ids.len(), "blob count mismatch");

    let mut lost = 0;
    for rep_id in &rep_ids {
        if let Some(rep) = rep_repo.get_by_id(rep_id) {
            if rep.payload_state == PayloadAvailability::Lost {
                lost += 1;
            }
        }
    }
    assert_eq!(lost, 0, "representations unexpectedly marked Lost");

    let mut evicted = 0;
    for rep_id in rep_ids.iter().take(10) {
        if rep_cache.get(rep_id).await.is_none() {
            evicted += 1;
        }
    }
    assert!(evicted > 0, "cache did not evict any early entries");

    Ok(())
}
