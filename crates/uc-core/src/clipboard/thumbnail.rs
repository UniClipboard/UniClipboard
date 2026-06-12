use crate::clipboard::{MimeType, TimestampMs};
use crate::ids::{BlobId, RepresentationId};

/// Thumbnail metadata associated with a clipboard representation.
///
/// 与剪贴板表示关联的缩略图元数据。
pub struct ThumbnailMetadata {
    /// Representation identifier this thumbnail belongs to.
    ///
    /// 此缩略图所属的表示标识符。
    pub representation_id: RepresentationId,
    /// Blob identifier where the thumbnail bytes are stored.
    ///
    /// 缩略图字节存储位置的 Blob 标识符。
    pub thumbnail_blob_id: BlobId,
    /// MIME type of the thumbnail bytes (e.g. image/webp).
    ///
    /// 缩略图字节的 MIME 类型（例如 image/webp）。
    pub thumbnail_mime_type: MimeType,
    /// Original image width in pixels.
    ///
    /// 原始图像宽度（像素）。
    pub original_width: i32,
    /// Original image height in pixels.
    ///
    /// 原始图像高度（像素）。
    pub original_height: i32,
    /// Logical size in bytes of the original image payload.
    ///
    /// 原始图像负载的逻辑大小（字节）。
    pub original_size_bytes: i64,
    /// Optional creation timestamp (epoch millis).
    ///
    /// 可选的创建时间戳（毫秒）。
    pub created_at_ms: Option<TimestampMs>,
}

impl ThumbnailMetadata {
    pub fn new(
        representation_id: RepresentationId,
        thumbnail_blob_id: BlobId,
        thumbnail_mime_type: MimeType,
        original_width: i32,
        original_height: i32,
        original_size_bytes: i64,
        created_at_ms: Option<TimestampMs>,
    ) -> Self {
        Self {
            representation_id,
            thumbnail_blob_id,
            thumbnail_mime_type,
            original_width,
            original_height,
            original_size_bytes,
            created_at_ms,
        }
    }
}
