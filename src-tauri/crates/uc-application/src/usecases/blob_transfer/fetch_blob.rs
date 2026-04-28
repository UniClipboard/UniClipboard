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
