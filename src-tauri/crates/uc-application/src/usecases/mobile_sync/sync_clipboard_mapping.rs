//! 共享的 `LatestPasteRepresentation` ↔ SyncClipboard wire 映射规则。
//!
//! P5a.4 的 `GET /SyncClipboard.json`(meta) 与 P5a.5 的 `GET /file/{dataName}`
//! (binary) 都要回答同一个问题:"给定一份 paste rep, 它在 SyncClipboard 协议
//! 里的 type 是什么、dataName 该用什么文件名"。如果两条路径各自实现一份,
//! 漂移之后会出现 meta wire 报 dataName=A 而 file GET 路径却以为是 B → iPhone
//! 客户端拿不到内容。把规则抽到这一处, 单一真相, 两条路径一起进退。
//!
//! 调用约束(`pub(super)`):仅供 mobile_sync use cases 内部 share, 不向外暴露
//! —— facade 层不该接触这些低层文件名规则,看不见就不会被滥用。

use uc_core::clipboard::{is_file_mime_or_format, MimeType};
use uc_core::ids::EntryId;
use uc_core::mobile_sync::LatestPasteRepresentation;

use crate::usecases::mobile_sync::clipboard_doc::SyncClipboardItemType;

/// 把 paste rep 翻成 SyncClipboard 协议的 4 个 `type` 之一。
///
/// 复用 `uc-core::clipboard::is_file_mime_or_format` 的同款判定 —— 与
/// capture / policy v1 链上的"file rep 识别"完全一致, 避免两套规则漂移。
/// 其余(text/* / rich-text / 完全没有 mime 的兜底)归 `Text`,
/// 富文本不影响整条同步链路, 富文本保格式留给 v2。
pub(super) fn classify_for_sync(rep: &LatestPasteRepresentation) -> SyncClipboardItemType {
    if is_file_mime_or_format(rep.mime.as_ref(), &rep.format_id) {
        return SyncClipboardItemType::File;
    }
    if mime_starts_with(&rep.mime, "image/") {
        return SyncClipboardItemType::Image;
    }
    SyncClipboardItemType::Text
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

// ─── internal filename helpers ──────────────────────────────────────────

fn mime_starts_with(mime: &Option<MimeType>, prefix: &str) -> bool {
    mime.as_ref()
        .is_some_and(|m| m.as_str().starts_with(prefix))
}

fn derive_image_filename(rep: &LatestPasteRepresentation) -> String {
    let ext = image_mime_to_ext(&rep.mime).unwrap_or("bin");
    format!("clipboard_{}.{}", entry_id_short(&rep.entry_id), ext)
}

fn image_mime_to_ext(mime: &Option<MimeType>) -> Option<&'static str> {
    let s = mime.as_ref()?.as_str().to_ascii_lowercase();
    match s.as_str() {
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
