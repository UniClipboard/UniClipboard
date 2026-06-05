//! Background worker to materialize blobs from staged representations.
//! 从暂存表示中异步生成 blob 的后台工作者。

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{debug, error, info_span, warn, Instrument};
use uc_core::clipboard::{MimeType, PayloadAvailability};
use uc_core::clipboard::{ThumbnailMetadata, TimestampMs};
use uc_core::ids::RepresentationId;
use uc_core::ports::clipboard::{
    ProcessingUpdateOutcome, ThumbnailGeneratorPort, ThumbnailRepositoryPort,
};
use uc_core::ports::{ClipboardRepresentationRepositoryPort, ClockPort, ContentHashPort};

use crate::blob::BlobWriterPort;
use crate::clipboard::{RepresentationCache, SpoolManager};

/// Check if an image MIME type needs conversion to PNG before blob storage.
/// Returns true for image/* types that are not already PNG or WebP.
fn should_convert_to_png(mime: Option<&MimeType>) -> bool {
    match mime {
        Some(m) => {
            let s = m.as_str();
            s.starts_with("image/") && s != "image/png" && s != "image/webp"
        }
        None => false,
    }
}

/// Result of converting an image to PNG, including pre-decoded RGBA pixels
/// to avoid re-decoding for thumbnail generation.
struct ConvertedImage {
    png_bytes: Vec<u8>,
    rgba_bytes: Vec<u8>,
    width: u32,
    height: u32,
}

