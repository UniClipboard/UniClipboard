use crate::clipboard::MimeType;
use anyhow::Result;

/// Generated thumbnail payload and metadata.
///
/// 生成的缩略图负载与元数据。
pub struct GeneratedThumbnail {
    /// Thumbnail bytes.
    ///
    /// 缩略图字节数据。
    pub thumbnail_bytes: Vec<u8>,
    /// MIME type for thumbnail bytes.
    ///
    /// 缩略图字节的 MIME 类型。
    pub thumbnail_mime_type: MimeType,
    /// Original image width in pixels.
    ///
    /// 原始图像宽度（像素）。
    pub original_width: i32,
    /// Original image height in pixels.
    ///
    /// 原始图像高度（像素）。
    pub original_height: i32,
}

/// Generator port for creating thumbnails from image bytes.
///
/// 从图像字节生成缩略图的生成器端口。
#[async_trait::async_trait]
pub trait ThumbnailGeneratorPort: Send + Sync {
    /// Generate thumbnail from image bytes.
    ///
    /// 从图像字节生成缩略图。
    async fn generate_thumbnail(&self, image_bytes: &[u8]) -> Result<GeneratedThumbnail>;

    /// Generate thumbnail from pre-decoded RGBA pixels.
    ///
    /// Avoids re-decoding when the caller already has raw pixel data
    /// (e.g. after TIFF→PNG conversion).
    ///
    /// 从已解码的 RGBA 像素生成缩略图，避免重复解码。
    async fn generate_thumbnail_from_rgba(
        &self,
        rgba_bytes: &[u8],
        width: u32,
        height: u32,
    ) -> Result<GeneratedThumbnail>;
}
