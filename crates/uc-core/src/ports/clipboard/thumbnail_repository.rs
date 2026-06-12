use crate::clipboard::ThumbnailMetadata;
use crate::ids::RepresentationId;
use anyhow::Result;

/// Repository port for thumbnail metadata persistence.
///
/// 缩略图元数据持久化的仓储端口。
#[async_trait::async_trait]
pub trait ThumbnailRepositoryPort: Send + Sync {
    /// Fetch thumbnail metadata by representation id.
    ///
    /// 通过表示标识符获取缩略图元数据。
    async fn get_by_representation_id(
        &self,
        representation_id: &RepresentationId,
    ) -> Result<Option<ThumbnailMetadata>>;

    /// Insert or update thumbnail metadata.
    ///
    /// 插入或更新缩略图元数据。
    async fn insert_thumbnail(&self, metadata: &ThumbnailMetadata) -> Result<()>;
}
