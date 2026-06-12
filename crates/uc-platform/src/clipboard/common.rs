use super::payload::rep_bytes;
use anyhow::{anyhow, Result};
use clipboard_rs::{common::RustImage, Clipboard, ContentFormat};
use tracing::{debug, info, warn};
#[cfg(target_os = "macos")]
use uc_core::clipboard::ImageKind;
use uc_core::clipboard::{
    MimeClass, MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot,
};
use uc_core::ids::RepresentationId;

use crate::clipboard::format_id_mime::format_id_default_mime;

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

/// Decide the "effective MIME" used by the single-rep write fast path.
///
/// This is the only function that needs to know how `format_id` and the
/// rep's declared `mime` field combine into a single canonical MIME for
/// the OS clipboard. Keeping it as a free function (no `&mut ClipboardContext`
/// dependency) means we can exhaustively unit-test every variant — the
/// regression that motivated this work (`text/plain;charset=utf-8` silently
/// missing the `text/plain` arm) belongs to this function's contract.
///
/// Three cases:
/// * `mime` present, `format_id` implies `image/*`, but `mime` is *not*
///   `image/*`: byte-sniff the payload (recovers iOS / SyncClipboard
///   uploads that omit Content-Type and get tagged `application/octet-stream`),
///   otherwise fall back to the format_id's default mime. **Never** let an
///   image rep flow through with a non-image mime — the 2026-05-08
///   IMG_20260508_200644.jpg incident took that path.
/// * `mime` present, anything else: use it as-is.
/// * `mime` absent: fall back to `format_id_default_mime`.
///
/// Returns `None` only when the rep has neither a mime nor a recognized
/// `format_id` — in that case the caller must refuse to write (§11.2,
/// no silent fallback to a non-UTI pasteboard type).
pub(crate) fn compute_effective_mime(rep: &ObservedClipboardRepresentation) -> Option<MimeType> {
    let format_default: Option<MimeType> = format_id_default_mime(rep.format_id.as_str());

    match (rep.mime.as_ref(), format_default.as_ref()) {
        (Some(m), Some(default)) if default.is_image() && !m.is_image() => {
            let recovered = rep_bytes(rep)
                .ok()
                .and_then(|b| sniff_image_magic(&b))
                .map(|s| MimeType(s.to_string()))
                .unwrap_or_else(|| default.clone());
            warn!(
                format_id = %rep.format_id,
                wire_mime = m.as_str(),
                recovered_mime = recovered.as_str(),
                "compute_effective_mime: image rep declared non-image mime; recovered via byte sniff/format_id default"
            );
            Some(recovered)
        }
        (Some(m), _) => Some(m.clone()),
        (None, _) => format_default,
    }
}

/// 基于文件后缀推断常见图片 MIME。仅用于 `image-from-file` LocalFile rep 的 mime 标注;
/// 与 `sniff_image_magic` 字节嗅探互补 —— 这里在抓取阶段不读字节,所以只能凭扩展名。
fn image_file_mime_from_path(path: &std::path::Path) -> Option<&'static str> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        _ => return None,
    })
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

