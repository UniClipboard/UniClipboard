use super::super::common::CommonClipboardImpl;
use anyhow::Result;
use async_trait::async_trait;
use clipboard_rs::{Clipboard, ClipboardContext};
use std::ops::Range;
use std::sync::{Arc, Mutex};
use tracing::{debug, debug_span, error, info, warn};
use uc_core::clipboard::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};
use uc_core::ids::RepresentationId;
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
            let mut snapshot = CommonClipboardImpl::read_snapshot(&mut ctx)?;

            // Check if clipboard-rs already captured an image
            let has_image = snapshot.representations.iter().any(|rep| {
                rep.mime
                    .as_ref()
                    .is_some_and(|m| m.as_str().starts_with("image/"))
            });

            if has_image {
                debug!(
                    formats = snapshot.representations.len(),
                    total_size_bytes = snapshot.total_size_bytes(),
                    "Captured system clipboard snapshot (image via clipboard-rs)"
                );
                return Ok(snapshot);
            }

            // No image from clipboard-rs -- try Windows native fallback.
            // MUST drop the mutex guard before calling clipboard-win to avoid
            // double clipboard open (clipboard-rs may still hold it internally).
            drop(ctx);

            match read_image_windows_as_png() {
                Ok(png_bytes) => {
                    info!(
                        size_bytes = png_bytes.len(),
                        "Read image via Windows native CF_DIB fallback"
                    );
                    snapshot
                        .representations
                        .push(ObservedClipboardRepresentation::new(
                            RepresentationId::new(),
                            "image".into(),
                            Some(MimeType("image/png".to_string())),
                            png_bytes,
                        ));
                }
                Err(err) => {
                    // Not necessarily an error -- clipboard may genuinely have no image.
                    // Use debug level (not warn) to avoid log noise when user copies text.
                    debug!(error = %err, "Windows native image fallback unavailable");
                }
            }

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
            let text_fallback_eligible = is_single_text_plain_snapshot(&snapshot);
            let image_fallback_eligible = is_single_image_snapshot(&snapshot);
            let expected_text = if text_fallback_eligible {
                extract_text_plain_utf8(&snapshot)?
            } else {
                None
            };
            // Extract image bytes before passing snapshot to CommonClipboardImpl
            // (which consumes it by reference but we need the bytes for fallback).
            let image_bytes = if image_fallback_eligible {
                snapshot.representations.first().map(|rep| rep.bytes.clone())
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
                // Drop clipboard-rs context before native fallback to avoid double clipboard open
                drop(ctx);

                if text_fallback_eligible {
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
                }

                if image_fallback_eligible {
                    if let Some(bytes) = image_bytes.as_deref() {
                        warn!(
                            error = %err,
                            image_size = bytes.len(),
                            "Primary clipboard-rs image write failed; using Windows native Bitmap fallback"
                        );
                        write_image_windows(bytes)?;
                        info!("Wrote clipboard image via Windows native Bitmap fallback");
                        return Ok(());
                    }
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

fn is_single_image_snapshot(snapshot: &SystemClipboardSnapshot) -> bool {
    if snapshot.representations.len() != 1 {
        return false;
    }

    snapshot.representations[0]
        .mime
        .as_ref()
        .is_some_and(|mime| mime.as_str().starts_with("image/"))
}

fn write_text_windows_native(text: &str) -> Result<()> {
    clipboard_win::set_clipboard_string(text)
        .map_err(|e| anyhow::anyhow!("Failed to set Windows Unicode clipboard text: {}", e))
}

/// Windows-specific: Read image from clipboard as CF_DIB and convert to PNG bytes.
///
/// Uses `clipboard-win` to read raw CF_DIB data (BITMAPINFOHEADER + pixel data,
/// without the 14-byte BMP file header), then delegates to the cross-platform
/// `dib_to_png` converter.
fn read_image_windows_as_png() -> Result<Vec<u8>> {
    use clipboard_win::{formats, get_clipboard};

    let dib_data: Vec<u8> = get_clipboard(formats::RawData(formats::CF_DIB))
        .map_err(|e| anyhow::anyhow!("No DIB image on clipboard: {}", e))?;

    debug!(
        dib_size_bytes = dib_data.len(),
        "Read CF_DIB from Windows clipboard"
    );
    crate::clipboard::image_convert::dib_to_png(&dib_data)
}

/// Windows-specific: Write image to clipboard as CF_DIB format.
///
/// Uses clipboard-win's `Clipboard` struct for explicit open/close control
/// with retry logic, avoiding the OSError(1418) failures seen with
/// clipboard-rs's set_image() on Windows.
///
/// Accepts raw image bytes in any format supported by the `image` crate
/// (PNG, TIFF, JPEG, BMP, etc.), decodes them, and writes as CF_DIB
/// (BITMAPINFOHEADER + pixel data, without 14-byte BMP file header).
fn write_image_windows(bytes: &[u8]) -> Result<()> {
    use clipboard_win::{formats, Clipboard as ClipboardWin, Setter};

    // Decode image bytes (supports PNG, TIFF, JPEG, BMP, etc. via `image` crate)
    let img = image::load_from_memory(bytes)
        .map_err(|e| anyhow::anyhow!("Failed to decode image for Windows native write: {}", e))?;

    // Convert to full BMP format then strip the 14-byte file header to get CF_DIB data.
    // CF_DIB = BITMAPINFOHEADER (40 bytes) + pixel data (no BMP file header).
    let bmp_bytes = to_bitmap(&img);
    let dib_bytes = &bmp_bytes[14..]; // Skip BITMAPFILEHEADER (14 bytes)

    // Use clipboard-win's Clipboard struct with retry (up to 10 attempts).
    // This handles OpenClipboard/EmptyClipboard/CloseClipboard atomically.
    let _clip = ClipboardWin::new_attempts(10)
        .map_err(|e| anyhow::anyhow!("Failed to open clipboard for image write: {}", e))?;

    clipboard_win::raw::set(formats::CF_DIB, dib_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to write CF_DIB to clipboard: {}", e))?;

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
