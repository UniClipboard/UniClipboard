//! 共享的 `LatestPasteRepresentation` ↔ SyncClipboard wire 映射规则。
//!
//! P5a.4 的 `GET /SyncClipboard.json`(meta) 与 P5a.5 的 `GET /file/{dataName}`
//! (binary) 都要回答同一个问题:"给定一份 paste rep, 它在 SyncClipboard 协议
//! 里的 type 是什么、dataName 该用什么文件名、图片 mime 该怎样兜底"。
//! 如果两条路径各自实现一份,漂移之后会出现 meta wire 报 dataName=A 而
//! file GET 路径却以为是 B → iPhone 客户端拿不到内容。把规则抽到这一处,
//! 单一真相, 两条路径一起进退。
//!
//! 调用约束(`pub(super)`):仅供 mobile_sync use cases 内部 share, 不向外暴露
//! —— facade 层不该接触这些低层文件名规则,看不见就不会被滥用。

use uc_core::clipboard::is_file_mime_or_format;
use uc_core::ids::EntryId;
use uc_core::mobile_sync::LatestPasteRepresentation;

use crate::usecases::mobile_sync::clipboard_doc::SyncClipboardItemType;

/// 把 paste rep 翻成 SyncClipboard 协议的 4 个 `type` 之一。
///
/// 复用 `uc-core::clipboard::is_file_mime_or_format` 的同款判定 —— 与
/// capture / policy v1 链上的"file rep 识别"完全一致, 避免两套规则漂移。
/// `format_id == image` 也按图片处理:SyncClipboard Android 的 multipart
/// 上传常把真实 JPEG/PNG 标成 `application/octet-stream`,但入站构建的
/// rep 仍会保留 format_id=image。不能只看 mime,否则最新历史记录会被误报
/// 为 Text。
///
/// 其余(text/* / rich-text / 完全没有类型线索的兜底)归 `Text`,富文本不
/// 影响整条同步链路,富文本保格式留给 v2。
pub(super) fn classify_for_sync(rep: &LatestPasteRepresentation) -> SyncClipboardItemType {
    if is_file_mime_or_format(rep.mime.as_ref(), &rep.format_id) {
        return SyncClipboardItemType::File;
    }
    if effective_image_mime_for_sync(rep).is_some() {
        return SyncClipboardItemType::Image;
    }
    SyncClipboardItemType::Text
}

/// SyncClipboard 出站时使用的图片 mime。
///
/// 优先相信显式 `image/*`;如果 format_id 表示图片但显式 mime 是
/// `application/octet-stream` 或缺失,先按文件头魔数恢复真实类型,再退回
/// format_id 的默认类型。
pub(super) fn effective_image_mime_for_sync(
    rep: &LatestPasteRepresentation,
) -> Option<&'static str> {
    if let Some(mime) = rep.mime.as_ref() {
        let raw = mime.as_str();
        if raw.starts_with("image/") {
            return Some(canonical_image_mime(raw));
        }
    }

    let default = image_mime_from_format_id(&rep.format_id)?;
    Some(sniff_image_magic(&rep.bytes).unwrap_or(default))
}

/// 派生 SyncClipboard wire 的 `dataName` 字段。
///
/// - `Text` / `Group` → `None`(协议层 `hasData=false`,无附件)
/// - `Image` → `Some("clipboard_<entry-short>.<ext>")`(ext 来自 mime,不识别
///   兜底 `.bin`)
/// - `File` → 首条非注释 URI 末段 + 百分号解码;解析失败兜底
///   `Some("clipboard_<entry-short>.bin")`
///
/// 同一份 rep 在 meta GET 与 file GET 跨请求中拿到一致 dataName 是 iPhone
/// 客户端 dedup 的前提条件 —— 这条函数就是那个一致性的 anchor。
pub(super) fn derive_data_name(
    rep: &LatestPasteRepresentation,
    item_type: SyncClipboardItemType,
) -> Option<String> {
    match item_type {
        SyncClipboardItemType::Text | SyncClipboardItemType::Group => None,
        SyncClipboardItemType::Image => Some(derive_image_filename(rep)),
        SyncClipboardItemType::File => Some(
            parse_first_uri_filename(&rep.bytes).unwrap_or_else(|| derive_fallback_filename(rep)),
        ),
    }
}

