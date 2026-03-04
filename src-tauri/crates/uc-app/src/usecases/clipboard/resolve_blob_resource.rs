use anyhow::Result;
use std::sync::Arc;

use uc_core::{
    clipboard::MimeType,
    ports::{BlobStorePort, ClipboardRepresentationRepositoryPort},
    BlobId,
};

/// Resolve blob resource by blob id.
/// 通过 blob id 解析资源内容。
pub struct ResolveBlobResourceUseCase {
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    blob_store: Arc<dyn BlobStorePort>,
}

/// Blob resource payload and metadata.
/// Blob 资源内容与元信息。
#[derive(Debug, Clone)]
pub struct BlobResourceResult {
    pub blob_id: BlobId,
    pub mime_type: Option<String>,
    pub bytes: Vec<u8>,
}

impl ResolveBlobResourceUseCase {
    pub fn new(
        representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
        blob_store: Arc<dyn BlobStorePort>,
    ) -> Self {
        Self {
            representation_repo,
            blob_store,
        }
    }

    pub async fn execute(&self, blob_id: &BlobId) -> Result<BlobResourceResult> {
        let representation = self
            .representation_repo
            .get_representation_by_blob_id(blob_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Representation not found"))?;

        if let Some(rep_blob_id) = representation.blob_id.as_ref() {
            if rep_blob_id != blob_id {
                return Err(anyhow::anyhow!("Representation blob_id mismatch"));
            }
        }

        let mime_type = representation
            .mime_type
            .as_ref()
            .map(MimeType::as_str)
            .map(String::from);

        let bytes = self.blob_store.get(blob_id).await?;

        Ok(BlobResourceResult {
            blob_id: blob_id.clone(),
            mime_type,
            bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use uc_core::clipboard::{MimeType, PayloadAvailability, PersistedClipboardRepresentation};
    use uc_core::ids::{EventId, RepresentationId};
    use uc_core::ports::clipboard::ProcessingUpdateOutcome;
    use uc_core::BlobId;

    struct MockRepresentationRepository {
        rep: Option<PersistedClipboardRepresentation>,
    }

    struct MockBlobStore {
        blob_id: BlobId,
        bytes: Vec<u8>,
    }

    #[async_trait]
    impl ClipboardRepresentationRepositoryPort for MockRepresentationRepository {
        async fn get_representation(
            &self,
            _event_id: &EventId,
            _representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn get_representation_by_id(
            &self,
            _representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn get_representation_by_blob_id(
            &self,
            _blob_id: &BlobId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(self.rep.clone())
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
            _rep_id: &RepresentationId,
            _expected_states: &[PayloadAvailability],
            _blob_id: Option<&BlobId>,
            _new_state: PayloadAvailability,
            _last_error: Option<&str>,
        ) -> Result<ProcessingUpdateOutcome> {
            Ok(ProcessingUpdateOutcome::NotFound)
        }
    }

    #[async_trait]
    impl BlobStorePort for MockBlobStore {
        async fn put(
            &self,
            _blob_id: &BlobId,
            _data: &[u8],
        ) -> Result<(std::path::PathBuf, Option<i64>)> {
            Ok((std::path::PathBuf::from("/tmp/mock"), None))
        }

        async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>> {
            if *blob_id == self.blob_id {
                Ok(self.bytes.clone())
            } else {
                Err(anyhow::anyhow!("Blob not found"))
            }
        }
    }

    #[tokio::test]
    async fn test_resolve_blob_resource_returns_bytes() {
        let blob_id = BlobId::from("blob-1");
        let rep_id = RepresentationId::from("rep-1");
        let representation = PersistedClipboardRepresentation::new(
            rep_id,
            uc_core::ids::FormatId::from("public.png"),
            Some(MimeType("image/png".to_string())),
            128,
            None,
            Some(blob_id.clone()),
        );

        let uc = ResolveBlobResourceUseCase::new(
            Arc::new(MockRepresentationRepository {
                rep: Some(representation),
            }),
            Arc::new(MockBlobStore {
                blob_id: blob_id.clone(),
                bytes: vec![1, 2, 3],
            }),
        );

        let result = uc.execute(&blob_id).await.unwrap();

        assert_eq!(result.blob_id, blob_id);
        assert_eq!(result.mime_type, Some("image/png".to_string()));
        assert_eq!(result.bytes, vec![1, 2, 3]);
    }
}
