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

/// macOS pasteboard formats that embed entire page resources (images, CSS, JS)
/// and can bloat a simple two-character browser copy to 20 MB+.
/// The useful content (text, HTML, RTF) is already captured by the high-level
/// clipboard APIs, so these archive formats are pure overhead for sync.
#[cfg(target_os = "macos")]
const WEBARCHIVE_FORMATS: &[&str] = &["com.apple.webarchive", "Apple Web Archive pasteboard type"];

/// Convert TIFF bytes to PNG, returning the PNG bytes.
///
/// macOS clipboard stores images as raw uncompressed TIFF (~18 MB for a
/// 3000x2000 image). Converting to PNG at capture time reduces payload
/// by 80-90%, dramatically improving sync speed to other platforms.
///
/// Returns `None` if conversion fails (caller should fall back to raw TIFF).
#[cfg(target_os = "macos")]
fn tiff_to_png(tiff_bytes: &[u8]) -> Option<Vec<u8>> {
    use std::io::Cursor;
    use tracing::info;

    let img = match image::load_from_memory_with_format(tiff_bytes, image::ImageFormat::Tiff) {
        Ok(img) => img,
        Err(err) => {
            warn!(error = %err, "Failed to decode TIFF for PNG conversion");
            return None;
        }
    };

    let mut png_bytes = Vec::new();
    match img.write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png) {
        Ok(()) => {
            info!(
                tiff_size = tiff_bytes.len(),
                png_size = png_bytes.len(),
                ratio = format!(
                    "{:.1}%",
                    (png_bytes.len() as f64 / tiff_bytes.len() as f64) * 100.0
                ),
                "Converted TIFF to PNG for sync"
            );
            Some(png_bytes)
        }
        Err(err) => {
            warn!(error = %err, "Failed to encode PNG from TIFF");
            None
        }
    }
}

pub struct CommonClipboardImpl;

