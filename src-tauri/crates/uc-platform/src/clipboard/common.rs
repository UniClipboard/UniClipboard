use anyhow::{anyhow, Result};
use clipboard_rs::{common::RustImage, Clipboard, ContentFormat};
use tracing::{debug, info, warn};
use uc_core::clipboard::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};
use uc_core::ids::RepresentationId;

/// 文件头魔数嗅探,返回桌面剪贴板能消费的 `image/*` mime 字符串。
/// 无法识别返回 None。只读前 12 字节,无内存分配。
pub(crate) fn sniff_image_magic(body: &[u8]) -> Option<&'static str> {
    if body.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if body.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if body.starts_with(b"GIF87a") || body.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if body.len() >= 12 && body.starts_with(b"RIFF") && &body[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if body.starts_with(&[0x42, 0x4D]) {
        return Some("image/bmp");
    }
    if body.starts_with(&[0x49, 0x49, 0x2A, 0x00]) || body.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]) {
        return Some("image/tiff");
    }
    None
}

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
    /// Maximum number of retry attempts when clipboard data is declared but not
    /// yet readable (macOS lazy/promised data providers).
    ///
    /// On macOS, apps (especially browsers) can declare pasteboard types via
    /// `setDataProvider:forTypes:` before the data is actually resolved. Our
    /// watcher may detect the `changeCount` change and try to read before the
    /// data provider has fulfilled the promise. A short retry closes this gap.
    const LAZY_DATA_MAX_RETRIES: u32 = 2;

    /// Delay between retry attempts (milliseconds).
    const LAZY_DATA_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(50);

    pub fn read_snapshot(
        ctx: &mut clipboard_rs::ClipboardContext,
    ) -> Result<SystemClipboardSnapshot> {
        for attempt in 0..=Self::LAZY_DATA_MAX_RETRIES {
            let (snapshot, had_unreadable_format) = Self::read_snapshot_once(ctx)?;

            if !had_unreadable_format || attempt == Self::LAZY_DATA_MAX_RETRIES {
                if attempt > 0 && !had_unreadable_format {
                    info!(
                        attempt = attempt + 1,
                        representations = snapshot.representations.len(),
                        "Clipboard retry succeeded"
                    );
                }
                return Ok(snapshot);
            }

            warn!(
                attempt = attempt + 1,
                max_retries = Self::LAZY_DATA_MAX_RETRIES,
                "Clipboard formats declared but data unreadable (lazy data provider?), retrying after delay"
            );
            std::thread::sleep(Self::LAZY_DATA_RETRY_DELAY);
        }
        unreachable!()
    }

    /// Perform a single attempt to read the clipboard snapshot.
    ///
    /// Returns `(snapshot, had_unreadable_format)` where `had_unreadable_format`
    /// is true if at least one high-level format (text/rtf/html) was declared as
    /// available via `has()` but the actual data read failed.
    fn read_snapshot_once(
        ctx: &mut clipboard_rs::ClipboardContext,
    ) -> Result<(SystemClipboardSnapshot, bool)> {
        let available = map_clipboard_err(ctx.available_formats())?;
        debug!(formats = ?available, "Clipboard available formats");

        let mut reps = Vec::new();

        // Track whether any high-level format was declared available but
        // could not be read — the signal for a lazy data provider retry.
        let mut had_unreadable_format = false;

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
                    had_unreadable_format = true;
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
                    had_unreadable_format = true;
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
                    had_unreadable_format = true;
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
        // captured above) and any of those files look like image files, this
        // fallback may load their bytes as inline `image-from-file` reps so
        // that screenshot tools (which copy a temp PNG path) render as a real
        // image preview in the UI rather than showing only a filename.
        //
        // Contract (post 260426-1gz quick task):
        // - Only "small" image files (< MAX_INLINE_IMAGE_BYTES) become inline
        //   `image-from-file` reps for immediate UI preview. Daily screenshots
        //   from system tools are typically well under this threshold.
        // - Large image files are intentionally NOT inlined here. Locally the
        //   UI still has the `files` rep (filename / file list) and the
        //   `BackgroundBlobWorker.try_generate_thumbnail` path will populate
        //   `clipboard_representation_thumbnail` asynchronously, so the
        //   history list still gets a thumbnail. Cross-device transfer of the
        //   actual pixels is the responsibility of the file transfer / blob
        //   ref path, not this fallback.
        // - This guarantees that no single rep produced by capture exceeds
        //   the V3 envelope budget, preventing the
        //   `peer rejected: payload ... exceeds local maximum 2097152`
        //   dispatch failure observed when a >= 2 MiB PNG file URI is copied.
        if !image_already_read && !captured_file_paths.is_empty() {
            // Hard upper safety cap: never read an image file larger than this
            // even if the inline-budget check below were somehow bypassed.
            const MAX_IMAGE_FILE_BYTES: u64 = 20 * 1024 * 1024; // 20 MB
                                                                // Conservative inline budget: stays well under the 2 MiB envelope
                                                                // (`MAX_PAYLOAD_SIZE` in `clipboard_wire.rs`) so that even after
                                                                // V3 header + AEAD tag + co-existing reps (plain text, files,
                                                                // html, ...) the encrypted envelope fits with ~8x headroom.
                                                                // 256 KiB still covers typical screenshot-tool PNGs (usually
                                                                // < 200 KiB), so the immediate inline preview UX does not regress.
            const MAX_INLINE_IMAGE_BYTES: u64 = 256 * 1024; // 256 KiB

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
                if meta.len() > MAX_INLINE_IMAGE_BYTES {
                    // Intentionally do NOT read or push an inline rep here.
                    // The downstream `files` rep + BackgroundBlobWorker
                    // thumbnail path covers local UI; cross-device pixel
                    // transfer is handled by file transfer / blob ref. This
                    // prevents the snapshot from carrying a multi-MiB inline
                    // rep that would blow the V3 envelope budget.
                    //
                    // Note (per uc-platform AGENTS §12.3): only path/size/
                    // mime/threshold are logged — never any file bytes.
                    debug!(
                        path = %path.display(),
                        size_bytes = meta.len(),
                        mime = mime,
                        threshold = MAX_INLINE_IMAGE_BYTES,
                        "Skipping inline image-from-file rep (too large for envelope); files rep + background thumbnail will handle it"
                    );
                    break;
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

        Ok((
            SystemClipboardSnapshot {
                ts_ms: chrono::Utc::now().timestamp_millis(),
                representations: reps,
            },
            had_unreadable_format,
        ))
    }

    /// 写入 `SystemClipboardSnapshot` 到系统剪贴板。
    ///
    /// 分流策略：
    /// 1. `representations.len() == 1`：走 `clipboard-rs` 高层 API 快路径（跨平台）。
    ///    —— 由 `clipboard-rs` 封装 set_text / set_html / set_image / set_files 等，
    ///    行为与早期版本完全一致。
    /// 2. `representations.len() > 1`：进入 `write_snapshot_multi` 分流：
    ///    - Windows：原子多 rep 写入（`write_snapshot_multi_windows`）——在单次
    ///      `OpenClipboard` 会话内用 `raw::set_without_clear` 累加 CF_UNICODETEXT
    ///      + CF_HTML 等多个 format，确保纯文本目的地也能粘贴。
    ///    - macOS：原子多 rep 写入（`write_snapshot_multi_macos`）——在单次
    ///      `NSPasteboard::writeObjects:` 调用内提交 `NSPasteboardItem`，承载
    ///      `NSPasteboardTypeString` + `NSPasteboardTypeHTML`，与 Windows 语义对齐。
    ///    - Linux / 其他 Unix：暂不支持原子多 rep（Wayland `wl-clipboard-rs` 与 X11
    ///      selection owner 模型与 `clipboard-rs` 不兼容，工作量独立），当前降级为
    ///      "用 `SelectRepresentationPolicyV1` 选出 paste-priority rep 再走单 rep
    ///      快路径"并 warn 日志。后续 phase 补齐（FIXME 见分支内注释）。
    ///
    /// 历史背景见 https://github.com/UniClipboard/UniClipboard/issues/92
    /// 以及 `uc-platform/src/clipboard/platform/{windows,macos}.rs`。
    pub fn write_snapshot(
        ctx: &mut clipboard_rs::ClipboardContext,
        snapshot: SystemClipboardSnapshot,
    ) -> Result<()> {
        use anyhow::bail;

        if snapshot.representations.is_empty() {
            bail!("platform::write expects at least ONE representation, got 0");
        }

        // 多 rep 分流入口：Windows 走原子写入，其他平台显式降级（§9.3 不允许静默降级）。
        if snapshot.representations.len() > 1 {
            return Self::write_snapshot_multi(ctx, snapshot);
        }

        // 单 rep 快路径：走既有 clipboard-rs 高层 API（行为与改动前完全一致）。
        let rep = &snapshot.representations[0];

        // 先按 format_id 推断默认 mime。
        let format_default = match rep.format_id.as_str() {
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
        };

        // 决定 effective_mime:
        //   情况 A —— image-like format_id 但显式 mime 不是 image/*:几乎一定是客户端
        //     误标(典型:iOS / SyncClipboard 兼容客户端 PUT /file 时不设 Content-Type
        //     → 服务端默认 application/octet-stream → 这里 mime=Some("application/octet-stream"))。
        //     先字节嗅探拿真实图片 mime,失败再回退到 format_id 默认。**绝不**让 image rep
        //     携带 application/octet-stream 流到下游 set_image / set_buffer:
        //     2026-05-08 真机回归(IMG_20260508_200644.jpg)就是这条路径把 JPEG 字节
        //     用非法 NSPasteboard type 写进了系统剪贴板。
        //   情况 B —— 显式 mime 存在 → 沿用。
        //   情况 C —— 没有显式 mime → 回退 format_id 默认。
        let effective_mime: Option<&str> = match (rep.mime.as_deref(), format_default) {
            (Some(m), Some(default))
                if default.starts_with("image/") && !m.starts_with("image/") =>
            {
                let recovered = sniff_image_magic(&rep.bytes).unwrap_or(default);
                warn!(
                    format_id = %rep.format_id,
                    wire_mime = m,
                    recovered_mime = recovered,
                    "write_snapshot: image rep declared non-image mime; recovered via byte sniff/format_id default"
                );
                Some(recovered)
            }
            (Some(m), _) => Some(m),
            (None, default) => default,
        };

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
                // 兜底分支:effective_mime 没命中任何已支持类型。这条路径之前会
                // 静默 set_buffer(format_id, bytes) —— 历史教训(2026-05-08
                // IMG_20260508_200644.jpg 真机回归):image rep + mime
                // application/octet-stream 落到这里,把 JPEG 字节用非法
                // pasteboard type "image" 写进了系统剪贴板,既无法以图像形式
                // 粘贴,又把原始 EXIF 字节当文本暴露给用户。违反
                // `uc-platform/AGENTS.md` §11.2「不允许静默降级」。
                //
                // 现在:
                //   * image-like format_id 在 effective_mime 决策阶段已被字节
                //     嗅探纠正,不会再到达这里;若到达说明前置守卫失效 —— bail。
                //   * 其它未识别 mime 的 rep 仍走 set_buffer,但必须先 WARN,让
                //     "OS 剪贴板里写了非标准 type"这件事可观测。
                let format_default_image_like = matches!(
                    rep.format_id.as_str(),
                    "image" | "public.png" | "public.tiff" | "public.jpeg" | "public.gif"
                );
                if format_default_image_like {
                    anyhow::bail!(
                        "write_snapshot: image-like format_id {:?} reached fallback branch \
                         with mime {:?} — image-mime recovery should have run upstream; \
                         refusing to set_buffer with a non-UTI pasteboard type",
                        rep.format_id,
                        rep.mime
                    );
                }
                warn!(
                    format_id = %rep.format_id,
                    mime = ?rep.mime,
                    bytes = rep.bytes.len(),
                    "write_snapshot: writing rep via raw set_buffer fallback (no recognized mime mapping)"
                );
                map_clipboard_err(ctx.set_buffer(&rep.format_id, rep.bytes.clone()))?;
            }
        }

        Ok(())
    }

    /// 多 representation 写入入口。
    ///
    /// 平台能力差异：
    /// - Windows：具备真正的原子多 rep 写入（`write_snapshot_multi_windows`），在单次
    ///   `OpenClipboard` 会话内用 `raw::set_without_clear` 累加 CF_UNICODETEXT + CF_HTML，
    ///   确保纯文本目的地（记事本等）也能粘贴到正确内容。
    /// - macOS：具备真正的原子多 rep 写入（`write_snapshot_multi_macos`），在单次
    ///   `NSPasteboard::writeObjects:` 调用内提交 `NSPasteboardItem`。
    /// - Linux / 其他 Unix：当前尚未支持（需 `wl-clipboard-rs` DataSource 或 X11
    ///   selection owner 重写），本次降级为 "用 `SelectRepresentationPolicyV1` 选出
    ///   paste-priority rep 再走单 rep 快路径"，并以 `warn!` 日志显式说明。行为与
    ///   应用层原 `narrow_to_primary` 等价，保证 Linux 粘贴语义零回归。后续 phase
    ///   再统一（§9.3：不允许静默降级）。
    ///
    /// 注意：本方法不应被"单 rep 快路径"调用。调用者需保证 `snapshot.representations.len() >= 1`。
    fn write_snapshot_multi(
        ctx: &mut clipboard_rs::ClipboardContext,
        snapshot: SystemClipboardSnapshot,
    ) -> Result<()> {
        let rep_count = snapshot.representations.len();

        #[cfg(target_os = "windows")]
        {
            // 把实际实现委托给平台子模块。
            // common.rs 通过 `crate::clipboard::platform::windows::write_snapshot_multi_windows`
            // 调用，不跨 crate 暴露，符合 §4.4 cfg 收敛原则。
            return crate::clipboard::platform::windows::write_snapshot_multi_windows(snapshot);
        }

        #[cfg(target_os = "macos")]
        {
            // macOS：具备真正的原子多 rep 写入能力（NSPasteboardItem + writeObjects:）。
            // 实现在 `clipboard::platform::macos::write_snapshot_multi_macos`。
            // 该函数自己通过 `NSPasteboard::generalPasteboard()` 拿系统剪贴板单例，
            // 不使用传入的 clipboard-rs `ctx`，也不需要 "提前 drop + dummy_ctx" 的绕道
            //（与 Windows 不同：macOS NSPasteboard 不是独占句柄模型）。
            let _ = ctx; // 显式标注未使用，消除 unused-variable warning。
            let _ = rep_count; // macOS 分支不需要 rep_count，显式忽略。
            return crate::clipboard::platform::macos::write_snapshot_multi_macos(snapshot);
        }

        // Linux 与其他非 Windows / 非 macOS 的 Unix：显式降级（§9.3 不允许静默降级）。
        //
        // FIXME(260423-mxu-next-phase)：Linux 的真正多 rep 原子写入需要 Wayland
        // `wl-clipboard-rs` 的 DataSource 接口（多 MIME type 注册）或 X11 的
        // selection owner 持久持有模型；二者与 `clipboard-rs` 高层 API 不兼容，
        // 工作量与 macOS 相当，留到下一个独立 phase 补齐。本次保留以下 V1-policy
        // 降级逻辑，语义与 260423-9do 改造前完全一致，保证浏览器复制到 Linux 的
        // 粘贴行为不回归。
        #[cfg(any(
            target_os = "linux",
            not(any(target_os = "windows", target_os = "macos"))
        ))]
        {
            // 用 V1 policy 选出 paste-priority rep —— 与应用层原 `narrow_to_primary`
            // 等价。硬编码 V1：当前 uc-core 只有这一个 `SelectRepresentationPolicyPort`
            // 实现；出现 V2 时再考虑从调用方注入 policy。
            use uc_core::clipboard::SelectRepresentationPolicyV1;
            use uc_core::ports::SelectRepresentationPolicyPort;

            let policy = SelectRepresentationPolicyV1::default();
            let selection = policy
                .select(&snapshot)
                .map_err(|e| anyhow!("representation policy failed: {e}"))?;
            let paste_id = selection.paste_rep_id.clone();

            let chosen_idx = snapshot
                .representations
                .iter()
                .position(|rep| rep.id == paste_id)
                .ok_or_else(|| {
                    anyhow!(
                        "policy selected paste_rep_id {:?} not present in snapshot",
                        paste_id
                    )
                })?;

            warn!(
                rep_count,
                paste_rep_id = ?paste_id,
                chosen_format_id = %snapshot.representations[chosen_idx].format_id,
                "Linux: multi-representation atomic write not yet supported; \
                 falling back to single-rep path via SelectRepresentationPolicyV1 \
                 — will be addressed in a follow-up phase (wl-clipboard-rs / X11 \
                 selection owner)."
            );

            let ts_ms = snapshot.ts_ms;
            let mut reps = snapshot.representations;
            let chosen = reps.remove(chosen_idx);
            let reduced = SystemClipboardSnapshot {
                ts_ms,
                representations: vec![chosen],
            };
            return Self::write_snapshot(ctx, reduced);
        }
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

    // ─── sniff_image_magic ──────────────────────────────────────────────
    //
    // 守住 2026-05-08 IMG_20260508_200644.jpg 真机回归:image rep 携带
    // application/octet-stream 时,write_snapshot 必须能从字节嗅出真实
    // image/* mime,而不是把原始 JPEG 字节用非法 UTI 写进 NSPasteboard。

    #[test]
    fn sniff_image_magic_recognizes_jpeg() {
        // JPEG SOI + APP0 marker (real JFIF header)
        let bytes = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];
        assert_eq!(sniff_image_magic(&bytes), Some("image/jpeg"));
    }

    #[test]
    fn sniff_image_magic_recognizes_jpeg_with_exif_marker() {
        // 真机回归 case:Xiaomi 14 拍的 JPEG,SOI 后第二段是 APP1 (Exif)
        let bytes = [0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x18, b'E', b'x', b'i', b'f'];
        assert_eq!(sniff_image_magic(&bytes), Some("image/jpeg"));
    }

    #[test]
    fn sniff_image_magic_recognizes_png() {
        let bytes = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        ];
        assert_eq!(sniff_image_magic(&bytes), Some("image/png"));
    }

    #[test]
    fn sniff_image_magic_recognizes_gif() {
        assert_eq!(sniff_image_magic(b"GIF87a..."), Some("image/gif"));
        assert_eq!(sniff_image_magic(b"GIF89a..."), Some("image/gif"));
    }

    #[test]
    fn sniff_image_magic_recognizes_webp() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(b"WEBP");
        assert_eq!(sniff_image_magic(&bytes), Some("image/webp"));
    }

    #[test]
    fn sniff_image_magic_recognizes_bmp() {
        assert_eq!(sniff_image_magic(&[0x42, 0x4D, 0x00]), Some("image/bmp"));
    }

    #[test]
    fn sniff_image_magic_recognizes_tiff_both_endians() {
        assert_eq!(
            sniff_image_magic(&[0x49, 0x49, 0x2A, 0x00, 0x00]),
            Some("image/tiff")
        );
        assert_eq!(
            sniff_image_magic(&[0x4D, 0x4D, 0x00, 0x2A, 0x00]),
            Some("image/tiff")
        );
    }

    #[test]
    fn sniff_image_magic_returns_none_for_text_or_short_input() {
        assert_eq!(sniff_image_magic(b"hello world"), None);
        assert_eq!(sniff_image_magic(&[]), None);
        // RIFF without WEBP suffix (e.g. WAV) must not be misclassified.
        let mut riff_wav = Vec::new();
        riff_wav.extend_from_slice(b"RIFF");
        riff_wav.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        riff_wav.extend_from_slice(b"WAVE");
        assert_eq!(sniff_image_magic(&riff_wav), None);
    }
}
