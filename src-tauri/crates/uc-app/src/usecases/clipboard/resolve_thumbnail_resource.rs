use anyhow::Result;
use std::sync::Arc;
use uc_core::ids::RepresentationId;
use uc_core::ports::{BlobStorePort, ThumbnailRepositoryPort};

/// Resolve thumbnail resource by representation id.
/// 通过表示 id 解析缩略图资源内容。
pub struct ResolveThumbnailResourceUseCase {
    thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
    blob_store: Arc<dyn BlobStorePort>,
}

/// Thumbnail resource payload and metadata.
/// 缩略图资源内容与元信息。
#[derive(Debug, Clone)]
pub struct ThumbnailResourceResult {
    pub representation_id: RepresentationId,
    pub mime_type: Option<String>,
    pub bytes: Vec<u8>,
}

impl ResolveThumbnailResourceUseCase {
    pub fn new(
        thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
        blob_store: Arc<dyn BlobStorePort>,
    ) -> Self {
        Self {
            thumbnail_repo,
            blob_store,
        }
    }

    #[tracing::instrument(
        name = "usecase.clipboard.resolve_thumbnail_resource.execute",
        skip(self)
    )]
    pub async fn execute(
        &self,
        representation_id: &RepresentationId,
    ) -> Result<ThumbnailResourceResult> {
        let metadata = self
            .thumbnail_repo
            .get_by_representation_id(representation_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Thumbnail not found"))?;

        if metadata.representation_id != *representation_id {
            return Err(anyhow::anyhow!("Thumbnail representation id mismatch"));
        }

        let bytes = self.blob_store.get(&metadata.thumbnail_blob_id).await?;
        let mime_type = Some(metadata.thumbnail_mime_type.as_str().to_string());

        Ok(ThumbnailResourceResult {
            representation_id: representation_id.clone(),
            mime_type,
            bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use async_trait::async_trait;
    use uc_core::clipboard::{MimeType, ThumbnailMetadata};
    use uc_core::ids::RepresentationId;
    use uc_core::BlobId;

    struct MockThumbnailRepo {
        metadata: Option<ThumbnailMetadata>,
    }

    struct MockBlobStore {
        blob_id: BlobId,
        bytes: Vec<u8>,
    }

    #[async_trait]
    impl ThumbnailRepositoryPort for MockThumbnailRepo {
        async fn get_by_representation_id(
            &self,
            _representation_id: &RepresentationId,
        ) -> Result<Option<ThumbnailMetadata>> {
            Ok(self.metadata.as_ref().map(|meta| {
                ThumbnailMetadata::new(
                    meta.representation_id.clone(),
                    meta.thumbnail_blob_id.clone(),
                    meta.thumbnail_mime_type.clone(),
                    meta.original_width,
                    meta.original_height,
                    meta.original_size_bytes,
                    meta.created_at_ms,
                )
            }))
        }

        async fn insert_thumbnail(&self, _metadata: &ThumbnailMetadata) -> Result<()> {
            Ok(())
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
    async fn test_resolve_thumbnail_resource_returns_bytes() {
        let rep_id = RepresentationId::from("rep-1");
        let blob_id = BlobId::from("thumb-1");
        let metadata = ThumbnailMetadata::new(
            rep_id.clone(),
            blob_id.clone(),
            MimeType("image/webp".to_string()),
            120,
            80,
            1024,
            None,
        );

        let uc = ResolveThumbnailResourceUseCase::new(
            Arc::new(MockThumbnailRepo {
                metadata: Some(metadata),
            }),
            Arc::new(MockBlobStore {
                blob_id: blob_id.clone(),
                bytes: vec![1, 2, 3],
            }),
        );

        let result = uc.execute(&rep_id).await.unwrap();
        assert_eq!(result.mime_type, Some("image/webp".to_string()));
        assert_eq!(result.bytes, vec![1, 2, 3]);
    }
}
