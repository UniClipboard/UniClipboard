//! Encrypting clipboard event writer decorator.
//!
//! Wraps ClipboardEventWriterPort and encrypts inline_data before storage.
//!
//! Slice 3 起通过 BlobCipherPort 加密——见 decrypting_clipboard_event_repo
//! 的 wire format 兼容性说明。

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, trace};

use uc_core::{
    clipboard::{ClipboardEvent, PersistedClipboardRepresentation},
    crypto::aad,
    crypto::domain::{Aad, ActiveSpace, Plaintext},
    ids::{EventId, SpaceId},
    ports::{security::BlobCipherPort, ClipboardEventWriterPort},
};

/// Decorator that encrypts representation inline_data before storage.
pub struct EncryptingClipboardEventWriter {
    inner: Arc<dyn ClipboardEventWriterPort>,
    blob_cipher: Arc<dyn BlobCipherPort>,
}

impl EncryptingClipboardEventWriter {
    pub fn new(
        inner: Arc<dyn ClipboardEventWriterPort>,
        blob_cipher: Arc<dyn BlobCipherPort>,
    ) -> Self {
        Self { inner, blob_cipher }
    }
}

#[async_trait]
impl ClipboardEventWriterPort for EncryptingClipboardEventWriter {
    async fn insert_event(
        &self,
        event: &ClipboardEvent,
        representations: &Vec<PersistedClipboardRepresentation>,
    ) -> Result<()> {
        // 单空间模型下用占位 ActiveSpace,adapter 当前不按 SpaceId 路由。
        let active = ActiveSpace::new(SpaceId::from("space"));

        let mut encrypted_reps = Vec::with_capacity(representations.len());
        let mut encrypted_count = 0usize;
        let mut total_plaintext_bytes = 0usize;
        let mut total_ciphertext_bytes = 0usize;

        for rep in representations {
            let encrypted_inline_data = if let Some(ref plain_bytes) = rep.inline_data {
                let aad = aad::for_inline(&event.event_id, &rep.id);
                let plaintext = Plaintext::new(plain_bytes.clone());
                let plaintext_len = plaintext.len();
                let ciphertext = self
                    .blob_cipher
                    .encrypt(&active, &plaintext, &Aad::from(aad.as_slice()))
                    .await
                    .context("failed to encrypt inline_data")?;
                let encrypted_bytes = ciphertext.into_bytes();

                trace!(
                    representation_id = %rep.id.as_ref(),
                    plaintext_bytes = plaintext_len,
                    ciphertext_bytes = encrypted_bytes.len(),
                    "Encrypted inline_data via BlobCipherPort"
                );
                encrypted_count += 1;
                total_plaintext_bytes += plaintext_len;
                total_ciphertext_bytes += encrypted_bytes.len();

                Some(encrypted_bytes)
            } else {
                None
            };

            encrypted_reps.push(PersistedClipboardRepresentation::new_with_state(
                rep.id.clone(),
                rep.format_id.clone(),
                rep.mime_type.clone(),
                rep.size_bytes,
                encrypted_inline_data,
                rep.blob_id.clone(),
                rep.payload_state(),
                rep.last_error.clone(),
            )?);
        }

        if encrypted_count > 0 {
            debug!(
                event_id = %event.event_id.as_ref(),
                representations = representations.len(),
                encrypted = encrypted_count,
                total_plaintext_bytes,
                total_ciphertext_bytes,
                "Encrypted inline_data for event via BlobCipherPort"
            );
        }

        self.inner.insert_event(event, &encrypted_reps).await
    }

    async fn delete_event_and_representations(&self, event_id: &EventId) -> Result<()> {
        self.inner.delete_event_and_representations(event_id).await
    }
}