fn should_skip_raw_format(
    format_id: &str,
    image_already_read: bool,
    files_already_read: bool,
) -> bool {
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

        // Skip macOS file-URL formats in raw fallback when files were already
        // captured via the high-level ContentFormat::Files path. These raw
        // formats contain the same file paths, causing duplicate representations
        // and inflated file_transfer_count.
        if files_already_read
            && (format_id == "public.file-url" || format_id == "NSFilenamesPboardType")
        {
            return true;
        }

        // Web-archive formats embed full page resources (images, CSS, JS).
        // A two-character browser copy can produce a 20 MB+ web archive.
        // Text, HTML, and RTF are already captured by high-level APIs.
        for wa in WEBARCHIVE_FORMATS {
            if format_id.eq_ignore_ascii_case(wa) {
                return true;
            }
        }
    }

    // Suppress unused-variable warnings on non-macOS.
    let _ = image_already_read;
    let _ = files_already_read;

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

        // Track file paths captured via ContentFormat::Files. After the image
        // block runs, we use these to opportunistically load image bytes when
        // the clipboard only contains a file reference to an image (e.g. many
        // screenshot tools write a temporary PNG file and copy its path).
        let mut captured_file_paths: Vec<std::path::PathBuf> = Vec::new();

        if ctx.has(ContentFormat::Files) {
            match ctx.get_files() {
                Ok(files) => {
                    // clipboard-rs returns raw OS paths (e.g. "C:\Users\mark\file.jpg" on Windows).
                    // Normalize to file:// URIs so downstream `extract_file_paths_from_snapshot`
                    // can parse them on all platforms via url::Url::parse().
                    let paths: Vec<std::path::PathBuf> =
                        files.iter().map(std::path::PathBuf::from).collect();
                    let uris: Vec<String> = paths
                        .iter()
                        .filter_map(|path| {
                            url::Url::from_file_path(path).ok().map(|u| u.to_string())
                        })
                        .collect();
                    let bytes = uris.join("\n").into_bytes();
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
                    captured_file_paths = paths;
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

                // Try raw TIFF first, then convert to PNG for efficient sync.
                // Raw TIFF is ~18 MB for a 3000x2000 image; PNG is ~2-5 MB.
                match ctx.get_buffer("public.tiff") {
                    Ok(tiff_bytes) => {
                        debug!(
                            format_id = "image",
                            tiff_size_bytes = tiff_bytes.len(),
                            "Read raw public.tiff from clipboard, converting to PNG"
                        );
                        match tiff_to_png(&tiff_bytes) {
                            Some(png_bytes) => {
                                reps.push(ObservedClipboardRepresentation::new(
                                    RepresentationId::new(),
                                    "image".into(),
                                    Some(MimeType("image/png".to_string())),
                                    png_bytes,
                                ));
                                captured = true;
                            }
                            None => {
                                // Conversion failed; fall back to raw TIFF
                                warn!(
                                    tiff_size_bytes = tiff_bytes.len(),
                                    "TIFF-to-PNG conversion failed, falling back to raw TIFF"
                                );
                                reps.push(ObservedClipboardRepresentation::new(
                                    RepresentationId::new(),
                                    "image".into(),
                                    Some(MimeType("image/tiff".to_string())),
                                    tiff_bytes,
                                ));
                                captured = true;
                            }
                        }
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

        // If the clipboard carried file references (but no image bytes were
        // captured above) and any of those files look like image files,
        // load their bytes and add them as image representations. This makes
        // screenshot tools that copy a temp PNG path render as a real image
        // preview in the UI rather than showing a filename.
        if !image_already_read && !captured_file_paths.is_empty() {
            // Safety cap to avoid blocking capture on huge files.
            const MAX_IMAGE_FILE_BYTES: u64 = 20 * 1024 * 1024; // 20 MB

            for path in &captured_file_paths {
                let ext = match path.extension().and_then(|e| e.to_str()) {
                    Some(e) => e.to_ascii_lowercase(),
                    None => continue,
                };
                let mime = match ext.as_str() {
                    "png" => "image/png",
                    "jpg" | "jpeg" => "image/jpeg",
                    "gif" => "image/gif",
                    "webp" => "image/webp",
                    "bmp" => "image/bmp",
                    "tif" | "tiff" => "image/tiff",
                    _ => continue,
                };
                let meta = match std::fs::metadata(path) {
                    Ok(m) => m,
                    Err(err) => {
                        warn!(
                            error = %err,
                            path = %path.display(),
                            "Failed to stat clipboard image file"
                        );
                        continue;
                    }
                };
                if meta.len() == 0 || meta.len() > MAX_IMAGE_FILE_BYTES {
                    debug!(
                        path = %path.display(),
                        size_bytes = meta.len(),
                        "Skipping clipboard image file (size out of range)"
                    );
                    continue;
                }
                match std::fs::read(path) {
                    Ok(bytes) => {
                        debug!(
                            path = %path.display(),
                            size_bytes = bytes.len(),
                            mime = mime,
                            "Loaded image bytes from clipboard file path"
                        );
                        reps.push(ObservedClipboardRepresentation::new(
                            RepresentationId::new(),
                            "image-from-file".into(),
                            Some(MimeType(mime.to_string())),
                            bytes,
                        ));
                        // One image representation is enough to drive the
                        // preview; avoid duplicating for multi-file selections.
                        break;
                    }
                    Err(err) => {
                        warn!(
                            error = %err,
                            path = %path.display(),
                            "Failed to read clipboard image file"
                        );
                    }
                }
            }
        }

        // raw fallback
        use std::collections::HashSet;
        let seen: HashSet<String> = reps.iter().map(|r| r.format_id.to_string()).collect();
        let files_already_read = seen.contains("files");

        for format_id in available {
            if seen.contains(&format_id) {
                continue;
            }
            if should_skip_raw_format(&format_id, image_already_read, files_already_read) {
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

        // Use explicit MIME if present, otherwise infer from macOS/cross-platform format_id.
        let effective_mime =
            rep.mime
                .as_ref()
                .map(|m| m.as_str())
                .or_else(|| match rep.format_id.as_str() {
                    "public.utf8-plain-text" | "public.text" | "NSStringPboardType" | "text" => {
                        Some("text/plain")
                    }
                    "public.html" | "Apple HTML pasteboard type" | "html" => Some("text/html"),
                    "public.rtf" | "rtf" => Some("text/rtf"),
                    "public.png" | "image" => Some("image/png"),
                    "public.tiff" => Some("image/tiff"),
                    "public.jpeg" => Some("image/jpeg"),
                    "public.file-url" | "NSFilenamesPboardType" => Some("text/uri-list"),
                    _ => None,
                });

        match effective_mime {
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
                // Convert file:// URIs back to raw OS paths for set_files(),
                // which expects native paths. Also handle raw paths for compatibility
                // with inbound cache paths that aren't URI-encoded.
                let files: Vec<String> = String::from_utf8(rep.bytes.clone())?
                    .lines()
                    .filter_map(|line| {
                        let line = line.trim();
                        if line.is_empty() {
                            return None;
                        }
                        // Try as file:// URI first
                        if let Ok(url) = url::Url::parse(line) {
                            if url.scheme() == "file" {
                                if let Ok(path) = url.to_file_path() {
                                    return Some(path.to_string_lossy().to_string());
                                }
                            }
                        }
                        // Fallback: treat as raw path
                        Some(line.to_string())
                    })
                    .collect();
                map_clipboard_err(ctx.set_files(files))?;
            }
            Some(mime) if mime.starts_with("image/") => {
                debug!(
                    mime = mime,
                    data_size = rep.bytes.len(),
                    format_id = %rep.format_id,
                    "write_snapshot: writing image to clipboard"
                );
                // On macOS, bypass clipboard-rs set_image() which does an unnecessary
                // decode → re-encode cycle (from_bytes → to_png). For large images this
                // re-encode can silently fail, leaving the clipboard empty after
                // clearContents(). Instead, write raw PNG bytes directly via set_buffer
                // with the "public.png" UTI (equivalent to NSPasteboardTypePNG).
                #[cfg(target_os = "macos")]
                {
                    if mime == "image/png" {
                        map_clipboard_err(ctx.set_buffer("public.png", rep.bytes.clone()))?;
                    } else {
                        // Non-PNG images still need format conversion via set_image
                        let img =
                            clipboard_rs::RustImageData::from_bytes(&rep.bytes).map_err(|e| {
                                warn!(
                                    mime = mime,
                                    data_size = rep.bytes.len(),
                                    error = %e,
                                    "write_snapshot: failed to decode image bytes"
                                );
                                anyhow!(e)
                            })?;
                        map_clipboard_err(ctx.set_image(img))?;
                    }
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let img = clipboard_rs::RustImageData::from_bytes(&rep.bytes).map_err(|e| {
                        warn!(
                            mime = mime,
                            data_size = rep.bytes.len(),
                            error = %e,
                            "write_snapshot: failed to decode image bytes"
                        );
                        anyhow!(e)
                    })?;
                    map_clipboard_err(ctx.set_image(img))?;
                }
                debug!(
                    mime = mime,
                    "write_snapshot: image set on system clipboard successfully"
                );
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
        assert!(should_skip_raw_format("BarrierOwnership", false, false));
        assert!(should_skip_raw_format("BarrierOwnership", true, false));
        assert!(should_skip_raw_format("barrierownership", true, false));
    }

    #[test]
    fn should_skip_tiff_aliases_when_image_already_read() {
        // On macOS, TIFF aliases should be skipped when image was already captured.
        #[cfg(target_os = "macos")]
        {
            assert!(should_skip_raw_format("public.tiff", true, false));
            assert!(should_skip_raw_format(
                "NeXT TIFF v4.0 pasteboard type",
                true,
                false
            ));
        }

        // On non-macOS, these are never skipped by the TIFF alias logic.
        #[cfg(not(target_os = "macos"))]
        {
            assert!(!should_skip_raw_format("public.tiff", true, false));
            assert!(!should_skip_raw_format(
                "NeXT TIFF v4.0 pasteboard type",
                true,
                false
            ));
        }
    }

    #[test]
    fn should_not_skip_tiff_aliases_when_image_not_read() {
        // When no image was captured, TIFF aliases should NOT be skipped
        // (they might be the only representation of image data).
        assert!(!should_skip_raw_format("public.tiff", false, false));
        assert!(!should_skip_raw_format(
            "NeXT TIFF v4.0 pasteboard type",
            false,
            false
        ));
    }

    #[test]
    fn should_not_skip_unrelated_formats() {
        assert!(!should_skip_raw_format(
            "org.nspasteboard.AutoGeneratedPasteboard",
            false,
            false
        ));
        assert!(!should_skip_raw_format(
            "org.nspasteboard.AutoGeneratedPasteboard",
            true,
            false
        ));
        assert!(!should_skip_raw_format(
            "com.apple.finder.node",
            true,
            false
        ));
    }

    #[test]
    fn should_skip_file_url_formats_when_files_already_read() {
        #[cfg(target_os = "macos")]
        {
            assert!(should_skip_raw_format("public.file-url", false, true));
            assert!(should_skip_raw_format("NSFilenamesPboardType", false, true));
        }

        #[cfg(not(target_os = "macos"))]
        {
            // On non-macOS, these are not skipped by the file-URL logic
            assert!(!should_skip_raw_format("public.file-url", false, true));
            assert!(!should_skip_raw_format(
                "NSFilenamesPboardType",
                false,
                true
            ));
        }
    }

    #[test]
    fn should_not_skip_file_url_formats_when_files_not_read() {
        assert!(!should_skip_raw_format("public.file-url", false, false));
        assert!(!should_skip_raw_format(
            "NSFilenamesPboardType",
            false,
            false
        ));
    }
}
