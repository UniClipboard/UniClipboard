//! Decrypting clipboard event repository decorator.
//!
//! Wraps ClipboardEventRepositoryPort and decrypts ObservedClipboardRepresentation.bytes on read.
//!
//! Slice 3 起通过 BlobCipherPort 加解密——adapter 内部端到端管理会话与
//! V1 AEAD,本 decorator 只做"业务 AAD 构造 + 字节过 port"。

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::trace;

use uc_core::{
    clipboard::ObservedClipboardRepresentation,
    crypto::aad,
    crypto::domain::{Aad, ActiveSpace, Ciphertext},
    ids::{EventId, RepresentationId, SpaceId},
    ports::{security::BlobCipherPort, ClipboardEventRepositoryPort},
};

/// Decorator that decrypts ObservedClipboardRepresentation.bytes on read.
pub struct DecryptingClipboardEventRepository {
    inner: Arc<dyn ClipboardEventRepositoryPort>,
    blob_cipher: Arc<dyn BlobCipherPort>,
}

impl DecryptingClipboardEventRepository {
    pub fn new(
        inner: Arc<dyn ClipboardEventRepositoryPort>,
        blob_cipher: Arc<dyn BlobCipherPort>,
    ) -> Self {
        Self { inner, blob_cipher }
    }
}

#[async_trait]
impl ClipboardEventRepositoryPort for DecryptingClipboardEventRepository {
    async fn get_representation(
        &self,
        event_id: &EventId,
        representation_id: &str,
    ) -> Result<ObservedClipboardRepresentation> {
        let mut observed = self
            .inner
            .get_representation(event_id, representation_id)
            .await?;

        if !observed.bytes.is_empty() {
            // BlobCipherAdapter 内部 wire format 与 4 个 decorator 历史 inline_data
            // 字节布局 (serde_json::to_vec(&EncryptedBlob)) 一致——既有数据可
            // 直接被新 port 解开,不需要数据迁移。
            let aad = aad::for_inline(event_id, &RepresentationId::from(representation_id));
            // 单空间模型下用占位 ActiveSpace,adapter 当前不按 SpaceId 路由。
            let active = ActiveSpace::new(SpaceId::from("space"));
            let ciphertext = Ciphertext::new(observed.bytes.clone());
            let plaintext = self
                .blob_cipher
                .decrypt(&active, &ciphertext, &Aad::from(aad.as_slice()))
                .await
                .context("failed to decrypt representation bytes")?;

            trace!(
                representation_id = %representation_id,
                bytes = plaintext.len(),
                "Decrypted representation bytes via BlobCipherPort"
            );

            observed.bytes = plaintext.into_bytes();
        }

        Ok(observed)
    }

    async fn get_source_device(
        &self,
        event_id: &EventId,
    ) -> Result<Option<uc_core::ids::DeviceId>> {
        self.inner.get_source_device(event_id).await
    }
}
