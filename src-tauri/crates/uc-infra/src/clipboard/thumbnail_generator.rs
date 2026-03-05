use anyhow::{Context, Result};
use async_trait::async_trait;
use image::{imageops::FilterType, ColorType, GenericImageView};
use uc_core::clipboard::MimeType;
use uc_core::ports::clipboard::{GeneratedThumbnail, ThumbnailGeneratorPort};

pub struct InfraThumbnailGenerator {
    max_edge: u32,
}

impl InfraThumbnailGenerator {
    pub fn new(max_edge: u32) -> Result<Self> {
        if max_edge == 0 {
            anyhow::bail!("max_edge must be greater than 0, got: {}", max_edge);
        }
        Ok(Self { max_edge })
    }
}

#[async_trait]
impl ThumbnailGeneratorPort for InfraThumbnailGenerator {
    async fn generate_thumbnail(&self, image_bytes: &[u8]) -> Result<GeneratedThumbnail> {
        let decoded =
            image::load_from_memory(image_bytes).context("decode image bytes for thumbnail")?;
        let (original_width, original_height) = decoded.dimensions();
        self.generate_from_decoded(decoded, original_width, original_height)
    }

    async fn generate_thumbnail_from_rgba(
        &self,
        rgba_bytes: &[u8],
        width: u32,
        height: u32,
    ) -> Result<GeneratedThumbnail> {
        let rgba_image = image::RgbaImage::from_raw(width, height, rgba_bytes.to_vec())
            .ok_or_else(|| anyhow::anyhow!("RGBA buffer size mismatch for {}x{}", width, height))?;
        let decoded = image::DynamicImage::ImageRgba8(rgba_image);
        self.generate_from_decoded(decoded, width, height)
    }
}

impl InfraThumbnailGenerator {
    fn generate_from_decoded(
        &self,
        decoded: image::DynamicImage,
        original_width: u32,
        original_height: u32,
    ) -> Result<GeneratedThumbnail> {
        let (target_width, target_height) =
            calculate_target_size(original_width, original_height, self.max_edge);

        let resized = if target_width == original_width && target_height == original_height {
            decoded
        } else {
            image::DynamicImage::ImageRgba8(image::imageops::resize(
                &decoded,
                target_width,
                target_height,
                FilterType::Triangle,
            ))
        };

        let rgba = resized.to_rgba8();
        let (thumbnail_width, thumbnail_height) = rgba.dimensions();
        let mut thumbnail_bytes = Vec::new();
        let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut thumbnail_bytes);
        encoder
            .encode(
                rgba.as_raw(),
                thumbnail_width,
                thumbnail_height,
                ColorType::Rgba8.into(),
            )
            .context("encode thumbnail to webp")?;

        Ok(GeneratedThumbnail {
            thumbnail_bytes,
            thumbnail_mime_type: MimeType("image/webp".to_string()),
            original_width: i32::try_from(original_width)
                .context("original width exceeds i32 range")?,
            original_height: i32::try_from(original_height)
                .context("original height exceeds i32 range")?,
        })
    }
}

fn calculate_target_size(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    if width <= max_edge && height <= max_edge {
        return (width, height);
    }

    if width >= height {
        let scaled_height = ((height as f64) * (max_edge as f64) / (width as f64)).round() as u32;
        (max_edge, scaled_height.max(1))
    } else {
        let scaled_width = ((width as f64) * (max_edge as f64) / (height as f64)).round() as u32;
        (scaled_width.max(1), max_edge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_thumbnail_generator_resizes_to_max_edge() {
        let image = image::RgbImage::new(256, 128);
        let mut png_bytes = Vec::new();
        image::DynamicImage::ImageRgb8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut png_bytes),
                image::ImageFormat::Png,
            )
            .unwrap();

        let generator = InfraThumbnailGenerator::new(128).unwrap();
        let output = generator.generate_thumbnail(&png_bytes).await.unwrap();

        assert_eq!(output.thumbnail_mime_type.as_str(), "image/webp");
        assert_eq!(output.original_width, 256);
        assert_eq!(output.original_height, 128);
        let decoded = image::load_from_memory(&output.thumbnail_bytes).unwrap();
        assert_eq!(decoded.width(), 128);
        assert_eq!(decoded.height(), 64);
    }

    #[tokio::test]
    async fn test_thumbnail_generator_does_not_upscale_smaller_image() {
        let image = image::RgbImage::new(64, 32);
        let mut png_bytes = Vec::new();
        image::DynamicImage::ImageRgb8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut png_bytes),
                image::ImageFormat::Png,
            )
            .unwrap();

        let generator = InfraThumbnailGenerator::new(128).unwrap();
        let output = generator.generate_thumbnail(&png_bytes).await.unwrap();

        assert_eq!(output.thumbnail_mime_type.as_str(), "image/webp");
        assert_eq!(output.original_width, 64);
        assert_eq!(output.original_height, 32);
        let decoded = image::load_from_memory(&output.thumbnail_bytes).unwrap();
        assert_eq!(decoded.width(), 64);
        assert_eq!(decoded.height(), 32);
    }
}
