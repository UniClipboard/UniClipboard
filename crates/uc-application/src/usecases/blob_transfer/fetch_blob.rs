use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;

use uc_core::ids::EntryId;
use uc_core::ports::blob::{
    BlobDigest, BlobProgressSink, BlobReferenceRepositoryPort, BlobTicket, BlobTransferPort,
    PlaintextHash, TagReason,
};
use uc_core::ports::ContentHashPort;

pub(crate) struct FetchBlobInput {
    pub ticket: BlobTicket,
    pub entry_id: EntryId,
    /// 可选进度上报通道。adapter 在 fetch 过程中按字节阈值/时间窗节流回调。
    /// `None` 则 adapter 不上报,行为与改造前一致。
    pub progress: Option<Arc<dyn BlobProgressSink>>,
}

#[derive(Debug, Clone)]
pub(crate) struct FetchBlobOutcome {
    pub plaintext: Bytes,
    pub entry_id: EntryId,
    pub plaintext_hash: PlaintextHash,
    pub digest: BlobDigest,
}

/// Streaming variant of [`FetchBlobInput`] — drops `plaintext: Bytes` from
/// the result and writes the blob directly to `target_path`.
///
/// GH#487 Phase 2: receive-side mirror of `PublishBlobInput::Path`. Used by
/// the inbound materializer for free-standing files so a 1 GiB clipboard
/// transfer no longer routes the full plaintext through `Bytes` + a second
/// `tokio::fs::write`.
pub(crate) struct FetchBlobPathInput {
    pub ticket: BlobTicket,
    pub entry_id: EntryId,
    pub target_path: PathBuf,
    pub progress: Option<Arc<dyn BlobProgressSink>>,
}

#[derive(Debug, Clone)]
pub(crate) struct FetchBlobPathOutcome {
    pub entry_id: EntryId,
    pub plaintext_hash: PlaintextHash,
    pub digest: BlobDigest,
    /// File size on disk after `fetch_to_path` returned. Lets callers (e.g.
    /// the facade emitting the final 100 % progress event) report a real
    /// number even when the upstream `total_bytes` was unknown.
    pub bytes_written: u64,
}

pub(crate) struct FetchBlobUseCase {
    hash: Arc<dyn ContentHashPort>,
    blob_transfer: Arc<dyn BlobTransferPort>,
    blob_reference: Arc<dyn BlobReferenceRepositoryPort>,
}

impl FetchBlobUseCase {
    pub fn new(
        hash: Arc<dyn ContentHashPort>,
        blob_transfer: Arc<dyn BlobTransferPort>,
        blob_reference: Arc<dyn BlobReferenceRepositoryPort>,
    ) -> Self {
        Self {
            hash,
            blob_transfer,
            blob_reference,
        }
    }

    pub async fn execute(&self, input: FetchBlobInput) -> Result<FetchBlobOutcome, FetchBlobError> {
        let digest = self
            .blob_transfer
            .digest_of(&input.ticket)
            .map_err(|e| FetchBlobError::Transfer(e.to_string()))?;
        // File blobs are stored raw on iroh-blobs (see PublishBlobUseCase).
        // The fetched bytes are already the plaintext; no decrypt step.
        let progress_ref: Option<&dyn BlobProgressSink> = input
            .progress
            .as_ref()
            .map(|p| &**p as &dyn BlobProgressSink);
        let plaintext_bytes = self
            .blob_transfer
            .fetch(&input.ticket, progress_ref)
            .await
            .map_err(|e| FetchBlobError::Transfer(e.to_string()))?;
        let plaintext_hash = PlaintextHash::from_bytes(
            self.hash
                .hash_bytes(&plaintext_bytes)
                .map_err(|e| FetchBlobError::Hash(e.to_string()))?
                .bytes,
        );

        self.blob_reference
            .save(plaintext_hash, digest)
            .await
            .map_err(|e| FetchBlobError::Reference(e.to_string()))?;
        self.blob_transfer
            .tag(&digest, TagReason::ClipboardEntry(input.entry_id.clone()))
            .await
            .map_err(|e| FetchBlobError::Transfer(e.to_string()))?;

        Ok(FetchBlobOutcome {
            plaintext: plaintext_bytes,
            entry_id: input.entry_id,
            plaintext_hash,
            digest,
        })
    }

    /// Streaming fetch — write the blob directly to `target_path` instead
    /// of returning it as `Bytes`. GH#487 Phase 2.
    ///
    /// Differs from [`execute`](Self::execute) in three places:
    /// * Calls `BlobTransferPort::fetch_to_path`, so the full plaintext
    ///   never passes through `Bytes` on this code path.
    /// * Skips the `ContentHashPort::hash_bytes` step. File blobs are
    ///   stored raw and unencrypted on iroh-blobs (see `PublishBlobUseCase`
    ///   path branch); the BAO root the adapter validates equals the
    ///   plaintext blake3, so `plaintext_hash` is read straight from
    ///   `digest` rather than being recomputed by streaming the file
    ///   back in. Saves a second whole-file BLAKE3 pass.
    /// * Returns the file size from `tokio::fs::metadata` so callers can
    ///   emit a final progress event without re-reading the file.
    pub async fn execute_to_path(
        &self,
        input: FetchBlobPathInput,
    ) -> Result<FetchBlobPathOutcome, FetchBlobError> {
        let progress_ref: Option<&dyn BlobProgressSink> = input
            .progress
            .as_ref()
            .map(|p| &**p as &dyn BlobProgressSink);
        let digest = self
            .blob_transfer
            .fetch_to_path(&input.ticket, &input.target_path, progress_ref)
            .await
            .map_err(|e| FetchBlobError::Transfer(e.to_string()))?;

        // File blobs are content-addressed by blake3 of the raw plaintext;
        // since they're stored without encryption the adapter's digest IS
        // the plaintext hash (mirrors `PublishBlobUseCase::execute_path`).
        let plaintext_hash = PlaintextHash::from_bytes(*digest.as_bytes());

        self.blob_reference
            .save(plaintext_hash, digest)
            .await
            .map_err(|e| FetchBlobError::Reference(e.to_string()))?;
        self.blob_transfer
            .tag(&digest, TagReason::ClipboardEntry(input.entry_id.clone()))
            .await
            .map_err(|e| FetchBlobError::Transfer(e.to_string()))?;

        let bytes_written = tokio::fs::metadata(&input.target_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(FetchBlobPathOutcome {
            entry_id: input.entry_id,
            plaintext_hash,
            digest,
            bytes_written,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum FetchBlobError {
    #[error("hash failed: {0}")]
    Hash(String),
    #[error("blob transfer failed: {0}")]
    Transfer(String),
    #[error("blob reference failed: {0}")]
    Reference(String),
}