/// 按 SyncClipboard profile hash 规则计算 hash。
///
/// Text 直接对 UTF-8 字节算 SHA-256；Image/File 先算内容 SHA-256，再用
/// `dataName|CONTENT_HASH` 拼接后二次 SHA-256。返回大写十六进制。
pub(super) fn profile_hash_for_sync(
    item_type: SyncClipboardItemType,
    data_name: Option<&str>,
    bytes: &[u8],
) -> String {
    let content_hash = sha256_hex_upper(bytes);
    match item_type {
        SyncClipboardItemType::Image | SyncClipboardItemType::File => {
            if let Some(name) = data_name.filter(|s| !s.is_empty()) {
                sha256_hex_upper(format!("{name}|{content_hash}").as_bytes())
            } else {
                content_hash
            }
        }
        SyncClipboardItemType::Text | SyncClipboardItemType::Group => content_hash,
    }
}

// ─── internal filename helpers ──────────────────────────────────────────

fn sha256_hex_upper(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    hex::encode(Sha256::digest(bytes)).to_ascii_uppercase()
}

fn derive_image_filename(rep: &LatestPasteRepresentation) -> String {
    let ext = effective_image_mime_for_sync(rep)
        .and_then(image_mime_str_to_ext)
        .unwrap_or("bin");
    format!("clipboard_{}.{}", entry_id_short(&rep.entry_id), ext)
}

fn image_mime_str_to_ext(mime: &str) -> Option<&'static str> {
    match mime.to_ascii_lowercase().as_str() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/bmp" => Some("bmp"),
        "image/tiff" | "image/tif" => Some("tiff"),
        "image/heic" => Some("heic"),
        // 不识别的子类型(image/svg+xml / image/x-icon / ...)
        _ => None,
    }
}

fn canonical_image_mime(mime: &str) -> &'static str {
    match mime.to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => "image/jpeg",
        "image/png" => "image/png",
        "image/gif" => "image/gif",
        "image/webp" => "image/webp",
        "image/bmp" => "image/bmp",
        "image/tiff" | "image/tif" => "image/tiff",
        "image/heic" => "image/heic",
        _ => "image/unknown",
    }
}

fn image_mime_from_format_id(format_id: &uc_core::ids::FormatId) -> Option<&'static str> {
    match format_id.as_str().to_ascii_lowercase().as_str() {
        "image" | "public.png" => Some("image/png"),
        "public.jpeg" | "public.jpg" => Some("image/jpeg"),
        "public.gif" => Some("image/gif"),
        "public.tiff" | "public.tif" => Some("image/tiff"),
        _ => None,
    }
}

fn sniff_image_magic(body: &[u8]) -> Option<&'static str> {
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

fn derive_fallback_filename(rep: &LatestPasteRepresentation) -> String {
    format!("clipboard_{}.bin", entry_id_short(&rep.entry_id))
}

fn entry_id_short(entry_id: &EntryId) -> String {
    let s = entry_id.to_string();
    let take_n = s.len().min(8);
    s.chars().take(take_n).collect()
}

/// File rep 的字节是 RFC 2483 风格的 URI-list (text/uri-list)。规则:
/// - 一行一个 URI, 空行 / `#` 注释行忽略
/// - 取首条非空非注释行作为目标 URI
/// - 解析为 URL, 取最后一个 path segment 作为文件名, 百分号解码
///
/// 解析失败(空 list / 非 URI / path 为空)返回 None, 调用方走 fallback 名。
fn parse_first_uri_filename(bytes: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(bytes).ok()?;
    let first = s
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))?;

    let url = url::Url::parse(first).ok()?;
    let last_segment = url
        .path_segments()?
        .filter(|seg| !seg.is_empty())
        .next_back()?;
    let decoded = percent_decode(last_segment);
    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

/// `url::Url::path_segments` 返回的已经是 percent-encoded 字符串(如
/// `My%20Photo.png`)。手写一份小巧的 percent decode 避免引入
/// `percent-encoding` 单独依赖 —— 上游 `url` 0.5 不再 re-export 它。
///
/// 仅处理 ASCII 内的 `%HH`, 异常序列(如 `%2X` 不合法 hex)整体保留原样。
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex_value(bytes[i + 1]);
            let lo = hex_value(bytes[i + 2]);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
