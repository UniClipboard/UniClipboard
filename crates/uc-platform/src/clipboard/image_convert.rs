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

    if dib_data.len() >= BITMAPV5HEADER_SIZE
        && read_u32(dib_data, 0) == Some(BITMAPV5HEADER_SIZE as u32)
    {
        return dibv5_to_png(dib_data);
    }

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

const BITMAPV5HEADER_SIZE: usize = 124;
const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;
const BI_ALPHABITFIELDS: u32 = 6;

fn dibv5_to_png(dib_data: &[u8]) -> Result<Vec<u8>> {
    use image::{Rgba, RgbaImage};
    use std::io::Cursor;

    let width =
        read_i32(dib_data, 4).ok_or_else(|| anyhow::anyhow!("CF_DIBV5 is missing width"))?;
    let height =
        read_i32(dib_data, 8).ok_or_else(|| anyhow::anyhow!("CF_DIBV5 is missing height"))?;
    let planes =
        read_u16(dib_data, 12).ok_or_else(|| anyhow::anyhow!("CF_DIBV5 is missing planes"))?;
    let bits_per_pixel =
        read_u16(dib_data, 14).ok_or_else(|| anyhow::anyhow!("CF_DIBV5 is missing bit count"))?;
    let compression =
        read_u32(dib_data, 16).ok_or_else(|| anyhow::anyhow!("CF_DIBV5 is missing compression"))?;

    if width <= 0 || height == 0 || planes != 1 || bits_per_pixel != 32 {
        anyhow::bail!(
            "unsupported CF_DIBV5 geometry: width={}, height={}, planes={}, bits_per_pixel={}",
            width,
            height,
            planes,
            bits_per_pixel
        );
    }
    if !matches!(compression, BI_RGB | BI_BITFIELDS | BI_ALPHABITFIELDS) {
        anyhow::bail!("unsupported CF_DIBV5 compression: {}", compression);
    }

    let width = width as usize;
    let height_abs = height.unsigned_abs() as usize;
    let row_bytes = width
        .checked_mul(4)
        .ok_or_else(|| anyhow::anyhow!("CF_DIBV5 row size overflow"))?;
    let pixel_bytes = row_bytes
        .checked_mul(height_abs)
        .ok_or_else(|| anyhow::anyhow!("CF_DIBV5 pixel size overflow"))?;
    let pixel_end = BITMAPV5HEADER_SIZE
        .checked_add(pixel_bytes)
        .ok_or_else(|| anyhow::anyhow!("CF_DIBV5 payload size overflow"))?;
    let pixels = dib_data
        .get(BITMAPV5HEADER_SIZE..pixel_end)
        .ok_or_else(|| anyhow::anyhow!("CF_DIBV5 pixel payload is truncated"))?;

    let mut image = RgbaImage::new(width as u32, height_abs as u32);
    for y in 0..height_abs {
        let source_y = if height < 0 { y } else { height_abs - 1 - y };
        let row_start = source_y * row_bytes;
        for x in 0..width {
            let offset = row_start + x * 4;
            let alpha = if compression == BI_RGB {
                255
            } else {
                pixels[offset + 3]
            };
            image.put_pixel(
                x as u32,
                y as u32,
                Rgba([
                    pixels[offset + 2],
                    pixels[offset + 1],
                    pixels[offset],
                    alpha,
                ]),
            );
        }
    }

    let mut png_bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .map_err(|e| anyhow::anyhow!("Failed to encode CF_DIBV5 as PNG: {}", e))?;
    Ok(png_bytes)
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    bytes
        .get(offset..offset + 2)
        .and_then(|value| value.try_into().ok())
        .map(u16::from_le_bytes)
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    bytes
        .get(offset..offset + 4)
        .and_then(|value| value.try_into().ok())
        .map(u32::from_le_bytes)
}

fn read_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    bytes
        .get(offset..offset + 4)
        .and_then(|value| value.try_into().ok())
        .map(i32::from_le_bytes)
}
