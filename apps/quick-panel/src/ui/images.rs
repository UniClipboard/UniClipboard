//! The bitmaps the panel draws, and when they are given back.
//!
//! The panel decodes images itself and draws them as `Arc<RenderImage>` instead of handing the
//! encoded bytes to `img`. GPUI keeps whatever `img` decodes in an application-wide cache and in
//! the window's texture atlas, and frees neither until it is told to; owning the bitmap lets the
//! panel release it with [`App::drop_image`] as soon as it is no longer shown.

use super::ImageData;
use gpui::{Image, ImageFormat, RenderImage};
use quick_panel_core::ports::ImagePayload;
use std::sync::Arc;

/// Longest edge, in physical pixels, of the bitmap a list row or a grid cell draws. A row shows
/// 96 x 64 points and a grid cell about a third of the panel width, so this stays sharp on a
/// Retina display without keeping the full-size bitmap (width x height x 4 bytes) of a screenshot.
const THUMBNAIL_EDGE: u32 = 320;

/// Decodes `payload` into a small bitmap for the list and the grid. Animated images show their
/// first frame. Runs on a blocking thread.
pub(super) fn thumbnail(payload: ImagePayload, size_bytes: i64) -> Option<ImageData> {
    let decoded = image::load_from_memory(&payload.bytes).ok()?;
    let scaled = if decoded.width() > THUMBNAIL_EDGE || decoded.height() > THUMBNAIL_EDGE {
        decoded.thumbnail(THUMBNAIL_EDGE, THUMBNAIL_EDGE)
    } else {
        decoded
    };
    let mut raster = scaled.into_rgba8();
    // GPUI draws BGRA.
    for pixel in raster.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Some(ImageData {
        image: Arc::new(RenderImage::new(vec![image::Frame::new(raster)])),
        width: payload.width,
        height: payload.height,
        size_bytes,
    })
}

/// Decodes `payload` at full size for the preview, with every frame of an animation, using GPUI's
/// own decoder (`decode` is `Image::to_image_data` with the application's SVG renderer, which
/// cannot be named outside GPUI). Runs on a blocking thread.
pub(super) fn full(
    payload: ImagePayload,
    size_bytes: i64,
    decode: impl FnOnce(&Image) -> Option<Arc<RenderImage>>,
) -> Option<ImageData> {
    let format = ImageFormat::from_mime_type(&payload.mime)?;
    let image = Image::from_bytes(format, payload.bytes);
    Some(ImageData {
        image: decode(&image)?,
        width: payload.width,
        height: payload.height,
        size_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(width: u32, height: u32, pixel: [u8; 4]) -> ImagePayload {
        let raster = image::RgbaImage::from_pixel(width, height, image::Rgba(pixel));
        let mut bytes = Vec::new();
        raster
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        ImagePayload {
            bytes,
            mime: "image/png".into(),
            width,
            height,
        }
    }

    #[test]
    fn a_large_image_is_scaled_down_to_the_thumbnail_edge() {
        let data = thumbnail(payload(1000, 500, [10, 20, 30, 255]), 7).unwrap();
        let bytes = data.image.as_bytes(0).unwrap();
        assert_eq!(bytes.len(), 320 * 160 * 4);
        // The size reported to the preview header is that of the original.
        assert_eq!((data.width, data.height, data.size_bytes), (1000, 500, 7));
    }

    #[test]
    fn a_small_image_is_not_enlarged() {
        let data = thumbnail(payload(120, 80, [10, 20, 30, 255]), 1).unwrap();
        assert_eq!(data.image.as_bytes(0).unwrap().len(), 120 * 80 * 4);
    }

    #[test]
    fn pixels_are_stored_as_bgra() {
        let data = thumbnail(payload(4, 4, [10, 20, 30, 255]), 1).unwrap();
        assert_eq!(&data.image.as_bytes(0).unwrap()[..4], &[30, 20, 10, 255]);
    }

    #[test]
    fn bytes_that_are_not_an_image_give_no_thumbnail() {
        let mut broken = payload(4, 4, [0, 0, 0, 255]);
        broken.bytes.truncate(10);
        assert!(thumbnail(broken, 1).is_none());
    }
}
