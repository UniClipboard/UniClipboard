use anyhow::Result;

/// Convert raw CF_DIB data (BITMAPINFOHEADER + pixel data, no BMP file header) to PNG bytes.
///
/// This function is platform-independent (uses only the `image` crate) and can be tested
/// on any OS. Windows-specific clipboard access is handled separately in `platform/windows.rs`.
///
/// **Encoder choice.** We use `image::write_to`'s defaults
/// (`CompressionType::Default` = `png::Compression::Balanced` = flate2 level 6
/// + `FilterType::Adaptive`). An earlier revision tried
/// `CompressionType::Fast` + `FilterType::NoFilter` to shave encoder time,
/// but Sentry observed a 36 MB CF_DIB (≈5K screenshot) emit a 34 MB PNG —
/// `CompressionType::Fast` routes through `png`'s `FdeflateUltraFast` which
/// the upstream docs explicitly warn "can result in files *larger* than
/// would be produced by `NoCompression` on incompressible data." For the
/// screenshot distribution we capture (large UI regions, many similar
/// pixels) the right point on the curve is Balanced + Adaptive: same
/// trade-off browsers and `oxipng` recommend, and the ~10× compression
/// ratio it gives back is worth far more than the seconds saved on
/// encoding when the downstream pays in disk / wire bytes per capture.
///
/// Encoder-CPU regressions in dev profile are mitigated by the
/// `opt-level = 3` overrides on `image` / `png` / `fdeflate` / `flate2` /
/// `miniz_oxide` in `src-tauri/Cargo.toml`.
///
/// The CF_DIB → PNG path is only the **second-tier** strategy on Windows:
/// modern screenshot sources (Chrome, Office, Snipping Tool, Snipaste, 微信)
/// also write a custom `"PNG"` clipboard format containing ready-to-use PNG
/// bytes, which `read_image_windows_native_png` in `platform/windows.rs`
/// reads with zero encoding work. This function only runs for CF_DIB-only
/// sources (Win+PrtScr, legacy apps).
pub(crate) fn dib_to_png(dib_data: &[u8]) -> Result<Vec<u8>> {
    use image::codecs::bmp::BmpDecoder;
    use image::DynamicImage;
    use std::io::Cursor;

    let cursor = Cursor::new(dib_data);
    let decoder = BmpDecoder::new_without_file_header(cursor)
        .map_err(|e| anyhow::anyhow!("Failed to decode DIB: {}", e))?;
    let image = DynamicImage::from_decoder(decoder)
        .map_err(|e| anyhow::anyhow!("Failed to load DIB image: {}", e))?;

    let mut png_bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .map_err(|e| anyhow::anyhow!("Failed to encode PNG: {}", e))?;

    Ok(png_bytes)
}
