use super::super::common::CommonClipboardImpl;
use anyhow::Result;
use async_trait::async_trait;
use clipboard_rs::{Clipboard, ClipboardContext};
use std::ops::Range;
use std::sync::{Arc, Mutex};
use tracing::{debug, debug_span, error, info, warn};
use uc_core::clipboard::SystemClipboardSnapshot;
use uc_core::ports::SystemClipboardPort;

/// Windows clipboard implementation using clipboard-rs and clipboard-win
pub struct WindowsClipboard {
    inner: Arc<Mutex<ClipboardContext>>,
}

impl WindowsClipboard {
    pub fn new() -> Result<Self> {
        let context = ClipboardContext::new()
            .map_err(|e| anyhow::anyhow!("Failed to create clipboard context: {}", e))?;
        Ok(Self {
            inner: Arc::new(Mutex::new(context)),
        })
    }
}

#[async_trait]
impl SystemClipboardPort for WindowsClipboard {
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
        let span = debug_span!("platform.windows.read_clipboard");
        span.in_scope(|| {
            let mut ctx = self.inner.lock().map_err(|poison| {
                error!("Failed to lock clipboard context in read_snapshot (poisoned mutex)");
                anyhow::anyhow!(
                    "mutex poisoned locking inner in read_snapshot: {}",
                    poison.to_string()
                )
            })?;
            let snapshot = CommonClipboardImpl::read_snapshot(&mut ctx)?;

            debug!(
                formats = snapshot.representations.len(),
                total_size_bytes = snapshot.total_size_bytes(),
                "Captured system clipboard snapshot"
            );

            Ok(snapshot)
        })
    }

    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
        let span = debug_span!(
            "platform.windows.write_clipboard",
            representations = snapshot.representations.len(),
        );
        span.in_scope(|| {
            let fallback_eligible = is_single_text_plain_snapshot(&snapshot);
            let expected_text = if fallback_eligible {
                extract_text_plain_utf8(&snapshot)?
            } else {
                None
            };
            let mut ctx = self.inner.lock().map_err(|poison| {
                error!("Failed to lock clipboard context in write_snapshot (poisoned mutex)");
                anyhow::anyhow!(
                    "mutex poisoned locking inner in write_snapshot: {}",
                    poison.to_string()
                )
            })?;
            let write_result = CommonClipboardImpl::write_snapshot(&mut ctx, snapshot);
            if let Err(err) = write_result {
                if !fallback_eligible {
                    return Err(err);
                }
                drop(ctx);
                if let Some(text) = expected_text.as_deref() {
                    warn!(
                        error = %err,
                        text_len = text.len(),
                        "Primary clipboard-rs write failed; using Windows Unicode text fallback"
                    );
                    write_text_windows_native(text)?;
                    info!("Wrote clipboard text via Windows Unicode fallback");
                    return Ok(());
                }
                return Err(err);
            }

            let mut needs_fallback = false;
            if let Some(expected) = expected_text.as_deref() {
                match ctx.get_text() {
                    Ok(actual_text) => {
                        if actual_text != expected {
                            warn!(
                                expected_len = expected.len(),
                                actual_len = actual_text.len(),
                                "Post-write clipboard text mismatch; enabling Windows Unicode fallback"
                            );
                            needs_fallback = true;
                        }
                    }
                    Err(err) => {
                        warn!(
                            error = %err,
                            expected_len = expected.len(),
                            "Post-write clipboard text read failed; enabling Windows Unicode fallback"
                        );
                        needs_fallback = true;
                    }
                }
            }
            drop(ctx);

            if needs_fallback {
                if let Some(text) = expected_text.as_deref() {
                    write_text_windows_native(text)?;
                    info!("Rewrote clipboard text via Windows Unicode fallback after verification");
                }
            }

            info!("Wrote clipboard snapshot to system");
            Ok(())
        })
    }
}

fn extract_text_plain_utf8(snapshot: &SystemClipboardSnapshot) -> Result<Option<String>> {
    let maybe_text_rep = snapshot.representations.iter().find(|rep| {
        rep.mime
            .as_ref()
            .is_some_and(|mime| mime.as_str().eq_ignore_ascii_case("text/plain"))
    });

    let Some(text_rep) = maybe_text_rep else {
        return Ok(None);
    };

    let text = String::from_utf8(text_rep.bytes.clone())
        .map_err(|err| anyhow::anyhow!("Failed to decode text/plain snapshot as UTF-8: {}", err))?;
    Ok(Some(text))
}

fn is_single_text_plain_snapshot(snapshot: &SystemClipboardSnapshot) -> bool {
    if snapshot.representations.len() != 1 {
        return false;
    }

    snapshot.representations[0]
        .mime
        .as_ref()
        .is_some_and(|mime| mime.as_str().eq_ignore_ascii_case("text/plain"))
}

fn write_text_windows_native(text: &str) -> Result<()> {
    clipboard_win::set_clipboard_string(text)
        .map_err(|e| anyhow::anyhow!("Failed to set Windows Unicode clipboard text: {}", e))
}