/// Convert image bytes (any format supported by the `image` crate) to PNG.
///
/// Uses fast compression because the blob store applies zstd compression on top,
/// making aggressive PNG compression redundant. This significantly reduces encoding time
/// for large images (e.g. 34MB TIFF → PNG drops from ~2s to ~0.5s).
///
/// Returns both PNG bytes and pre-decoded RGBA pixels to avoid re-decoding for thumbnails.
fn convert_image_to_png(image_bytes: &[u8]) -> Result<ConvertedImage> {
    let img = image::load_from_memory(image_bytes).context("decode image for PNG conversion")?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();

    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new_with_quality(
        std::io::Cursor::new(&mut png_bytes),
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Sub,
    );
    img.write_with_encoder(encoder)
        .context("encode image as PNG")?;

    Ok(ConvertedImage {
        png_bytes,
        rgba_bytes: rgba.into_raw(),
        width,
        height,
    })
}

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
            let result = self
                .process_with_retry(rep_id.clone())
                .instrument(span)
                .await;
            // Return the in-memory cache copy regardless of terminal outcome.
            // The cache is only an accelerator for the Staged/Processing window;
            // once the worker is done (blob materialized, failed after retries,
            // or bytes missing) the spool is the source of truth for any bytes
            // still needed. Dropping it here keeps daemon memory flat under a
            // stream of image copies instead of growing until the cache hits
            // its byte ceiling. Retries inside `process_with_retry` still get a
            // cache hit because removal only happens after the loop exits.
            self.cache.remove(&rep_id).await;
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

        // Check if this representation needs format conversion (e.g. TIFF -> PNG).
        // Fetch the representation metadata to read its MIME type.
        let rep_meta = self.repo.get_representation_by_id(rep_id).await?;
        let mime = rep_meta.as_ref().and_then(|r| r.mime_type.as_ref());
        let needs_conversion = should_convert_to_png(mime);

        // Pre-decoded RGBA pixels from format conversion, used to skip
        // re-decoding in thumbnail generation.
        let mut pre_decoded_rgba: Option<(Vec<u8>, u32, u32)> = None;

        let (blob_bytes, mime_updated) = if needs_conversion {
            let original_mime = mime.map(|m| m.as_str()).unwrap_or("unknown");
            let original_size = raw_bytes.len();
            match convert_image_to_png(&raw_bytes) {
                Ok(converted) => {
                    debug!(
                        representation_id = %rep_id,
                        original_mime = %original_mime,
                        original_size = original_size,
                        converted_size = converted.png_bytes.len(),
                        "Converted image to PNG for blob storage"
                    );
                    pre_decoded_rgba =
                        Some((converted.rgba_bytes, converted.width, converted.height));
                    (converted.png_bytes, true)
                }
                Err(err) => {
                    warn!(
                        representation_id = %rep_id,
                        original_mime = %original_mime,
                        error = %err,
                        "Failed to convert image to PNG; storing original bytes"
                    );
                    (raw_bytes, false)
                }
            }
        } else {
            (raw_bytes, false)
        };

        let content_hash = self
            .hasher
            .hash_bytes(&blob_bytes)
            .context("failed to hash representation bytes")?;

        // BlobWriterPort should handle deduplication; data is passed as-is.
        let blob_id = self
            .blob_writer
            .write_if_absent(&content_hash, &blob_bytes)
            .await
            .context("failed to write blob")?;

        // Update the representation MIME type in DB if conversion occurred.
        if mime_updated {
            if let Err(err) = self
                .repo
                .update_mime_type(rep_id, &MimeType("image/png".to_string()))
                .await
            {
                warn!(
                    representation_id = %rep_id,
                    error = %err,
                    "Failed to update MIME type to image/png after conversion"
                );
            }
        }

        let updated = self
            .repo
            .update_processing_result(
                rep_id,
                &[PayloadAvailability::Processing],
                Some(&blob_id),
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
                self.try_generate_thumbnail(rep_id, &blob_bytes, pre_decoded_rgba)
                    .await;
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

    async fn try_generate_thumbnail(
        &self,
        rep_id: &RepresentationId,
        raw_bytes: &[u8],
        pre_decoded_rgba: Option<(Vec<u8>, u32, u32)>,
    ) {
        if let Err(err) = self
            .generate_thumbnail(rep_id, raw_bytes, pre_decoded_rgba)
            .await
        {
            error!(
                representation_id = %rep_id,
                error = %err,
                "Failed to generate thumbnail"
            );
        }
    }

    async fn generate_thumbnail(
        &self,
        rep_id: &RepresentationId,
        raw_bytes: &[u8],
        pre_decoded_rgba: Option<(Vec<u8>, u32, u32)>,
    ) -> Result<()> {
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

        let generated = if let Some((rgba, width, height)) = pre_decoded_rgba {
            self.thumbnail_generator
                .generate_thumbnail_from_rgba(&rgba, width, height)
                .await
                .context("failed to generate thumbnail from pre-decoded RGBA")?
        } else {
            self.thumbnail_generator
                .generate_thumbnail(raw_bytes)
                .await
                .context("failed to generate thumbnail")?
        };

        let thumbnail_hash = self
            .hasher
            .hash_bytes(&generated.thumbnail_bytes)
            .context("failed to hash thumbnail bytes")?;

        let thumbnail_blob_id = self
            .blob_writer
            .write_if_absent(&thumbnail_hash, &generated.thumbnail_bytes)
            .await
            .context("failed to write thumbnail blob")?;

        let created_at_ms = TimestampMs::from_epoch_millis(self.clock.now_ms());
        let thumbnail_size_bytes = generated.thumbnail_bytes.len() as i64;
        let metadata = ThumbnailMetadata::new(
            rep_id.clone(),
            thumbnail_blob_id,
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
    use crate::clipboard::testing::{ScriptedRepRepo, ScriptedReturn};
    use crate::security::Blake3Hasher;
    use async_trait::async_trait;
    use tempfile::TempDir;
    use uc_core::clipboard::PersistedClipboardRepresentation;
    use uc_core::ids::FormatId;
    use uc_core::ports::clipboard::GeneratedThumbnail;
    use uc_core::{BlobId, ContentHash};

    struct FakeBlobWriter;
    #[async_trait]
    impl BlobWriterPort for FakeBlobWriter {
        async fn write_if_absent(
            &self,
            _content_id: &ContentHash,
            _bytes: &[u8],
        ) -> anyhow::Result<BlobId> {
            Ok(BlobId::from("blob-x"))
        }

        async fn write_path_if_absent(&self, _path: &std::path::Path) -> anyhow::Result<BlobId> {
            unimplemented!("worker only uses write_if_absent")
        }
    }

    // A text/plain representation never reaches thumbnail generation, so these
    // fakes must stay unreachable. They panic if the worker ever calls them.
    struct UnusedThumbnailRepo;
    #[async_trait]
    impl ThumbnailRepositoryPort for UnusedThumbnailRepo {
        async fn get_by_representation_id(
            &self,
            _id: &RepresentationId,
        ) -> anyhow::Result<Option<ThumbnailMetadata>> {
            unimplemented!("thumbnail path is unreachable for text/plain")
        }
        async fn insert_thumbnail(&self, _m: &ThumbnailMetadata) -> anyhow::Result<()> {
            unimplemented!("thumbnail path is unreachable for text/plain")
        }
    }

    struct UnusedThumbnailGenerator;
    #[async_trait]
    impl ThumbnailGeneratorPort for UnusedThumbnailGenerator {
        async fn generate_thumbnail(&self, _b: &[u8]) -> anyhow::Result<GeneratedThumbnail> {
            unimplemented!("thumbnail path is unreachable for text/plain")
        }
        async fn generate_thumbnail_from_rgba(
            &self,
            _b: &[u8],
            _w: u32,
            _h: u32,
        ) -> anyhow::Result<GeneratedThumbnail> {
            unimplemented!("thumbnail path is unreachable for text/plain")
        }
    }

    struct FixedClock;
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            0
        }
    }

    fn text_rep(id: &RepresentationId) -> PersistedClipboardRepresentation {
        PersistedClipboardRepresentation::new_staged(
            id.clone(),
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType("text/plain".to_string())),
            10,
        )
    }

    /// Regression: once the worker materializes a blob, the in-memory
    /// `RepresentationCache` copy must be released. Otherwise every copied
    /// image leaks a full byte buffer into the cache until it hits its size
    /// ceiling, so daemon memory grows under a stream of copies and never
    /// falls back down.
    #[tokio::test]
    async fn worker_releases_cache_after_processing() {
        let dir = TempDir::new().expect("tempdir");
        let cache = Arc::new(RepresentationCache::new(16, 1024 * 1024));
        let spool = Arc::new(SpoolManager::new(dir.path(), 1024 * 1024).expect("spool"));
        let (tx, rx) = mpsc::channel(8);

        let rep_id = RepresentationId::from("rep-1");
        // Seed the cache exactly like CaptureClipboardUseCase does on capture.
        cache.put(&rep_id, b"clipboard-bytes".to_vec()).await;
        assert!(cache.get(&rep_id).await.is_some());

        let repo = Arc::new(ScriptedRepRepo::new());
        // Staged -> Processing, then Processing -> BlobReady.
        repo.push_update_outcome(ScriptedReturn::Ok(ProcessingUpdateOutcome::Updated(
            text_rep(&rep_id),
        )));
        repo.push_update_outcome(ScriptedReturn::Ok(ProcessingUpdateOutcome::Updated(
            text_rep(&rep_id),
        )));
        repo.set_representation(text_rep(&rep_id));

        let worker = BackgroundBlobWorker::new(
            rx,
            cache.clone(),
            spool.clone(),
            repo,
            Arc::new(FakeBlobWriter),
            Arc::new(Blake3Hasher),
            Arc::new(UnusedThumbnailRepo),
            Arc::new(UnusedThumbnailGenerator),
            Arc::new(FixedClock),
            3,
            Duration::from_millis(1),
        );

        tx.send(rep_id.clone()).await.expect("send");
        drop(tx); // close channel so run() drains and returns
        worker.run().await;

        assert!(
            cache.get(&rep_id).await.is_none(),
            "cache bytes must be released after blob materialization"
        );
    }
}
