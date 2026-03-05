use anyhow::Result;

/// Convert raw CF_DIB data (BITMAPINFOHEADER + pixel data, no BMP file header) to PNG bytes.
///
/// This function is platform-independent (uses only the `image` crate) and can be tested
/// on any OS. Windows-specific clipboard access is handled separately in `platform/windows.rs`.
pub(crate) fn dib_to_png(_dib_data: &[u8]) -> Result<Vec<u8>> {
    unimplemented!("RED phase: not yet implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construct a minimal valid BITMAPINFOHEADER (40 bytes) with a 2x2 32-bit BGRA image.
    fn make_dib_2x2_red() -> Vec<u8> {
        let mut data = Vec::new();
        // BITMAPINFOHEADER (40 bytes)
        data.extend_from_slice(&40u32.to_le_bytes()); // biSize
        data.extend_from_slice(&2i32.to_le_bytes()); // biWidth = 2
                                                     // Negative height = top-down DIB (no vertical flip needed)
        data.extend_from_slice(&(-2i32).to_le_bytes()); // biHeight = -2 (top-down)
        data.extend_from_slice(&1u16.to_le_bytes()); // biPlanes = 1
        data.extend_from_slice(&32u16.to_le_bytes()); // biBitCount = 32
        data.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
        data.extend_from_slice(&0u32.to_le_bytes()); // biSizeImage = 0
        data.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
        data.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
        data.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
        data.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant
                                                     // Pixel data: 4 red pixels in BGRA format
        for _ in 0..4 {
            data.extend_from_slice(&[0x00, 0x00, 0xFF, 0xFF]); // B=0, G=0, R=255, A=255
        }
        data
    }

    #[test]
    fn test_dib_to_png_magic_bytes() {
        let dib = make_dib_2x2_red();
        let png = dib_to_png(&dib).expect("should convert successfully");
        assert_eq!(&png[0..4], &[0x89, 0x50, 0x4E, 0x47], "PNG magic bytes");
    }

    #[test]
    fn test_dib_to_png_roundtrip() {
        let dib = make_dib_2x2_red();
        let png = dib_to_png(&dib).expect("should convert successfully");
        let img = image::load_from_memory(&png).expect("should decode PNG");
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
    }

    #[test]
    fn test_dib_to_png_empty_input() {
        let result = dib_to_png(&[]);
        assert!(result.is_err(), "empty input should error");
    }

    #[test]
    fn test_dib_to_png_truncated_header() {
        let result = dib_to_png(&[0x28, 0x00, 0x00]); // partial header
        assert!(result.is_err(), "truncated header should error");
    }
}
