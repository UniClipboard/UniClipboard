use anyhow::{anyhow, ensure, Result};
use clipboard_rs::{common::RustImage, Clipboard, ContentFormat};
use tracing::{debug, warn};
use uc_core::clipboard::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};
use uc_core::ids::RepresentationId;

/// Known TIFF UTI aliases on macOS pasteboard.
/// When the image has already been captured via the fast raw-TIFF path,
/// these formats must be skipped in the raw fallback loop to avoid
/// reading the same TIFF data a second time.
#[cfg(target_os = "macos")]
const TIFF_ALIASES: &[&str] = &["public.tiff", "NeXT TIFF v4.0 pasteboard type"];

pub struct CommonClipboardImpl;

fn should_skip_raw_format(format_id: &str, image_already_read: bool) -> bool {
    // Barrier writes this ownership marker during clipboard handoff.
    // It is not user clipboard content and should never be persisted.
    if format_id.eq_ignore_ascii_case("BarrierOwnership") {
        return true;
    }

    #[cfg(target_os = "windows")]
    {
        // Windows standard text-related formats are already handled by high-level
        // clipboard APIs (get_text/get_rich_text/get_html). Attempting raw buffer
        // reads for these often returns transient OSError(1168), which is noisy
        // and not actionable for sync correctness.
        if format_id.eq_ignore_ascii_case("CF_UNICODETEXT")
            || format_id.eq_ignore_ascii_case("CF_TEXT")
            || format_id.eq_ignore_ascii_case("CF_OEMTEXT")
            || format_id.eq_ignore_ascii_case("CF_LOCALE")
        {
            return true;
        }
    }

    // On macOS, skip TIFF aliases in the raw fallback loop when the image
    // was already captured via the fast path (get_buffer("public.tiff") or
    // get_buffer("public.png") or get_image()).
    #[cfg(target_os = "macos")]
    {
        if image_already_read {
            for alias in TIFF_ALIASES {
                if format_id == *alias {
                    return true;
                }
            }
        }
    }

    // Suppress unused-variable warning on non-macOS.
    let _ = image_already_read;

    false
}

fn map_clipboard_err<T>(
    result: std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>,
) -> Result<T> {
    result.map_err(|e| anyhow!(e))
}