/// 过滤剪贴板 `files` rep 中应当跨设备同步的路径。
///
/// 当前过滤策略:
/// 1. **零字节文件直接丢弃** —— 第三方剪贴板同步工具(例如网易 UU 远控的
///    `~/Library/Application Support/com.netease.uuremote/Clipboard/.uuremote_*`,
///    其他远控如 TeamViewer / 向日葵也观察到类似模式)会反复往本地目录写零字节占位
///    文件作为心跳/握手,这些文件没有真实负载,跨设备传输纯属噪音,且对端会被弹通知。
/// 2. **stat 失败的文件也丢弃** —— 拿不到元数据就无法保证文件依然存在或可读,跨网
///    传输有可能拿到部分写入的内容或触发对端的 ENOENT。
///
/// `size_lookup` 注入便于单元测试;生产环境传 `std::fs::metadata(p).map(|m| m.len())`。
fn filter_syncable_clipboard_files<F>(
    paths: Vec<std::path::PathBuf>,
    mut size_lookup: F,
) -> Vec<std::path::PathBuf>
where
    F: FnMut(&std::path::Path) -> std::io::Result<u64>,
{
    paths
        .into_iter()
        .filter_map(|path| match size_lookup(&path) {
            Ok(0) => {
                debug!(
                    path = %path.display(),
                    "Skipping zero-byte file in clipboard files rep (likely third-party clipboard-sync tool)"
                );
                None
            }
            Ok(_) => Some(path),
            Err(err) => {
                warn!(
                    error = %err,
                    path = %path.display(),
                    "Failed to stat clipboard file, skipping"
                );
                None
            }
        })
        .collect()
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
            // On Windows we deliberately bypass `ctx.get_html()`. The upstream
            // implementation does `data[start_idx..end_idx].to_string()` on the
            // CF_HTML payload using header byte offsets — and `std`'s string
            // slicer aborts the process when those offsets land inside a
            // multi-byte UTF-8 character. Some source apps (Chinese-language
            // Office, certain chat clients) ship payloads where that happens
            // for the trailing CJK character. See Sentry UNICLIPBOARD-RUST-1V
            // and the regression tests in `super::cf_html`.
            //
            // The Windows path reads raw CF_HTML bytes via `clipboard-win` and
            // slices on bytes, so a bad offset becomes a `U+FFFD` (or one
            // truncated char at the tail) instead of a panic.
            #[cfg(target_os = "windows")]
            let html_result: Result<String> =
                match super::platform::windows::read_html_windows_native() {
                    Ok(Some(html)) if !html.is_empty() => Ok(html),
                    Ok(Some(_)) | Ok(None) => {
                        Err(anyhow!("CF_HTML declared available but payload was empty"))
                    }
                    Err(err) => Err(err),
                };
            #[cfg(not(target_os = "windows"))]
            let html_result: Result<String> = ctx.get_html().map_err(|e| anyhow!(e));

            match html_result {
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
                    let raw_paths: Vec<std::path::PathBuf> =
                        files.iter().map(std::path::PathBuf::from).collect();
                    let raw_count = raw_paths.len();
                    let paths = filter_syncable_clipboard_files(raw_paths, |p| {
                        std::fs::metadata(p).map(|m| m.len())
                    });

                    if paths.is_empty() {
                        debug!(
                            raw_count,
                            "All clipboard files filtered out (zero-byte or unreadable); skipping files rep"
                        );
                    } else {
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
                            kept = paths.len(),
                            dropped = raw_count - paths.len(),
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
                }
                Err(err) => {
                    warn!(error = %err, "Failed to read files representation");
                }
            }
        }

        // macOS Finder thumbnail pollution guard.
        //
        // 当 captured_file_paths 含图片扩展名时,ContentFormat::Image 99% 是 Finder 在
        // "复制文件"时注入的文件图标缩略图(128/256 px),而不是用户复制的图片内容本身。
        // 跳过 ContentFormat::Image 分支,让下面的 image-from-file fallback 用 LocalFile
        // source 引用真实文件 —— 这样 dashboard / 跨设备同步看到的都是真图,不是缩略图。
        //
        // 不在 captured_file_paths 含图片扩展名时退化:即使 Image 与 Files 共存,Files 也
        // 可能是其他类型(PDF/ZIP)且 Image 真是用户复制的截图,不能误杀。所以仅当 file 列表
        // 里至少一个文件是图片扩展名才 suppress。
        #[cfg(target_os = "macos")]
        let suppress_image_due_to_file_thumbnail = captured_file_paths
            .iter()
            .any(|p| image_file_mime_from_path(p).is_some());
        #[cfg(not(target_os = "macos"))]
        let suppress_image_due_to_file_thumbnail = false;

        // Track whether we successfully read image data via the high-level path.
        // Used to skip TIFF aliases in the raw fallback loop on macOS.
        let mut image_already_read = false;

        if ctx.has(ContentFormat::Image) && suppress_image_due_to_file_thumbnail {
            info!(
                captured_files = captured_file_paths.len(),
                "macOS: ContentFormat::Image suppressed (looks like Finder file thumbnail); \
                 deferring to image-from-file LocalFile rep below"
            );
        } else if ctx.has(ContentFormat::Image) {
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

            // Windows: layered fast path that prefers the native "PNG"
            // clipboard format (zero encoding) before falling back to
            // CF_DIB+fast-PNG and ultimately to clipboard-rs's
            // decode+re-encode slow path. See
            // `super::platform::windows::try_read_image_windows_optimized`
            // for the rationale and tier breakdown.
            #[cfg(target_os = "windows")]
            {
                match super::platform::windows::try_read_image_windows_optimized(ctx) {
                    Ok(Some(bytes)) => {
                        debug!(
                            format_id = "image",
                            size_bytes = bytes.len(),
                            "Read image representation via Windows optimized path"
                        );
                        reps.push(ObservedClipboardRepresentation::new(
                            RepresentationId::new(),
                            "image".into(),
                            Some(MimeType("image/png".to_string())),
                            bytes,
                        ));
                        image_already_read = true;
                    }
                    Ok(None) => {
                        debug!(
                            "Windows optimized image read returned None; \
                             clipboard reports Image format but no tier produced bytes"
                        );
                        // Treat as transient unreadable so the lazy-data
                        // retry in `read_snapshot` gets a chance.
                        had_unreadable_format = true;
                    }
                    Err(err) => {
                        warn!(
                            error = %err,
                            "Windows optimized image read failed across all tiers"
                        );
                    }
                }
            }
        } else {
            // Log at debug level -- this is normal when clipboard has only text
            debug!("clipboard-rs reports no ContentFormat::Image available");
        }

        // 当 clipboard 携带文件引用且至少有一个是图片文件,produce 一条 `image-from-file`
        // rep —— 这条 rep 走 `ClipboardPayloadSource::LocalFile` 路径,**不读字节、不占内存**,
        // 由 capture pipeline 在 normalize 阶段同步调 `BlobWriter.write_path_if_absent` 物化
        // 到 blob 仓库(hardlink 优先,跨卷 fallback 流式 copy)。
        //
        // 任意大小的图片文件都能被收纳:dashboard 通过 `/clipboard/blobs/{blob_id}` 拿真实
        // 字节预览,跨设备同步通过 V3BlobRef + iroh-blobs 流式拉取。256 KiB 与 2 MiB envelope
        // 预算只约束 wire 路径的 inline rep,跟 LocalFile 物化路径无关。
        if !image_already_read && !captured_file_paths.is_empty() {
            // Sanity cap:超过此阈值的图片文件视为异常(可能用户误把磁盘镜像复制了),
            // 跳过登记,让 files rep 仍然能传文件本体。100 MB 对桌面截图场景留充裕空间。
            const MAX_IMAGE_FILE_BYTES: u64 = 100 * 1024 * 1024;

            for path in &captured_file_paths {
                let Some(mime) = image_file_mime_from_path(path) else {
                    continue;
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
                        threshold = MAX_IMAGE_FILE_BYTES,
                        "Skipping clipboard image file (size out of safe range)"
                    );
                    continue;
                }
                debug!(
                    path = %path.display(),
                    size_bytes = meta.len(),
                    mime = mime,
                    "Captured image-from-file rep as LocalFile source (no inline read; blob ingest happens during normalize)"
                );
                reps.push(ObservedClipboardRepresentation::new_local_file(
                    RepresentationId::new(),
                    "image-from-file".into(),
                    Some(MimeType(mime.to_string())),
                    path.clone(),
                    meta.len(),
                ));
                // 一个图片 rep 足以驱动 dashboard 预览;多文件选择时不重复产 rep。
                break;
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

        // 单 rep 快路径：走既有 clipboard-rs 高层 API。
        let rep = &snapshot.representations[0];

        let effective_mime = compute_effective_mime(rep);

        // 把单 rep 的字节预读到 owned `Vec<u8>`（Inline 转 owned, LocalFile 同步读盘）。
        // 后续各分支再从这份 owned 字节构造 String / RustImageData / set_buffer 输入,
        // 保证 LocalFile rep 也能走完单 rep 快路径（避免 `expect_inline_bytes` panic,
        // 见 `common::rep_bytes` 注释）。
        let single_rep_bytes = rep_bytes(rep)?.into_owned();

        // Refuse to write when we couldn't derive an effective mime — neither
        // the rep's declared mime nor its format_id maps to a known clipboard
        // category. Per `uc-platform/AGENTS.md` §11.2, raw set_buffer with a
        // non-UTI pasteboard type is a silent failure on macOS (the system
        // accepts the write but no application can Cmd+V it) and must be
        // surfaced rather than papered over.
        let Some(effective_mime) = effective_mime else {
            anyhow::bail!(
                "write_snapshot: no effective mime — rep has no declared mime and \
                 format_id {:?} is not in the platform mapping",
                rep.format_id
            );
        };

        match effective_mime.classify() {
            // All `text/*` reps that we can carry on the system clipboard
            // ultimately land on the OS string buffer. Markdown / csv /
            // x-url / etc are written as plain text — system pasteboards
            // don't have a richer slot for them, and pasting as plain text
            // is what every consumer expects.
            MimeClass::TextPlain
            | MimeClass::TextMarkdown
            | MimeClass::TextLink
            | MimeClass::TextOther => {
                map_clipboard_err(ctx.set_text(String::from_utf8(single_rep_bytes)?))?;
            }
            MimeClass::TextRtf => {
                map_clipboard_err(ctx.set_rich_text(String::from_utf8(single_rep_bytes)?))?;
            }
            MimeClass::TextHtml => {
                map_clipboard_err(ctx.set_html(String::from_utf8(single_rep_bytes)?))?;
            }
            MimeClass::UriList => {
                // Convert file:// URIs back to raw OS paths for set_files(),
                // which expects native paths. Also handle raw paths for compatibility
                // with inbound cache paths that aren't URI-encoded.
                let files: Vec<String> = String::from_utf8(single_rep_bytes)?
                    .lines()
                    .filter_map(|line| {
                        let line = line.trim();
                        if line.is_empty() {
                            return None;
                        }
                        if let Ok(url) = url::Url::parse(line) {
                            if url.scheme() == "file" {
                                if let Ok(path) = url.to_file_path() {
                                    return Some(path.to_string_lossy().to_string());
                                }
                            }
                        }
                        Some(line.to_string())
                    })
                    .collect();
                map_clipboard_err(ctx.set_files(files))?;
            }
            MimeClass::Image(kind) => {
                debug!(
                    mime = effective_mime.as_str(),
                    data_size = rep.size_bytes(),
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
                    if matches!(kind, ImageKind::Png) {
                        map_clipboard_err(ctx.set_buffer("public.png", single_rep_bytes))?;
                    } else {
                        let img = clipboard_rs::RustImageData::from_bytes(&single_rep_bytes)
                            .map_err(|e| {
                                warn!(
                                    mime = effective_mime.as_str(),
                                    data_size = rep.size_bytes(),
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
                    let _ = kind;
                    let img = clipboard_rs::RustImageData::from_bytes(&single_rep_bytes).map_err(
                        |e| {
                            warn!(
                                mime = effective_mime.as_str(),
                                data_size = rep.size_bytes(),
                                error = %e,
                                "write_snapshot: failed to decode image bytes"
                            );
                            anyhow!(e)
                        },
                    )?;
                    map_clipboard_err(ctx.set_image(img))?;
                }
                debug!(
                    mime = effective_mime.as_str(),
                    "write_snapshot: image set on system clipboard successfully"
                );
            }
            MimeClass::OctetStream | MimeClass::Unrecognized => {
                // §11.2: any rep that survives this far without a recognized
                // mime would previously be written via `set_buffer(format_id, …)`,
                // which on macOS attaches the bytes to a non-UTI pasteboard
                // type that no consumer (including the OS clipboard manager)
                // recognizes — Cmd+V silently produces nothing. Refusing the
                // write surfaces the upstream mis-classification instead.
                //
                // The 2026-05-08 IMG_20260508_200644.jpg regression took
                // this exact path: image rep + application/octet-stream
                // landed here and corrupted the clipboard. Image bytes are
                // now caught earlier by the byte-sniff guard above; this
                // branch remains as a hard floor against future regressions.
                anyhow::bail!(
                    "write_snapshot: refusing to write rep with unrecognized mime \
                     (mime={:?}, format_id={:?}, bytes={}); rep would land on a \
                     non-standard pasteboard type that no consumer can read",
                    effective_mime.as_str(),
                    rep.format_id,
                    rep.size_bytes()
                );
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

    // ─── filter_syncable_clipboard_files ─────────────────────────────────
    //
    // 守住"零字节剪贴板文件不进入同步管线"的契约。回归场景:macOS 上启动 uuremote 后,
    // 远控的剪贴板同步功能会以 ~500ms/个 的节奏往 com.netease.uuremote/Clipboard/
    // 写入 .uuremote_* 占位文件,这些文件长度恒为 0 字节。

    use std::path::{Path, PathBuf};

    #[test]
    fn filter_syncable_clipboard_files_drops_zero_byte_files() {
        let paths = vec![
            PathBuf::from("/tmp/.uuremote_aaa"),
            PathBuf::from("/tmp/real.txt"),
            PathBuf::from("/tmp/.uuremote_bbb"),
        ];
        let kept = filter_syncable_clipboard_files(paths, |p: &Path| {
            if p == Path::new("/tmp/real.txt") {
                Ok(1024)
            } else {
                Ok(0)
            }
        });
        assert_eq!(kept, vec![PathBuf::from("/tmp/real.txt")]);
    }

    #[test]
    fn filter_syncable_clipboard_files_returns_empty_when_all_zero() {
        let paths = vec![
            PathBuf::from("/tmp/.uuremote_aaa"),
            PathBuf::from("/tmp/.uuremote_bbb"),
        ];
        let kept = filter_syncable_clipboard_files(paths, |_p: &Path| Ok(0));
        assert!(kept.is_empty());
    }

    #[test]
    fn filter_syncable_clipboard_files_drops_unreadable_files() {
        let paths = vec![
            PathBuf::from("/tmp/missing"),
            PathBuf::from("/tmp/real.bin"),
        ];
        let kept = filter_syncable_clipboard_files(paths, |p: &Path| {
            if p == Path::new("/tmp/missing") {
                Err(std::io::Error::from(std::io::ErrorKind::NotFound))
            } else {
                Ok(42)
            }
        });
        assert_eq!(kept, vec![PathBuf::from("/tmp/real.bin")]);
    }

    #[test]
    fn filter_syncable_clipboard_files_keeps_all_when_all_nonzero() {
        let paths = vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")];
        let kept = filter_syncable_clipboard_files(paths.clone(), |_p: &Path| Ok(1));
        assert_eq!(kept, paths);
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

    mod effective_mime {
        use super::*;
        use uc_core::clipboard::MimeClass;
        use uc_core::ids::{FormatId, RepresentationId};

        fn rep(
            format: &str,
            mime: Option<&str>,
            bytes: Vec<u8>,
        ) -> ObservedClipboardRepresentation {
            ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from_str(format),
                mime.map(|m| MimeType(m.to_string())),
                bytes,
            )
        }

        /// 直接复现今天用户报的 fedora → mac 同步问题：
        /// `text/plain;charset=utf-8` 必须命中 `MimeClass::TextPlain`,
        /// 而不是掉进 OctetStream/Unrecognized 让 write_snapshot bail.
        #[test]
        fn parameterized_text_plain_classifies_as_text_plain() {
            let cases = [
                "text/plain",
                "text/plain;charset=utf-8",
                "text/plain; charset=utf-8",
                "Text/Plain; Charset=UTF-8",
                "  text/plain ; charset = \"utf-8\" ",
                "TEXT/PLAIN",
                "public.utf8-plain-text",
            ];
            for raw in cases {
                let r = rep("text", Some(raw), b"hello".to_vec());
                let effective = compute_effective_mime(&r).expect("effective mime");
                assert_eq!(
                    effective.classify(),
                    MimeClass::TextPlain,
                    "expected TextPlain for mime={raw:?}"
                );
            }
        }

        #[test]
        fn parameterized_text_html_classifies_as_text_html() {
            let r = rep("html", Some("text/html;charset=utf-8"), b"<p>hi".to_vec());
            assert_eq!(
                compute_effective_mime(&r).unwrap().classify(),
                MimeClass::TextHtml
            );
        }

        #[test]
        fn format_id_falls_back_when_mime_missing() {
            // 现代 capture 路径会给 rep 打 mime,但旧 envelope / legacy 客户端
            // 可能省略 mime —— 单源映射表必须能从 format_id 兜底。
            let r = rep("text", None, b"hello".to_vec());
            assert_eq!(
                compute_effective_mime(&r).unwrap().classify(),
                MimeClass::TextPlain
            );

            let r = rep("public.png", None, vec![0x89, b'P', b'N', b'G']);
            assert!(matches!(
                compute_effective_mime(&r).unwrap().classify(),
                MimeClass::Image(_)
            ));
        }

        #[test]
        fn image_format_id_with_octet_stream_mime_is_sniffed_back_to_image() {
            // 历史回归 (2026-05-08 IMG_20260508_200644.jpg)：iOS 客户端 PUT /file
            // 不带 Content-Type → 服务端默认 application/octet-stream,但
            // format_id 仍是 image-like。必须字节嗅探 recover,不能让 octet-stream
            // 落到下游写入分支。
            let png_magic = vec![
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
                0x00, 0x00, 0x00, 0x0D, // IHDR length
            ];
            let r = rep("public.png", Some("application/octet-stream"), png_magic);
            let effective = compute_effective_mime(&r).expect("effective mime");
            assert_eq!(effective.essence(), "image/png");
        }

        #[test]
        fn no_mime_and_unknown_format_id_returns_none() {
            // 此时 write_snapshot 会 bail —— §11.2 不允许静默用 format_id 当
            // pasteboard type 写非 UTI 内容。
            let r = rep("vendor-private-format", None, b"opaque".to_vec());
            assert!(compute_effective_mime(&r).is_none());
        }

        #[test]
        fn application_json_classifies_as_unrecognized() {
            // unrecognized application/* mime 必须分类为 Unrecognized,
            // 让 write_snapshot bail 而不是走 fallback set_buffer。
            let r = rep("unknown", Some("application/json"), b"{}".to_vec());
            let effective = compute_effective_mime(&r).expect("effective mime");
            assert_eq!(effective.classify(), MimeClass::Unrecognized);
        }
    }
}