/// Windows-specific: Read image from clipboard as Bitmap format
fn read_image_windows() -> Result<Vec<u8>> {
    use clipboard_win::{formats, get_clipboard};
    let data = get_clipboard(formats::Bitmap)
        .map_err(|e| anyhow::anyhow!("Failed to get image from clipboard: {}", e))?;

    // Convert BMP to PNG for consistency with other platforms
    let image = image::load_from_memory_with_format(&data, image::ImageFormat::Bmp)
        .map_err(|e| anyhow::anyhow!("Failed to decode BMP: {}", e))?;
    let rgba_image = image.to_rgba8();

    Ok(rgba_image.to_vec())
}

/// Windows-specific: Write image to clipboard as Bitmap format
fn write_image_windows(bytes: &[u8]) -> Result<()> {
    use clipboard_win::{empty, formats, set_clipboard};
    use std::ptr::null_mut;
    use winapi::um::winuser::{CloseClipboard, GetOpenClipboardWindow, OpenClipboard};

    // Decode image bytes
    let img = image::load_from_memory(bytes)
        .map_err(|e| anyhow::anyhow!("Failed to decode image: {}", e))?;

    // Convert to BMP format with proper header
    let bmp_bytes = to_bitmap(&img);

    // Retry mechanism to open clipboard (max 5 attempts, 10ms delay)
    let mut retry_count = 0;
    while retry_count < 5 {
        unsafe {
            if OpenClipboard(null_mut()) != 0 {
                // Successfully opened clipboard
                break;
            }
            // If clipboard is opened by another process, wait and retry
            if GetOpenClipboardWindow() != null_mut() {
                std::thread::sleep(std::time::Duration::from_millis(10));
                retry_count += 1;
            } else {
                return Err(anyhow::anyhow!("Failed to open clipboard"));
            }
        }
    }

    if retry_count == 5 {
        return Err(anyhow::anyhow!(
            "Failed to open clipboard after multiple attempts"
        ));
    }

    // Clear clipboard and set new content
    let clear_result = empty();
    let set_result = set_clipboard(formats::Bitmap, &bmp_bytes);

    // Close clipboard
    unsafe {
        CloseClipboard();
    }

    clear_result.map_err(|e| anyhow::anyhow!("Failed to clear clipboard: {}", e))?;
    set_result.map_err(|e| anyhow::anyhow!("Failed to set clipboard: {}", e))?;

    Ok(())
}

/// Convert image to BMP format (Windows Bitmap)
/// Generates BMP file header + info header + pixel data
fn to_bitmap(img: &image::DynamicImage) -> Vec<u8> {
    use image::GenericImageView;

    // Flip image vertically because BMP scan lines are stored bottom to top
    let img = img.flipv();

    // Generate the 54-byte header
    let mut byte_vec = get_bmp_header(img.width(), img.height());

    // Add pixel data (BGRA format)
    for (_, _, pixel) in img.pixels() {
        let pixel_bytes = pixel.0;
        byte_vec.push(pixel_bytes[2]); // B
        byte_vec.push(pixel_bytes[1]); // G
        byte_vec.push(pixel_bytes[0]); // R
        byte_vec.push(pixel_bytes[3]); // A (unused in BMP spec but included)
    }

    byte_vec
}

/// Generate BMP file header and info header (54 bytes total)
fn get_bmp_header(width: u32, height: u32) -> Vec<u8> {
    let mut vec = vec![0; 54];

    // BM signature
    vec[0] = 66; // 'B'
    vec[1] = 77; // 'M'

    // File size
    let file_size = width * height * 4 + 54;
    set_bytes(&mut vec, &file_size.to_le_bytes(), 2..6);

    // Reserved (unused)
    set_bytes(&mut vec, &0_u32.to_le_bytes(), 6..10);

    // Offset to pixel data
    let offset = 54_u32;
    set_bytes(&mut vec, &offset.to_le_bytes(), 10..14);

    // Info header size
    let header_size = 40_u32;
    set_bytes(&mut vec, &header_size.to_le_bytes(), 14..18);

    // Width
    set_bytes(&mut vec, &width.to_le_bytes(), 18..22);

    // Height
    set_bytes(&mut vec, &height.to_le_bytes(), 22..26);

    // Planes (must be 1)
    let planes = 1_u16;
    set_bytes(&mut vec, &planes.to_le_bytes(), 26..28);

    // Bits per pixel (32 bits for BGRA)
    let bits_per_pixel = 32_u16;
    set_bytes(&mut vec, &bits_per_pixel.to_le_bytes(), 28..30);

    // Compression (0 = no compression)
    set_bytes(&mut vec, &0_u32.to_le_bytes(), 30..34);

    // Compressed size (0 when no compression)
    set_bytes(&mut vec, &0_u32.to_le_bytes(), 34..38);

    // Horizontal resolution (0 is allowed)
    set_bytes(&mut vec, &0_u32.to_le_bytes(), 38..42);

    // Vertical resolution (0 is allowed)
    set_bytes(&mut vec, &0_u32.to_le_bytes(), 42..46);

    // Colors used (0 is allowed)
    set_bytes(&mut vec, &0_u32.to_le_bytes(), 46..50);

    // Important colors (0 is allowed)
    set_bytes(&mut vec, &0_u32.to_le_bytes(), 50..54);

    vec
}

/// Helper to set bytes in a slice at a specific range
fn set_bytes(to: &mut [u8], from: &[u8], range: Range<usize>) {
    for (from_idx, i) in range.enumerate() {
        to[i] = from[from_idx];
    }
}