impl CommonClipboardImpl {
    pub fn read_snapshot(
        ctx: &mut clipboard_rs::ClipboardContext,
    ) -> Result<SystemClipboardSnapshot> {
        let available = map_clipboard_err(ctx.available_formats())?;
        debug!(formats = ?available, "Clipboard available formats");

        let mut reps = Vec::new();

        if ctx.has(ContentFormat::Text) {
            match ctx.get_text() {
                Ok(text) => {
                    let bytes = text.into_bytes();
                    debug!(
                        format_id = "text",
                        size_bytes = bytes.len(),
                        "Read text representation"
                    );
                    reps.push(ObservedClipboardRepresentation::new(
                        RepresentationId::new(),
                        "text".into(),
                        Some(MimeType::text_plain()),
                        bytes,
                    ));
                }
                Err(err) => {
                    warn!(error = %err, "Failed to read text representation");
                }
            }
        }

        if ctx.has(ContentFormat::Rtf) {
            match ctx.get_rich_text() {
                Ok(rtf) => {
                    let bytes = rtf.into_bytes();
                    debug!(
                        format_id = "rtf",
                        size_bytes = bytes.len(),
                        "Read rtf representation"
                    );
                    reps.push(ObservedClipboardRepresentation::new(
                        RepresentationId::new(),
                        "rtf".into(),
                        Some(MimeType("text/rtf".to_string())),
                        bytes,
                    ));
                }
                Err(err) => {
                    warn!(error = %err, "Failed to read rtf representation");
                }
            }
        }

        if ctx.has(ContentFormat::Html) {
            match ctx.get_html() {
                Ok(html) => {
                    let bytes = html.into_bytes();
                    debug!(
                        format_id = "html",
                        size_bytes = bytes.len(),
                        "Read html representation"
                    );
                    reps.push(ObservedClipboardRepresentation::new(
                        RepresentationId::new(),
                        "html".into(),
                        Some(MimeType::text_html()),
                        bytes,
                    ));
                }
                Err(err) => {
                    warn!(error = %err, "Failed to read html representation");
                }
            }
        }

        if ctx.has(ContentFormat::Files) {
            match ctx.get_files() {
                Ok(files) => {
                    let bytes = files.join("\n").into_bytes();
                    debug!(
                        format_id = "files",
                        size_bytes = bytes.len(),
                        "Read files representation"
                    );
                    reps.push(ObservedClipboardRepresentation::new(
                        RepresentationId::new(),
                        "files".into(),
                        Some(MimeType("text/uri-list".to_string())),
                        bytes,
                    ));
                }
                Err(err) => {
                    warn!(error = %err, "Failed to read files representation");
                }
            }
        }

        // Track whether we successfully read image data via the high-level path.
        // Used to skip TIFF aliases in the raw fallback loop on macOS.
        let mut image_already_read = false;

        if ctx.has(ContentFormat::Image) {
            debug!("clipboard-rs reports ContentFormat::Image available");

            // macOS fast path: read raw TIFF directly via get_buffer, avoiding
            // the expensive decode+re-encode through get_image()+to_png().
            #[cfg(target_os = "macos")]
            {
                let mut captured = false;

                // Try raw TIFF first (fastest — no decode, no re-encode)
                match ctx.get_buffer("public.tiff") {
                    Ok(tiff_bytes) => {
                        debug!(
                            format_id = "image",
                            size_bytes = tiff_bytes.len(),
                            mime = "image/tiff",
                            "Read image representation via raw public.tiff (fast path)"
                        );
                        reps.push(ObservedClipboardRepresentation::new(
                            RepresentationId::new(),
                            "image".into(),
                            Some(MimeType("image/tiff".to_string())),
                            tiff_bytes,
                        ));
                        captured = true;
                    }
                    Err(err) => {
                        debug!(error = %err, "public.tiff not available, trying public.png");
                    }
                }

                // Fallback: try raw PNG
                if !captured {
                    match ctx.get_buffer("public.png") {
                        Ok(png_bytes) => {
                            debug!(
                                format_id = "image",
                                size_bytes = png_bytes.len(),
                                mime = "image/png",
                                "Read image representation via raw public.png"
                            );
                            reps.push(ObservedClipboardRepresentation::new(
                                RepresentationId::new(),
                                "image".into(),
                                Some(MimeType("image/png".to_string())),
                                png_bytes,
                            ));
                            captured = true;
                        }
                        Err(err) => {
                            debug!(error = %err, "public.png not available, falling back to get_image()");
                        }
                    }
                }

                // Final fallback: get_image() + to_png() (slow path — for apps
                // that only provide NSImage without raw TIFF/PNG buffers)
                if !captured {
                    match ctx.get_image() {
                        Ok(img) => {
                            debug!(
                                "clipboard-rs get_image() succeeded, converting to PNG (slow path)"
                            );
                            match img.to_png() {
                                Ok(png) => {
                                    let bytes = png.get_bytes().to_vec();
                                    debug!(
                                        format_id = "image",
                                        size_bytes = bytes.len(),
                                        "Read image representation via clipboard-rs get_image()+to_png()"
                                    );
                                    reps.push(ObservedClipboardRepresentation::new(
                                        RepresentationId::new(),
                                        "image".into(),
                                        Some(MimeType("image/png".to_string())),
                                        bytes,
                                    ));
                                    captured = true;
                                }
                                Err(err) => {
                                    warn!(error = %err, "clipboard-rs: image available but to_png() failed");
                                }
                            }
                        }
                        Err(err) => {
                            warn!(error = %err, "clipboard-rs: ContentFormat::Image reported available but get_image() failed");
                        }
                    }
                }

                image_already_read = captured;
            }

            // Non-macOS: keep original get_image()+to_png() path
            #[cfg(not(target_os = "macos"))]
            {
                match ctx.get_image() {
                    Ok(img) => {
                        debug!("clipboard-rs get_image() succeeded, converting to PNG");
                        match img.to_png() {
                            Ok(png) => {
                                let bytes = png.get_bytes().to_vec();
                                debug!(
                                    format_id = "image",
                                    size_bytes = bytes.len(),
                                    "Read image representation via clipboard-rs"
                                );
                                reps.push(ObservedClipboardRepresentation::new(
                                    RepresentationId::new(),
                                    "image".into(),
                                    Some(MimeType("image/png".to_string())),
                                    bytes,
                                ));
                                image_already_read = true;
                            }
                            Err(err) => {
                                warn!(error = %err, "clipboard-rs: image available but to_png() failed");
                            }
                        }
                    }
                    Err(err) => {
                        warn!(error = %err, "clipboard-rs: ContentFormat::Image reported available but get_image() failed");
                    }
                }
            }
        } else {
            // Log at debug level -- this is normal when clipboard has only text
            debug!("clipboard-rs reports no ContentFormat::Image available");
        }

        // raw fallback
        use std::collections::HashSet;
        let seen: HashSet<String> = reps.iter().map(|r| r.format_id.to_string()).collect();

        for format_id in available {
            if seen.contains(&format_id) {
                continue;
            }
            if should_skip_raw_format(&format_id, image_already_read) {
                debug!(format_id = %format_id, "Skipping raw buffer representation");
                continue;
            }
            match ctx.get_buffer(&format_id) {
                Ok(buf) => {
                    debug!(
                        format_id = %format_id,
                        size_bytes = buf.len(),
                        "Read raw buffer representation"
                    );
                    reps.push(ObservedClipboardRepresentation::new(
                        RepresentationId::new(),
                        format_id.into(),
                        None,
                        buf,
                    ));
                }
                Err(err) => {
                    warn!(
                        format_id = %format_id,
                        error = %err,
                        "Failed to read raw buffer representation"
                    );
                }
            }
        }

        Ok(SystemClipboardSnapshot {
            ts_ms: chrono::Utc::now().timestamp_millis(),
            representations: reps,
        })
    }

