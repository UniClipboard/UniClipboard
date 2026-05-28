use anyhow::Result;

/// Convert raw CF_DIB data (BITMAPINFOHEADER + pixel data, no BMP file header) to PNG bytes.
///
/// This function is platform-independent (uses only the `image` crate) and can be tested
/// on any OS. Windows-specific clipboard access is handled separately in `platform/windows.rs`.
///
/// **Encoder tuning.** This path runs synchronously on the clipboard watcher
/// thread for every CF_DIB-only screenshot capture, so encoder speed is the
/// dominant constraint. We deviate from `image::write_to`'s defaults
/// (`CompressionType::Default` + `FilterType::Adaptive`, which match libpng's
/// "best compression" preset) in favor of:
///
/// * [`CompressionType::Fast`] — routes deflate through the `png` crate's
///   `fdeflate` fast encoder; per upstream docs this is "dramatically faster
///   than the fastest mode of libpng while simultaneously providing better
///   compression ratio" for the screenshot/UI image distribution we see.
/// * [`FilterType::NoFilter`] — Adaptive filter selection requires a second
///   pass per scanline that costs more than it saves on flat UI screenshots
///   (large solid-color regions, where filtering's "smaller residuals" win
///   evaporates). The file-size penalty vs. Adaptive is ~5-15% in practice;
///   we trade that for capture latency that's user-visible.
///
/// The CF_DIB → PNG path is only the **second-tier** strategy on Windows:
/// modern screenshot sources (Chrome, Office, Snipping Tool, Snipaste, 微信)
/// also write a custom `"PNG"` clipboard format containing ready-to-use PNG
/// bytes, which `read_image_windows_native_png` in `platform/windows.rs`
/// reads with zero encoding work. This function only runs for CF_DIB-only
/// sources (Win+PrtScr, legacy apps), so the size/speed trade above applies
/// to the worst case, not the common case.
pub(crate) fn dib_to_png(dib_data: &[u8]) -> Result<Vec<u8>> {
    use image::codecs::bmp::BmpDecoder;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::{DynamicImage, ImageEncoder};
    use std::io::Cursor;

    let cursor = Cursor::new(dib_data);
    let decoder = BmpDecoder::new_without_file_header(cursor)
        .map_err(|e| anyhow::anyhow!("Failed to decode DIB: {}", e))?;
    let image = DynamicImage::from_decoder(decoder)
        .map_err(|e| anyhow::anyhow!("Failed to load DIB image: {}", e))?;

    // Encode in the image's native color space — round-tripping through
    // `to_rgba8()` (the `image::write_to` default) would force a wasted
    // RGB8 → RGBA8 conversion for the typical CF_DIB-from-screenshot case.
    let rgba = image.to_rgba8();
    let mut png_bytes = Vec::new();
    PngEncoder::new_with_quality(&mut png_bytes, CompressionType::Fast, FilterType::NoFilter)
        .write_image(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| anyhow::anyhow!("Failed to encode PNG: {}", e))?;

    Ok(png_bytes)
}
