use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use tracing::info;

use uc_core::ids::EntryId;
use uc_core::ports::blob::{
    BlobDigest, BlobReferenceRepositoryPort, BlobTicket, BlobTransferPort, PlaintextHash, TagReason,
};
use uc_core::ports::ContentHashPort;

#[derive(Debug, Clone)]
pub(crate) struct PublishBlobInput {
    pub plaintext: Bytes,
    pub entry_id: EntryId,
}

#[derive(Debug, Clone)]
pub(crate) struct PublishBlobOutcome {
    pub ticket: BlobTicket,
    pub entry_id: EntryId,
    pub plaintext_hash: PlaintextHash,
    pub digest: BlobDigest,
    pub reused_existing: bool,
}

pub(crate) struct PublishBlobUseCase {
    hash: Arc<dyn ContentHashPort>,
    blob_transfer: Arc<dyn BlobTransferPort>,
    blob_reference: Arc<dyn BlobReferenceRepositoryPort>,
}

impl PublishBlobUseCase {
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

    pub async fn execute(
        &self,
        input: PublishBlobInput,
    ) -> Result<PublishBlobOutcome, PublishBlobError> {
        if input.plaintext.is_empty() {
            return Err(PublishBlobError::EmptyPlaintext);
        }

        // Phase timing for outbound blob publish.
        // hash 与 add_bytes 都会对 plaintext 做一次 BLAKE3,大文件场景下两次
        // 加起来不可忽略;tag/ticket/save_ref 涉及 store + sqlite 写入,冷启动
        // 时也可能慢。GH#487 诊断需要这些阶段拆分。
        let bytes = input.plaintext.len() as u64;

        let hash_start = Instant::now();
        let plaintext_hash = PlaintextHash::from_bytes(
            self.hash
                .hash_bytes(&input.plaintext)
                .map_err(|e| PublishBlobError::Hash(e.to_string()))?
                .bytes,
        );
        let hash_ms = hash_start.elapsed().as_millis() as u64;

        let lookup_start = Instant::now();
        if let Some(digest) = self.find_reusable_digest(&plaintext_hash).await? {
            let lookup_ms = lookup_start.elapsed().as_millis() as u64;

            let tag_start = Instant::now();
            self.blob_transfer
                .tag(&digest, TagReason::ClipboardEntry(input.entry_id.clone()))
                .await
                .map_err(|e| PublishBlobError::Transfer(e.to_string()))?;
            let tag_ms = tag_start.elapsed().as_millis() as u64;

            let ticket_start = Instant::now();
            let ticket = self
                .blob_transfer
                .issue_ticket(&digest)
                .await
                .map_err(|e| PublishBlobError::Transfer(e.to_string()))?;
            let ticket_ms = ticket_start.elapsed().as_millis() as u64;

            info!(
                entry_id = %input.entry_id.as_str(),
                bytes,
                reused_existing = true,
                hash_ms,
                lookup_ms,
                tag_ms,
                ticket_ms,
                "publish_blob: reused existing digest"
            );

            return Ok(PublishBlobOutcome {
                ticket,
                entry_id: input.entry_id,
                plaintext_hash,
                digest,
                reused_existing: true,
            });
        }
        let lookup_ms = lookup_start.elapsed().as_millis() as u64;

        // File blobs go through iroh-blobs as raw bytes — content-addressed by
        // blake3 of the plaintext, which equals `plaintext_hash`. Application-
        // layer encryption is intentionally absent: file payloads are opaque
        // user-chosen content (the user already consented by copying), and any
        // sensitive *metadata* (filenames, paths, mime, thumbnails) lives on
        // the clipboard event side and is encrypted there by
        // `EncryptingClipboardEventWriter`.
        let publish_start = Instant::now();
        let digest = self
            .blob_transfer
            .publish(input.plaintext)
            .await
            .map_err(|e| PublishBlobError::Transfer(e.to_string()))?;
        let publish_ms = publish_start.elapsed().as_millis() as u64;

        let save_ref_start = Instant::now();
        self.blob_reference
            .save(plaintext_hash, digest)
            .await
            .map_err(|e| PublishBlobError::Reference(e.to_string()))?;
        let save_ref_ms = save_ref_start.elapsed().as_millis() as u64;

        let tag_start = Instant::now();
        self.blob_transfer
            .tag(&digest, TagReason::ClipboardEntry(input.entry_id.clone()))
            .await
            .map_err(|e| PublishBlobError::Transfer(e.to_string()))?;
        let tag_ms = tag_start.elapsed().as_millis() as u64;

        let ticket_start = Instant::now();
        let ticket = self
            .blob_transfer
            .issue_ticket(&digest)
            .await
            .map_err(|e| PublishBlobError::Transfer(e.to_string()))?;
        let ticket_ms = ticket_start.elapsed().as_millis() as u64;

        info!(
            entry_id = %input.entry_id.as_str(),
            bytes,
            reused_existing = false,
            hash_ms,
            lookup_ms,
            publish_ms,
            save_ref_ms,
            tag_ms,
            ticket_ms,
            "publish_blob: new blob added"
        );

        Ok(PublishBlobOutcome {
            ticket,
            entry_id: input.entry_id,
            plaintext_hash,
            digest,
            reused_existing: false,
        })
    }

    async fn find_reusable_digest(
        &self,
        plaintext_hash: &PlaintextHash,
    ) -> Result<Option<BlobDigest>, PublishBlobError> {
        let Some(digest) = self
            .blob_reference
            .find_by_plaintext_hash(plaintext_hash)
            .await
            .map_err(|e| PublishBlobError::Reference(e.to_string()))?
        else {
            return Ok(None);
        };

        let exists = self
            .blob_transfer
            .has(&digest)
            .await
            .map_err(|e| PublishBlobError::Transfer(e.to_string()))?;
        Ok(exists.then_some(digest))
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PublishBlobError {
    #[error("blob plaintext is empty")]
    EmptyPlaintext,
    #[error("hash failed: {0}")]
    Hash(String),
    #[error("blob transfer failed: {0}")]
    Transfer(String),
    #[error("blob reference failed: {0}")]
    Reference(String),
}