    /// TODO(clipboard/multi-representation):
    ///
    /// This implementation writes clipboard content via `clipboard-rs` high-level APIs,
    /// which implicitly overwrite the clipboard on each call.
    ///
    /// As a result, **multiple representations cannot be written as a single clipboard item**.
    /// Only the last written representation is reliably preserved.
    ///
    /// This is acceptable for now, but it prevents high-fidelity restore of clipboard snapshots
    /// that contain multiple representations (e.g. text + html + rtf + private formats).
    ///
    /// Proper support requires a platform-specific implementation that:
    /// - Constructs a single clipboard item
    /// - Attaches multiple representations to that item
    /// - Commits it atomically (e.g. `NSPasteboardItem` on macOS)
    ///
    /// Tracked in: https://github.com/UniClipboard/UniClipboard/issues/92
    pub fn write_snapshot(
        ctx: &mut clipboard_rs::ClipboardContext,
        snapshot: SystemClipboardSnapshot,
    ) -> Result<()> {
        #[cfg(debug_assertions)]
        {
            if snapshot.representations.len() > 1 {
                eprintln!(
                    "warning: writing {} clipboard representations via clipboard-rs; \
             multi-representation restore is lossy in current implementation",
                    snapshot.representations.len()
                );
            }
        }

        ensure!(
            snapshot.representations.len() == 1,
            "platform::write expects exactly ONE representation"
        );

        let rep = &snapshot.representations[0];

        match rep.mime.as_ref().map(|m| m.as_str()) {
            Some("text/plain") => {
                map_clipboard_err(ctx.set_text(String::from_utf8(rep.bytes.clone())?))?;
            }
            Some("text/rtf") => {
                map_clipboard_err(ctx.set_rich_text(String::from_utf8(rep.bytes.clone())?))?;
            }
            Some("text/html") => {
                map_clipboard_err(ctx.set_html(String::from_utf8(rep.bytes.clone())?))?;
            }
            Some("text/uri-list") | Some("file/uri-list") => {
                let files = String::from_utf8(rep.bytes.clone())?
                    .lines()
                    .map(|s| s.to_string())
                    .collect();
                map_clipboard_err(ctx.set_files(files))?;
            }
            Some("image/png") => {
                let img =
                    clipboard_rs::RustImageData::from_bytes(&rep.bytes).map_err(|e| anyhow!(e))?;
                map_clipboard_err(ctx.set_image(img))?;
            }
            _ => {
                map_clipboard_err(ctx.set_buffer(&rep.format_id, rep.bytes.clone()))?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_skip_barrier_ownership_regardless_of_image_flag() {
        assert!(should_skip_raw_format("BarrierOwnership", false));
        assert!(should_skip_raw_format("BarrierOwnership", true));
        assert!(should_skip_raw_format("barrierownership", true));
    }

    #[test]
    fn should_skip_tiff_aliases_when_image_already_read() {
        // On macOS, TIFF aliases should be skipped when image was already captured.
        #[cfg(target_os = "macos")]
        {
            assert!(should_skip_raw_format("public.tiff", true));
            assert!(should_skip_raw_format(
                "NeXT TIFF v4.0 pasteboard type",
                true
            ));
        }

        // On non-macOS, these are never skipped by the TIFF alias logic.
        #[cfg(not(target_os = "macos"))]
        {
            assert!(!should_skip_raw_format("public.tiff", true));
            assert!(!should_skip_raw_format(
                "NeXT TIFF v4.0 pasteboard type",
                true
            ));
        }
    }

    #[test]
    fn should_not_skip_tiff_aliases_when_image_not_read() {
        // When no image was captured, TIFF aliases should NOT be skipped
        // (they might be the only representation of image data).
        assert!(!should_skip_raw_format("public.tiff", false));
        assert!(!should_skip_raw_format(
            "NeXT TIFF v4.0 pasteboard type",
            false
        ));
    }

    #[test]
    fn should_not_skip_unrelated_formats() {
        assert!(!should_skip_raw_format(
            "org.nspasteboard.AutoGeneratedPasteboard",
            false
        ));
        assert!(!should_skip_raw_format(
            "org.nspasteboard.AutoGeneratedPasteboard",
            true
        ));
        assert!(!should_skip_raw_format("com.apple.finder.node", true));
    }
}
