//! Decrypting clipboard representation repository decorator.
//!
//! Wraps ClipboardRepresentationRepositoryPort and decrypts inline_data on read.
//!
//! Slice 3 起通过 BlobCipherPort 加解密——见 decrypting_clipboard_event_repo
//! 的 wire format 兼容性说明。

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, trace};

use uc_core::ports::clipboard::ProcessingUpdateOutcome;
use uc_core::{
    clipboard::{PayloadAvailability, PersistedClipboardRepresentation},
    crypto::aad,
    crypto::domain::{Aad, ActiveSpace, Ciphertext},
    ids::{EventId, RepresentationId, SpaceId},
    ports::{security::BlobCipherPort, ClipboardRepresentationRepositoryPort},
    BlobId,
};

/// Decorator that decrypts representation inline_data on read.
pub struct DecryptingClipboardRepresentationRepository {
    inner: Arc<dyn ClipboardRepresentationRepositoryPort>,
    blob_cipher: Arc<dyn BlobCipherPort>,
}

impl DecryptingClipboardRepresentationRepository {
    pub fn new(
        inner: Arc<dyn ClipboardRepresentationRepositoryPort>,
        blob_cipher: Arc<dyn BlobCipherPort>,
    ) -> Self {
        Self { inner, blob_cipher }
    }
}

fn placeholder_active_space() -> ActiveSpace {
    ActiveSpace::new(SpaceId::from("space"))
}

#[async_trait]
impl ClipboardRepresentationRepositoryPort for DecryptingClipboardRepresentationRepository {
    async fn get_representation(
        &self,
        event_id: &EventId,
        representation_id: &RepresentationId,
    ) -> Result<Option<PersistedClipboardRepresentation>> {
        let rep_opt = self
            .inner
            .get_representation(event_id, representation_id)
            .await?;

        let Some(rep) = rep_opt else {
            return Ok(None);
        };

        let decrypted_inline_data = if let Some(ref encrypted_bytes) = rep.inline_data {
            let aad = aad::for_inline(event_id, representation_id);
            let active = placeholder_active_space();
            let ciphertext = Ciphertext::new(encrypted_bytes.clone());
            let plaintext = self
                .blob_cipher
                .decrypt(&active, &ciphertext, &Aad::from(aad.as_slice()))
                .await
                .context("failed to decrypt inline_data")?;

            trace!(
                representation_id = %representation_id.as_ref(),
                bytes = plaintext.len(),
                "Decrypted inline_data for representation via BlobCipherPort"
            );

            Some(plaintext.into_bytes())
        } else {
            None
        };

        Ok(Some(PersistedClipboardRepresentation::new_with_state(
            rep.id,
            rep.format_id,
            rep.mime_type,
            rep.size_bytes,
            decrypted_inline_data,
            rep.blob_id,
            rep.payload_state,
            rep.last_error,
        )?))
    }

    async fn get_representation_by_id(
        &self,
        representation_id: &RepresentationId,
    ) -> Result<Option<PersistedClipboardRepresentation>> {
        let rep_opt = self
            .inner
            .get_representation_by_id(representation_id)
            .await?;

        let Some(rep) = rep_opt else {
            return Ok(None);
        };

        if rep.inline_data.is_some() {
            trace!(
                representation_id = %representation_id.as_ref(),
                "Skipping inline_data decryption: event_id unavailable"
            );
        }

        Ok(Some(rep))
    }

    async fn get_representation_by_blob_id(
        &self,
        blob_id: &BlobId,
    ) -> Result<Option<PersistedClipboardRepresentation>> {
        let rep_opt = self.inner.get_representation_by_blob_id(blob_id).await?;

        let Some(rep) = rep_opt else {
            return Ok(None);
        };

        if rep.inline_data.is_some() {
            trace!(
                blob_id = %blob_id.as_ref(),
                "Skipping inline_data decryption: event_id unavailable"
            );
        }

        Ok(Some(rep))
    }

    async fn update_blob_id(
        &self,
        representation_id: &RepresentationId,
        blob_id: &BlobId,
    ) -> Result<()> {
        self.inner.update_blob_id(representation_id, blob_id).await
    }

    async fn update_blob_id_if_none(
        &self,
        representation_id: &RepresentationId,
        blob_id: &BlobId,
    ) -> Result<bool> {
        self.inner
            .update_blob_id_if_none(representation_id, blob_id)
            .await
    }

    async fn update_processing_result(
        &self,
        rep_id: &RepresentationId,
        expected_states: &[PayloadAvailability],
        blob_id: Option<&BlobId>,
        new_state: PayloadAvailability,
        last_error: Option<&str>,
    ) -> Result<ProcessingUpdateOutcome> {
        self.inner
            .update_processing_result(rep_id, expected_states, blob_id, new_state, last_error)
            .await
    }

    async fn get_representations_for_event(
        &self,
        event_id: &EventId,
    ) -> Result<Vec<PersistedClipboardRepresentation>> {
        let reps = self.inner.get_representations_for_event(event_id).await?;
        let input_count = reps.len();
        let mut decrypted_count = 0usize;
        let mut decrypted_bytes = 0usize;
        let mut result = Vec::with_capacity(reps.len());
        let active = placeholder_active_space();
        for rep in reps {
            if let Some(ref encrypted_bytes) = rep.inline_data {
                let aad = aad::for_inline(event_id, &rep.id);
                let ciphertext = Ciphertext::new(encrypted_bytes.clone());
                match self
                    .blob_cipher
                    .decrypt(&active, &ciphertext, &Aad::from(aad.as_slice()))
                    .await
                {
                    Ok(plaintext) => {
                        decrypted_count += 1;
                        decrypted_bytes += plaintext.len();
                        result.push(PersistedClipboardRepresentation::new_with_state(
                            rep.id,
                            rep.format_id,
                            rep.mime_type,
                            rep.size_bytes,
                            Some(plaintext.into_bytes()),
                            rep.blob_id,
                            rep.payload_state,
                            rep.last_error,
                        )?);
                    }
                    Err(_) => {
                        // 与历史行为对齐:解密失败时返回原始（密文）数据,
                        // 让上层决定如何处理（一般会跳过该 representation）。
                        result.push(rep);
                    }
                }
            } else {
                result.push(rep);
            }
        }
        if decrypted_count > 0 {
            debug!(
                event_id = %event_id.as_ref(),
                representations = input_count,
                decrypted = decrypted_count,
                decrypted_bytes,
                "Decrypted representations for event via BlobCipherPort"
            );
        }
        Ok(result)
    }

    async fn update_mime_type(
        &self,
        rep_id: &RepresentationId,
        mime: &uc_core::clipboard::MimeType,
    ) -> Result<()> {
        self.inner.update_mime_type(rep_id, mime).await
    }

    async fn list_ids_by_payload_state(
        &self,
        states: &[PayloadAvailability],
    ) -> Result<Vec<RepresentationId>> {
        self.inner.list_ids_by_payload_state(states).await
    }
}
