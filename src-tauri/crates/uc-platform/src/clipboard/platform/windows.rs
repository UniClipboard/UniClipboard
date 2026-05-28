use super::super::cf_html::strip_cf_html_wrapper;
use super::super::common::CommonClipboardImpl;
use super::super::payload::rep_bytes;
use anyhow::Result;
use async_trait::async_trait;
use clipboard_rs::{Clipboard, ClipboardContext};
use std::ops::Range;
use std::sync::{Arc, Mutex};
use tracing::{debug, debug_span, error, info, warn};
use uc_core::clipboard::{
    ImageKind, MimeClass, MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot,
};
use uc_core::ids::RepresentationId;
use uc_core::ports::SystemClipboardPort;

use crate::clipboard::format_id_mime::format_id_default_mime;

/// Classify a rep for the Windows multi-rep write path.
///
/// Single source of mapping (shared with `common.rs` single-rep path and
/// `macos.rs::resolve_multi_rep_mime`):
/// 1. Use the rep's declared mime when present.
/// 2. Otherwise fall back to `format_id_default_mime` (the project-wide
///    `format_id → MIME` table).
/// 3. Recover image bytes mis-labeled as non-image (typically
///    `application/octet-stream` from clients that omit Content-Type) by
///    sniffing the magic-number. See `common.rs::write_snapshot` for the
///    incident history.
fn resolve_multi_rep_mime(rep: &ObservedClipboardRepresentation) -> Option<MimeClass> {
    let format_default = format_id_default_mime(rep.format_id.as_str());

    let effective: Option<MimeType> = match (rep.mime.as_ref(), format_default.as_ref()) {
        (Some(m), Some(default)) if default.is_image() && !m.is_image() => {
            let recovered = rep_bytes(rep)
                .ok()
                .and_then(|b| crate::clipboard::common::sniff_image_magic(&b))
                .map(|s| MimeType(s.to_string()))
                .unwrap_or_else(|| default.clone());
            tracing::warn!(
                format_id = %rep.format_id,
                wire_mime = m.as_str(),
                recovered_mime = recovered.as_str(),
                "Windows multi-rep: image rep declared non-image mime; recovered via byte sniff/format_id default"
            );
            Some(recovered)
        }
        (Some(m), _) => Some(m.clone()),
        (None, _) => format_default.clone(),
    };

    effective.map(|m| m.classify())
}

/// Whether a [`MimeClass`] is writable through the Windows multi-rep path.
/// Multi-rep only supports MIMEs that map to a known clipboard format
/// (CF_UNICODETEXT / CF_HTML / "Rich Text Format" / CF_DIBV5 + "PNG" /
/// CF_HDROP); anything else is skipped rather than dropped on a custom
/// format that no consumer can read.
fn is_multi_rep_writable(class: &MimeClass) -> bool {
    matches!(
        class,
        MimeClass::TextPlain
            | MimeClass::TextHtml
            | MimeClass::TextRtf
            | MimeClass::UriList
            | MimeClass::Image(ImageKind::Png)
    )
}

/// 把 text/uri-list rep 的字节解析为本机路径列表（`Vec<PathBuf>`）。
///
/// 接受两种形式：
/// - `file://...` URI：通过 `url::Url::to_file_path` 还原为原生路径
/// - 原始路径：非 URI 字符串按行直接当作路径处理（兼容 materializer 变更前的行为）
///
/// 空行与以 `#` 开头的注释行（RFC 2483 text/uri-list 规定）被跳过。
fn parse_uri_list_to_paths(bytes: &[u8]) -> Result<Vec<std::path::PathBuf>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| anyhow::anyhow!("text/uri-list rep is not valid UTF-8: {}", e))?;
    let mut paths = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Ok(url) = url::Url::parse(line) {
            if url.scheme() == "file" {
                if let Ok(path) = url.to_file_path() {
                    paths.push(path);
                    continue;
                }
            }
        }
        // Fallback：非 URI 当原生路径处理（允许下游 materializer 向后兼容）。
        paths.push(std::path::PathBuf::from(line));
    }
    Ok(paths)
}

/// 把 `Vec<PathBuf>` 编码为 CF_HDROP 所需的 DROPFILES 结构。
///
/// CF_HDROP 二进制布局（Win32 SDK `shlobj_core.h`）：
/// ```text
/// [DROPFILES struct (20 bytes)]
///   pFiles: u32       // 文件名数组相对 DROPFILES 起点的偏移
///   pt.x: i32
///   pt.y: i32         // 拖放时的像素坐标（剪贴板粘贴场景 0/0 即可）
///   fNC: u32          // 0
///   fWide: u32        // 非零 = 文件名数组为 Unicode (UTF-16 LE)
/// [UTF-16 LE, NUL-terminated file names (一个接一个)]
/// [额外的 UTF-16 NUL (u16 0)]  // double-NUL 终结
/// ```
fn paths_to_cf_hdrop_bytes(paths: &[std::path::PathBuf]) -> Result<Vec<u8>> {
    if paths.is_empty() {
        anyhow::bail!("CF_HDROP 要求至少一条路径");
    }

    // DROPFILES (20 bytes) + UTF-16 名字串 + 终止 NUL。
    let mut out = Vec::with_capacity(20 + paths.len() * 32);

    // DROPFILES.pFiles = 20 (struct 长度)
    out.extend_from_slice(&20u32.to_le_bytes());
    // POINT.x / POINT.y / fNC
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    // fWide = 1 (Unicode)
    out.extend_from_slice(&1u32.to_le_bytes());
    debug_assert_eq!(out.len(), 20);

    // Windows 上 OsStr 原生 UTF-16；用 `OsStrExt::encode_wide` 直接拿 surrogate-safe
    // 的 u16 迭代，避免经过 UTF-8 `to_string_lossy` 的有损中转。
    use std::os::windows::ffi::OsStrExt;
    for path in paths {
        for code_unit in path.as_os_str().encode_wide() {
            out.extend_from_slice(&code_unit.to_le_bytes());
        }
        // 每条路径以 UTF-16 NUL 结束
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    // 额外一个 UTF-16 NUL —— double-NUL 终结
    out.extend_from_slice(&0u16.to_le_bytes());

    Ok(out)
}

/// 把 PNG 字节解码后编码为 **CF_DIBV5** 格式的字节流（`BITMAPV5HEADER` + pixel data）。
///
/// CF_DIBV5（Win2000+）相比 CF_BITMAP / CF_DIB 有三个关键优势，这也是我们在多 rep
/// 写入路径选它的原因：
///
/// 1. **保留 alpha 通道**：`BI_BITFIELDS` + 32bpp + `bV5AlphaMask = 0xFF000000`
///    让透明像素在 PowerPoint / Photoshop 等目标应用里保留。BMP / CF_DIB 会把
///    alpha 压平成黑/白边。
/// 2. **不经过 GDI**：`SetClipboardData(CF_DIBV5, bytes)` 只做 GlobalAlloc + memcpy，
///    不走 `CreateDIBitmap`。CF_BITMAP 路径的 1418 根因就是 `CreateDIBitmap` 对大
///    bitmap 触发 GDI 消息泵 / 资源竞争。
/// 3. **自动合成**：写 CF_DIBV5 之后 Windows 会为其他应用合成 CF_BITMAP / CF_DIB /
///    CF_PALETTE，对只认 CF_BITMAP 的老应用（画图、Office 2010-）完全兼容。
///
/// **调用时机**：仍然建议在 `OpenClipboard` 会话之外预编码。PNG 解码 + RGBA→BGRA
/// 拷贝对 MB 级图像仍有几十毫秒成本，保持"预编码前移"策略可以进一步压缩剪贴板
/// 持有窗口。
///
/// **像素布局**：top-down（`bV5Height` 为负），32bpp，内存 byte order 为 BGRA —— 这
/// 由 little-endian + bit-field mask 决定：`bV5RedMask = 0x00FF0000` 意味着 R 落在
/// dword 的 byte-offset 2，`bV5GreenMask = 0x0000FF00` → offset 1，`bV5BlueMask
/// = 0x000000FF` → offset 0，`bV5AlphaMask = 0xFF000000` → offset 3。
fn png_to_dibv5(png_bytes: &[u8]) -> Result<Vec<u8>> {
    use image::{load_from_memory_with_format, ImageFormat};

    let img = load_from_memory_with_format(png_bytes, ImageFormat::Png)
        .map_err(|e| anyhow::anyhow!("decode PNG failed: {e}"))?
        .to_rgba8();
    let width = img.width();
    let height = img.height();
    let pixel_bytes = (width as usize) * (height as usize) * 4;

    let mut out = Vec::with_capacity(124 + pixel_bytes);

    // BITMAPV5HEADER (124 bytes) — 字段顺序按 MSDN wingdi.h 定义。
    out.extend_from_slice(&124u32.to_le_bytes()); // bV5Size
    out.extend_from_slice(&(width as i32).to_le_bytes()); // bV5Width
    out.extend_from_slice(&(-(height as i32)).to_le_bytes()); // bV5Height（负值 = top-down，免翻转）
    out.extend_from_slice(&1u16.to_le_bytes()); // bV5Planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bV5BitCount
    out.extend_from_slice(&3u32.to_le_bytes()); // bV5Compression = BI_BITFIELDS
    out.extend_from_slice(&(pixel_bytes as u32).to_le_bytes()); // bV5SizeImage
    out.extend_from_slice(&0i32.to_le_bytes()); // bV5XPelsPerMeter
    out.extend_from_slice(&0i32.to_le_bytes()); // bV5YPelsPerMeter
    out.extend_from_slice(&0u32.to_le_bytes()); // bV5ClrUsed
    out.extend_from_slice(&0u32.to_le_bytes()); // bV5ClrImportant
                                                // Channel bit masks — 决定 32bpp 内 BGRA byte layout（见上方 doc comment）。
    out.extend_from_slice(&0x00FF0000u32.to_le_bytes()); // bV5RedMask
    out.extend_from_slice(&0x0000FF00u32.to_le_bytes()); // bV5GreenMask
    out.extend_from_slice(&0x000000FFu32.to_le_bytes()); // bV5BlueMask
    out.extend_from_slice(&0xFF000000u32.to_le_bytes()); // bV5AlphaMask
    out.extend_from_slice(&0x7352_4742u32.to_le_bytes()); // bV5CSType = LCS_sRGB ('sRGB' little-endian)
    out.extend_from_slice(&[0u8; 36]); // bV5Endpoints CIEXYZTRIPLE（sRGB 下被忽略）
    out.extend_from_slice(&0u32.to_le_bytes()); // bV5GammaRed
    out.extend_from_slice(&0u32.to_le_bytes()); // bV5GammaGreen
    out.extend_from_slice(&0u32.to_le_bytes()); // bV5GammaBlue
    out.extend_from_slice(&4u32.to_le_bytes()); // bV5Intent = LCS_GM_IMAGES（图像渲染意图）
    out.extend_from_slice(&0u32.to_le_bytes()); // bV5ProfileData
    out.extend_from_slice(&0u32.to_le_bytes()); // bV5ProfileSize
    out.extend_from_slice(&0u32.to_le_bytes()); // bV5Reserved
    debug_assert_eq!(out.len(), 124);

    // Pixel data：image crate 给 RGBA，CF_DIBV5 BI_BITFIELDS 需要 BGRA（见 mask 解释）。
    out.reserve(pixel_bytes);
    for chunk in img.as_raw().chunks_exact(4) {
        out.push(chunk[2]); // B
        out.push(chunk[1]); // G
        out.push(chunk[0]); // R
        out.push(chunk[3]); // A
    }

    Ok(out)
}

/// 单次写入尝试的最大次数。
///
/// `ERROR_CLIPBOARD_NOT_OPEN (1418)` 在大多数情况下是瞬态的（消息泵、GDI 竞争
/// 或其他进程短暂打开剪贴板）。经验上第二次尝试几乎都能成功。
///
/// 4 次而非 3 次：Sentry RUST-F 显示某些 Windows 机器上剪贴板会被反病毒/输入法/
/// RDP 会话独占数十秒（`OSError(5) ERROR_ACCESS_DENIED`），毫秒级退避完全碰不
/// 到让出窗口。配合下面秒级退避表，整体重试覆盖 ~2.6s 长尾。
const MAX_WRITE_ATTEMPTS: u32 = 4;

/// 每次重试前的退避（毫秒）。索引 = attempt 次数，0 次不退避。
///
/// 之前是 `[0, 20, 40]`（共 60ms），对短暂的 GDI/消息泵竞争够用，但拦不住
/// `ERROR_ACCESS_DENIED` 这类长尾独占。改为秒级退避覆盖反病毒扫描、输入法
/// hook、RDP 剪贴板代理等长时间持有剪贴板的场景。仍然有界——超过 ~2.6s 的
/// 持续独占会被上层 circuit breaker（`ClipboardWriteCoordinator`）兜底。
const RETRY_BACKOFF_MS: [u64; MAX_WRITE_ATTEMPTS as usize] = [0, 100, 500, 2000];

/// Classify a Windows clipboard write error into a stable `error_kind` tag.
///
/// Used in `warn!` / `error!` events to give Sentry a column it can
/// `group_by` independent of the (often-localized) error message string.
/// Keep these strings stable — they are referenced in dashboards and queries.
fn classify_windows_write_error(err: &anyhow::Error) -> &'static str {
    let s = err.to_string();
    // OSError(5) = ERROR_ACCESS_DENIED — clipboard held exclusively by
    // another process (AV scan, IME hook, RDP clip proxy, clipboard manager).
    // The literal "拒绝访问" is the localized message from a zh-CN host;
    // matching both keeps the kind language-independent.
    if s.contains("OSError(5)") || s.contains("ERROR_ACCESS_DENIED") || s.contains("拒绝访问") {
        "os_clipboard_access_denied"
    // OSError(1418) = ERROR_CLIPBOARD_NOT_OPEN — transient GDI / message-pump
    // race. Usually clears on the next retry.
    } else if s.contains("OSError(1418)") || s.contains("ERROR_CLIPBOARD_NOT_OPEN") {
        "os_clipboard_not_open"
    } else {
        "os_clipboard_write_other"
    }
}

/// Windows 原子多 representation 写入。
///
/// 在**单次** `OpenClipboard` 会话内写入多个剪贴板格式（CF_UNICODETEXT + CF_HTML +
/// CF_DIBV5 + 自定义 "PNG" format），确保纯文本目的地（记事本）、富文本目的地
/// （Word / 写字板）、图片目的地（画图 / Chrome / Paint.NET / PowerPoint 等）都能正确粘贴。
///
/// ## 设计要点
///
/// ### 为何要手动 `empty()` 一次
/// `Clipboard::new_attempts` 只负责 `OpenClipboard`，**不**清空剪贴板现有内容。
/// 如果不手动调用 `raw::empty()`，旧的 representations（如上一次复制的图片 CF_DIB）
/// 会与新内容混在一起，产生"幽灵格式"。因此在会话开头显式清空一次。
///
/// ### 为何使用 `*_without_clear` / `set_html` / `set_string_with::<NoClear>` 系列
/// `raw::set()` 和 `raw::set_string()` 在写入时会内部调用 `EmptyClipboard`，
/// 将前面已写入的格式全部抹掉——正是本次要修复的 bug 根因。
/// `raw::set_without_clear` / `raw::set_html`（默认 NoClear）/
/// `raw::set_string_with::<NoClear>` / `raw::set_without_clear` 不调
/// `EmptyClipboard`，可以在同一 RAII 会话内累加多个 format 到同一 clipboard item。
///
/// ### 本次支持 text/plain + text/html + image/png
/// - `text/plain` → CF_UNICODETEXT（`set_string_with::<NoClear>`）
/// - `text/html` → CF_HTML（`set_html`，内部默认 NoClear）
/// - `image/png` → CF_DIBV5（PNG→`BITMAPV5HEADER` 字节后 `set_without_clear(CF_DIBV5, …)`）
///   + 自定义 "PNG" format（`register_format("PNG")` 后 `set_without_clear`）
///   双写策略：CF_DIBV5 由 Windows 自动合成出 CF_BITMAP / CF_DIB 给老应用（画图 /
///   Office 2010- / 剪贴板历史 / 第三方剪贴板工具）；现代应用（Chrome / Firefox /
///   Paint.NET / 新版 Office / Google Docs）优先读 "PNG" 拿到原始字节保留压缩率
///   与全部 PNG 元数据。CF_DIBV5 原生带 alpha 通道（`bV5AlphaMask = 0xFF000000`），
///   粘贴到 PowerPoint / Photoshop 时透明度不会被压平。
///
/// `text/rtf` → "Rich Text Format" 自定义 format（`register_format("Rich Text Format")`
///   返回的 RegisterClipboardFormat 注册码；这是 Word / 写字板识别的标准 RTF 剪贴
///   板 format 名）。RTF 字节直接 `set_without_clear` 累加到同一会话。
///
/// `image/jpeg` / `image/tiff` / `image/webp` / `image/gif` 仍未支持（各自需要独立的
/// 编码 / format 注册）。遇到本路径不认的 rep 记 debug 日志并跳过。
///
/// ### 调用方注意
/// 此函数由 `common.rs::write_snapshot_multi` 在 `#[cfg(target_os = "windows")]`
/// 分支调用。`WindowsClipboard::write_snapshot` 在多 rep 场景下会提前 drop
/// clipboard-rs ctx，避免与本函数的 `clipboard-win` OpenClipboard 抢句柄（OSError 1418）。
///
/// ### `empty()` 副作用的防御
/// `EmptyClipboard` 会立即清空用户当前的 OS 剪贴板。如果我们开了会话、清空了、
/// 然后发现全部 rep 都不认（例如 PixPin 截图：files + image + 5 个平台私有类型，
/// 没有 text/plain 也没有 text/html），用户看到的就是"粘贴为空"——原本剪贴板里的
/// 有效内容被静默抹掉了。为避免这个副作用，本函数**先**扫描 snapshot 判断至少
/// 有一条可写的 rep，再打开剪贴板并清空——没可写时直接 bail，OS 剪贴板保持原样。
/// bail 错误文案包含 "未清空 OS 剪贴板" 字样以便排障。
///
/// ### 大 PNG 的鲁棒写入策略
/// 早期用 `set_bitmap_with`（CF_BITMAP）在大图场景稳定触发
/// `ERROR_CLIPBOARD_NOT_OPEN (1418)`：`CreateDIBitmap` 对大 bitmap 的 GDI 交互
/// 与消息泵存在竞争。切到 CF_DIBV5（纯 `GlobalAlloc + memcpy`，不走 GDI）后
/// 根因消失。在此之上仍保留两层工程化防御：
/// 1. **预编码前移**：`png_to_dibv5` 在 `OpenClipboard` 会话**之外**完成，
///    压缩剪贴板持有窗口。
/// 2. **整体重试**：整个会话（open → empty → 逐 rep 写入）作为一个原子单元，
///    失败后退避重试，最多 `MAX_WRITE_ATTEMPTS` 次。兜底其他瞬态因素
///    （第三方剪贴板工具抢句柄、消息泵偶发竞争等）。
#[tracing::instrument(
    name = "windows.write_snapshot_multi",
    level = "debug",
    skip(snapshot),
    fields(rep_count = snapshot.representations.len())
)]
pub(crate) fn write_snapshot_multi_windows(snapshot: SystemClipboardSnapshot) -> Result<()> {
    // 前置扫描：如果没有任何 rep 是我们能写的（text/plain、text/html 或 image/png），
    // 直接 bail；**不**打开 Windows 剪贴板、**不**调 empty()。
    // 避免把用户原本的 OS 剪贴板清掉却什么都写不进去（见上方 doc comment "empty() 副作用的防御"）。
    let has_writable = snapshot
        .representations
        .iter()
        .any(|rep| resolve_multi_rep_mime(rep).is_some_and(|c| is_multi_rep_writable(&c)));

    if !has_writable {
        let skipped: Vec<String> = snapshot
            .representations
            .iter()
            .map(|r| r.format_id.as_str().to_string())
            .collect();
        warn!(
            rep_count = snapshot.representations.len(),
            skipped = ?skipped,
            "Windows 多 rep 写入：无可写 rep；未清空 OS 剪贴板（防副作用兜底）"
        );
        anyhow::bail!(
            "Windows 多 rep 写入：无可写 rep（支持 text/plain, text/html, text/rtf, \
             image/png, text/uri-list）；未清空 OS 剪贴板；跳过的 rep = {:?}",
            skipped
        );
    }

    // 预编码阶段 —— 在 OpenClipboard 会话**之外**完成两件事:
    //   1) 把 image/png rep 的字节物化成 owned `Vec<u8>`（Inline 直接 to_vec, LocalFile
    //      同步读盘）。在 OpenClipboard 会话内读盘会拉长持锁时间、增加 1418 风险,因此
    //      所有 IO 都在会话外完成。
    //   2) 把这份字节继续转码为 CF_DIBV5。失败时 `dib=None`,只写 "PNG" 自定义 format。
    //
    // 与 `rep` 一一对应；外层 `None` 表示该 rep 不走 image/png 路径（非 image/png 或读
    // LocalFile 失败）。LocalFile 读盘失败会在这里 warn 并 None,主循环看到 None 时跳过。
    let png_preencoded: Vec<Option<(Vec<u8>, Option<Vec<u8>>)>> = snapshot
        .representations
        .iter()
        .map(|rep| {
            if !matches!(
                resolve_multi_rep_mime(rep),
                Some(MimeClass::Image(ImageKind::Png))
            ) {
                return None;
            }
            let bytes = match rep_bytes(rep) {
                Ok(b) => b.into_owned(),
                Err(err) => {
                    warn!(
                        error = %err,
                        format_id = %rep.format_id,
                        size_bytes = rep.size_bytes(),
                        "Windows 多 rep 写入：读取 LocalFile image/png rep 字节失败，跳过该 rep"
                    );
                    return None;
                }
            };
            let dib = match png_to_dibv5(&bytes) {
                Ok(dib) => Some(dib),
                Err(e) => {
                    warn!(
                        error = %e,
                        png_bytes = bytes.len(),
                        "PNG→CF_DIBV5 转码失败；仅写 \"PNG\" 自定义 format"
                    );
                    None
                }
            };
            Some((bytes, dib))
        })
        .collect();

    // 整体重试 —— 把 OpenClipboard → EmptyClipboard → 逐 rep 写入 → CloseClipboard
    // 作为一个原子单元重试。任一 set_* 失败（典型为 1418）都放弃本次尝试，让 RAII
    // guard drop 关闭剪贴板，退避后重新打开再写一遍。
    //
    // 关键语义：重试时再次调 EmptyClipboard 会抹掉上一次失败 attempt 里已经写进去
    // 的部分格式；但由于 attempt 失败时 OS 剪贴板要么只含 empty() 后的空状态、
    // 要么含极少量已写入格式（尚未让用户感知），重新 empty 不会进一步损失用户
    // 可见的内容。
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..MAX_WRITE_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(
                RETRY_BACKOFF_MS[attempt as usize],
            ));
        }

        match attempt_multi_write_inner(&snapshot, &png_preencoded) {
            Ok(skipped) => {
                if !skipped.is_empty() {
                    debug!(
                        skipped_count = skipped.len(),
                        skipped = ?skipped,
                        "Windows 多 rep 写入：部分 rep 已跳过（不支持或写入失败）"
                    );
                }
                info!(
                    total_reps = snapshot.representations.len(),
                    skipped = skipped.len(),
                    attempt = attempt + 1,
                    "Windows 原子多 rep 写入完成"
                );
                return Ok(());
            }
            Err(e) => {
                let error_kind = classify_windows_write_error(&e);
                warn!(
                    event = "windows_multi_write_attempt_failed",
                    error_kind,
                    attempt = attempt + 1,
                    max_attempts = MAX_WRITE_ATTEMPTS,
                    backoff_ms = RETRY_BACKOFF_MS[attempt as usize],
                    error = %e,
                    "Windows 原子多 rep 写入本次尝试失败"
                );
                last_err = Some(e);
            }
        }
    }

    // Cumulative backoff so operators can correlate "how long did we
    // actually wait" vs "Sentry timestamp delta" without having to
    // re-derive it from the RETRY_BACKOFF_MS constant table.
    let cumulative_backoff_ms: u64 = RETRY_BACKOFF_MS.iter().sum();
    let final_error_kind = last_err
        .as_ref()
        .map(classify_windows_write_error)
        .unwrap_or("unknown");
    error!(
        event = "windows_multi_write_exhausted",
        error_kind = "windows_multi_write_exhausted",
        final_attempt_error_kind = final_error_kind,
        max_attempts = MAX_WRITE_ATTEMPTS,
        cumulative_backoff_ms,
        error = ?last_err.as_ref().map(|e| e.to_string()),
        "Windows 多 rep 写入：所有重试已耗尽"
    );

    anyhow::bail!(
        "Windows 多 rep 写入：{} 次尝试均失败；最后一次错误：{}",
        MAX_WRITE_ATTEMPTS,
        last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "<unknown>".to_string())
    )
}

/// 单次写入尝试：打开 → empty → 逐 rep 写入 → guard drop 时关闭。
///
/// 任何 `set_*` 失败（含 `ERROR_CLIPBOARD_NOT_OPEN (1418)`）都直接 `?` 上抛到
/// 外层重试；不在本函数内做"只写一部分"的局部降级 —— 降级由外层重试覆盖，保证
/// 每次尝试的所有 rep 要么全在同一会话内成功，要么整体作废。
///
/// 返回值是"因为不支持 / 子路径失败而跳过的 format_id 列表"。注意这只记录**明确
/// 跳过**的 rep（比如 image/png 的 CF_DIBV5 预编码为 None 且 register_format
/// 返回 None），不包含"整体尝试失败"的情况。
fn attempt_multi_write_inner(
    snapshot: &SystemClipboardSnapshot,
    png_preencoded: &[Option<(Vec<u8>, Option<Vec<u8>>)>],
) -> Result<Vec<String>> {
    use clipboard_win::formats::{Html as HtmlFmt, CF_DIBV5};
    use clipboard_win::options::NoClear;
    use clipboard_win::raw as cb_raw;
    use clipboard_win::Clipboard as ClipboardWin;

    // RAII：作用域内整段占有 Windows 剪贴板，drop 时自动 CloseClipboard。
    let _clip = ClipboardWin::new_attempts(10)
        .map_err(|e| anyhow::anyhow!("OpenClipboard failed after retries: {}", e))?;

    // 在会话开头显式清空一次——后续每次写入都走 *_without_clear 系列，
    // 这样多次调用会把多个 format 累加到同一个 clipboard item，
    // 而不会把前面写好的 CF_UNICODETEXT 再次清掉（这是 raw::set / set_string 的默认行为）。
    cb_raw::empty().map_err(|e| anyhow::anyhow!("EmptyClipboard failed: {}", e))?;

    // 提前注册 "HTML Format"——此为 Windows 约定的自定义 format 名称，
    // RegisterClipboardFormat 对相同名称是幂等的，返回值在整个进程生命周期内固定。
    let html_fmt_opt: Option<u32> = HtmlFmt::new().map(|h| h.code());

    // 提前注册 "Rich Text Format"——这是 Word / 写字板 / Outlook 等富文本应用约定的
    // RTF 剪贴板 format 名（CF_RTF 不是 Win32 预定义常量）。同样靠 RegisterClipboardFormat
    // 幂等性保证返回值在进程内稳定。失败时记 warn，主循环里跳过 RTF rep（不影响其他
    // rep 的写入）。
    let rtf_fmt_opt: Option<u32> = cb_raw::register_format("Rich Text Format").map(|nz| nz.get());

    let mut wrote_any = false;
    let mut skipped: Vec<String> = Vec::new();

    for (idx, rep) in snapshot.representations.iter().enumerate() {
        // 与前置扫描使用同一个 helper，保证 "能否写" 的判定与主循环分派逻辑不漂移。
        let effective_mime = resolve_multi_rep_mime(rep);

        match effective_mime {
            Some(MimeClass::TextPlain) => {
                // 必须使用 set_string_with::<NoClear>：
                // set_string 内部调用 DoClear（EmptyClipboard），会把已写的其他 format 抹掉。
                let bytes = match rep_bytes(rep) {
                    Ok(b) => b,
                    Err(err) => {
                        warn!(
                            error = %err,
                            format_id = %rep.format_id,
                            size_bytes = rep.size_bytes(),
                            "Windows 多 rep 写入：读取 LocalFile text/plain rep 字节失败，跳过该 rep"
                        );
                        skipped.push(rep.format_id.as_str().to_string());
                        continue;
                    }
                };
                let text = String::from_utf8(bytes.into_owned())
                    .map_err(|e| anyhow::anyhow!("text/plain rep is not valid UTF-8: {}", e))?;
                cb_raw::set_string_with::<NoClear>(&text, NoClear)
                    .map_err(|e| anyhow::anyhow!("set CF_UNICODETEXT failed: {}", e))?;
                debug!(bytes = text.len(), "写入 CF_UNICODETEXT 成功");
                wrote_any = true;
            }
            Some(MimeClass::TextHtml) => {
                let Some(html_fmt) = html_fmt_opt else {
                    warn!("注册 HTML Format 失败，跳过 text/html rep");
                    skipped.push(rep.format_id.as_str().to_string());
                    continue;
                };
                let bytes = match rep_bytes(rep) {
                    Ok(b) => b,
                    Err(err) => {
                        warn!(
                            error = %err,
                            format_id = %rep.format_id,
                            size_bytes = rep.size_bytes(),
                            "Windows 多 rep 写入：读取 LocalFile text/html rep 字节失败，跳过该 rep"
                        );
                        skipped.push(rep.format_id.as_str().to_string());
                        continue;
                    }
                };
                let html = String::from_utf8(bytes.into_owned())
                    .map_err(|e| anyhow::anyhow!("text/html rep is not valid UTF-8: {}", e))?;
                // Strip any CF_HTML outer wrapper before handing the payload
                // to `set_html`. `clipboard-win::raw::set_html` always re-wraps
                // its input with `<html><body><!--StartFragment-->...<!--EndFragment-->`
                // headers, while `clipboard-rs::get_html` returns the full
                // document (StartHTML..EndHTML, wrappers included). Without
                // this normalization each Win → peer → Win round-trip nests
                // another wrapper layer; `content_hash`-based dedup cannot
                // collapse them because each layer changes the hash.
                let html_payload = strip_cf_html_wrapper(&html);
                // set_html 默认走 NoClear 分支，内部构造 "Version:0.9 / StartHTML / EndHTML /
                // StartFragment / EndFragment" 头并包裹 BODY_HEADER/BODY_FOOTER，适合累加。
                cb_raw::set_html(html_fmt, html_payload)
                    .map_err(|e| anyhow::anyhow!("set CF_HTML failed: {}", e))?;
                debug!(
                    bytes = html_payload.len(),
                    stripped_bytes = html.len() - html_payload.len(),
                    "写入 CF_HTML 成功"
                );
                wrote_any = true;
            }
            Some(MimeClass::TextRtf) => {
                // RTF 走 RegisterClipboardFormat("Rich Text Format")。RTF 1.x 规范要求
                // 字节流是 ASCII 安全（非 ASCII 字符均通过 \uN 转义），因此可以直接以
                // 原始字节写入 raw set_without_clear，不需要 UTF-8 / UTF-16 转换。
                // set_without_clear 不调用 EmptyClipboard，保持累加语义。
                let Some(rtf_fmt) = rtf_fmt_opt else {
                    warn!("注册 Rich Text Format 失败，跳过 text/rtf rep");
                    skipped.push(rep.format_id.as_str().to_string());
                    continue;
                };
                let bytes = match rep_bytes(rep) {
                    Ok(b) => b,
                    Err(err) => {
                        warn!(
                            error = %err,
                            format_id = %rep.format_id,
                            size_bytes = rep.size_bytes(),
                            "Windows 多 rep 写入：读取 LocalFile text/rtf rep 字节失败，跳过该 rep"
                        );
                        skipped.push(rep.format_id.as_str().to_string());
                        continue;
                    }
                };
                cb_raw::set_without_clear(rtf_fmt, &bytes)
                    .map_err(|e| anyhow::anyhow!("set Rich Text Format failed: {}", e))?;
                debug!(bytes = bytes.len(), "写入 Rich Text Format 成功");
                wrote_any = true;
            }
            Some(MimeClass::Image(ImageKind::Png)) => {
                // 双写策略：CF_DIBV5（标准格式，Windows 自动合成 CF_BITMAP/CF_DIB 给老应用）
                // + 自定义 "PNG" format（现代应用直读 PNG 字节，保留 PNG 压缩率与 alpha 元数据）。
                //
                // 兼容矩阵：
                //   - CF_DIBV5 ← 画图、Office、写字板、剪贴板历史（Win+V）、第三方剪贴板工具
                //     （合成路径覆盖所有认 CF_BITMAP / CF_DIB 的应用）
                //   - "PNG"    ← Chrome、Firefox、Paint.NET、新版 Office、Google Docs
                //
                // PNG owned 字节 + CF_DIBV5 字节都已在 OpenClipboard 会话外完成预编码
                // （见 `png_preencoded` 构造），这里只做纯系统调用；任一 set_* 失败都 `?`
                // 抛到外层由重试机制兜底。
                //
                // 外层 None 表示该 rep 在预编码阶段读 LocalFile 失败（已 warn）,直接跳过。
                let Some((png_bytes, dib_opt)) = png_preencoded.get(idx).and_then(|o| o.as_ref())
                else {
                    skipped.push(rep.format_id.as_str().to_string());
                    continue;
                };
                let mut wrote_dib = false;
                let mut wrote_png = false;

                if let Some(dib_bytes) = dib_opt.as_ref() {
                    cb_raw::set_without_clear(CF_DIBV5, dib_bytes)
                        .map_err(|e| anyhow::anyhow!("set CF_DIBV5 failed: {}", e))?;
                    debug!(
                        dib_bytes = dib_bytes.len(),
                        png_bytes = png_bytes.len(),
                        "写入 CF_DIBV5 成功"
                    );
                    wrote_dib = true;
                }

                match cb_raw::register_format("PNG") {
                    Some(png_fmt) => {
                        cb_raw::set_without_clear(png_fmt.get(), png_bytes).map_err(|e| {
                            anyhow::anyhow!("set \"PNG\" custom format failed: {}", e)
                        })?;
                        debug!(
                            png_bytes = png_bytes.len(),
                            "写入 \"PNG\" 自定义 format 成功"
                        );
                        wrote_png = true;
                    }
                    None => {
                        warn!("register_format(\"PNG\") 返回 None；跳过 \"PNG\" 路径");
                    }
                }

                if wrote_dib || wrote_png {
                    wrote_any = true;
                } else {
                    skipped.push(rep.format_id.as_str().to_string());
                }
            }
            Some(MimeClass::UriList) => {
                // CF_HDROP 写入路径：把 rep 里的 file:// URI 列表（接收端 materializer
                // 已把 blob 落地到本机 iroh-blobs 缓存目录并改写为本机 URI）解析回本机
                // 路径，打包成 DROPFILES + UTF-16 名字串，`SetClipboardData(CF_HDROP)`。
                // 这是 Explorer / Office / 大多数桌面应用识别的文件拷贝语义。
                let bytes = match rep_bytes(rep) {
                    Ok(b) => b,
                    Err(err) => {
                        warn!(
                            error = %err,
                            format_id = %rep.format_id,
                            size_bytes = rep.size_bytes(),
                            "Windows 多 rep 写入：读取 LocalFile text/uri-list rep 字节失败，跳过该 rep"
                        );
                        skipped.push(rep.format_id.as_str().to_string());
                        continue;
                    }
                };
                let paths = match parse_uri_list_to_paths(&bytes) {
                    Ok(paths) => paths,
                    Err(e) => {
                        warn!(
                            error = %e,
                            bytes = bytes.len(),
                            format_id = %rep.format_id,
                            "Windows 多 rep 写入：text/uri-list 解析失败，跳过该 rep"
                        );
                        skipped.push(rep.format_id.as_str().to_string());
                        continue;
                    }
                };
                if paths.is_empty() {
                    info!(
                        format_id = %rep.format_id,
                        "Windows 多 rep 写入：text/uri-list 为空，跳过该 rep"
                    );
                    skipped.push(rep.format_id.as_str().to_string());
                    continue;
                }
                let hdrop_bytes = paths_to_cf_hdrop_bytes(&paths)
                    .map_err(|e| anyhow::anyhow!("CF_HDROP 编码失败: {}", e))?;
                // CF_HDROP = 15（见 Win32 `winuser.h`）。clipboard-win 的 `formats` 模块
                // 未直接导出该常量，直接使用数值避免引入额外的 `windows-sys` 依赖。
                const CF_HDROP: u32 = 15;
                cb_raw::set_without_clear(CF_HDROP, &hdrop_bytes)
                    .map_err(|e| anyhow::anyhow!("set CF_HDROP failed: {}", e))?;
                debug!(
                    path_count = paths.len(),
                    hdrop_bytes = hdrop_bytes.len(),
                    "写入 CF_HDROP 成功"
                );
                wrote_any = true;
            }
            // image/jpeg / image/tiff / image/webp / image/gif 等均未支持，
            // 未来在独立 phase 补齐（各自需要独立的编码转换 / format 注册）。
            other => {
                // `rep.size_bytes()` 对 Inline / LocalFile 两种 source 都安全（LocalFile
                // 返回构造时记录的 meta.len()），避免触发 expect_inline_bytes panic。
                info!(
                    format_id = %rep.format_id,
                    rep_mime = ?rep.mime.as_ref().map(|m| m.as_str()),
                    classified = ?other,
                    bytes = rep.size_bytes(),
                    "Windows 多 rep 写入：跳过不支持的 rep（当前支持 text/plain, text/html, text/rtf, image/png, text/uri-list）"
                );
                skipped.push(rep.format_id.as_str().to_string());
            }
        }
    }

    if !wrote_any {
        // 防御分支：前置 has_writable 扫描已确认至少有一条 rep 可写。
        // 走到这里说明主循环内部所有可写 rep 的 encode / register 都失败（极罕见）。
        // 本函数返回 Err，由外层重试兜底；若所有 attempt 都落到这里，最终由
        // `write_snapshot_multi_windows` 的 `bail!` 报给调用方。
        anyhow::bail!(
            "Windows 多 rep 写入：所有候选 rep 在写入阶段均失败（支持 text/plain, text/html, \
             text/rtf, image/png, text/uri-list）；跳过的 rep = {:?}",
            skipped
        );
    }

    Ok(skipped)
}

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
            let has_image = snapshot
                .representations
                .iter()
                .any(|rep| rep.mime.as_ref().is_some_and(|m| m.is_image()));

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
            // 用 `rep_bytes` 而非 `expect_inline_bytes`,后者对 LocalFile source panic
            // （远端 push 单张图时 materializer 合成 `LocalFile` image-from-file rep,
            // 一进这里就崩；见 `common::rep_bytes` 注释）。读盘失败时返回 None,fallback
            // 路径自然降级走非 image 写入。
            let image_bytes = if image_fallback_eligible {
                snapshot.representations.first().and_then(|rep| match rep_bytes(rep) {
                    Ok(b) => Some(b.into_owned()),
                    Err(err) => {
                        warn!(
                            error = %err,
                            format_id = %rep.format_id,
                            "Windows 单 rep 写入：读取 LocalFile image rep 字节失败，image fallback 不可用"
                        );
                        None
                    }
                })
            } else {
                None
            };

            // 多 rep 场景：write_snapshot_multi_windows 内部会自己 OpenClipboard（通过
            // clipboard-win）。若 clipboard-rs 的 ctx 仍持有剪贴板句柄，两者会争抢同一个
            // Windows 剪贴板句柄，产生 OSError 1418（"无法打开剪贴板"）。
            // 解决方案：提前 drop clipboard-rs ctx，再用一个临时 dummy ctx 满足
            // CommonClipboardImpl::write_snapshot 的签名要求——common.rs 内部会立即分流到
            // write_snapshot_multi_windows，不会真正使用这个 dummy ctx。
            if snapshot.representations.len() > 1 {
                // 不需要持有 ctx——多 rep 路径完全由 write_snapshot_multi_windows 接管。
                let mut dummy_ctx = ClipboardContext::new().map_err(|e| {
                    anyhow::anyhow!("创建多 rep 分发用临时 clipboard ctx 失败: {}", e)
                })?;
                return CommonClipboardImpl::write_snapshot(&mut dummy_ctx, snapshot);
            }

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
    let maybe_text_rep = snapshot
        .representations
        .iter()
        .find(|rep| rep.mime.as_ref().is_some_and(|m| m.is_text_plain()));

    let Some(text_rep) = maybe_text_rep else {
        return Ok(None);
    };

    // 用 `rep_bytes` 而非 `expect_inline_bytes`：text/plain rep 当前不会被
    // materializer 合成为 `LocalFile`，但 `expect_inline_bytes` 的契约已经在
    // 出站路径被打破（见 `common::rep_bytes`），保持本函数对 LocalFile 安全
    // 以免未来 materializer 扩展再次引入 panic。
    let bytes = rep_bytes(text_rep)?;
    let text = String::from_utf8(bytes.into_owned())
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
        .is_some_and(|m| m.is_text_plain())
}

fn is_single_image_snapshot(snapshot: &SystemClipboardSnapshot) -> bool {
    if snapshot.representations.len() != 1 {
        return false;
    }

    snapshot.representations[0]
        .mime
        .as_ref()
        .is_some_and(|m| m.is_image())
}

fn write_text_windows_native(text: &str) -> Result<()> {
    clipboard_win::set_clipboard_string(text)
        .map_err(|e| anyhow::anyhow!("Failed to set Windows Unicode clipboard text: {}", e))
}

/// Windows-specific: read CF_HTML payload via `clipboard-win`, bypassing
/// `clipboard_rs::ClipboardContext::get_html`.
///
/// **Why this exists.** Upstream `clipboard_rs::platform::win::extract_html_from_clipboard_data`
/// parses the `StartHTML`/`EndHTML` byte offsets from the CF_HTML header and
/// then does `data[start_idx..end_idx].to_string()` on a UTF-8 `&str`. Some
/// source apps (observed: Chinese-language Office, certain chat clients)
/// emit offsets that are off by 1-2 bytes when the payload contains
/// multi-byte UTF-8 characters; when the bad offset lands inside such a
/// character `std`'s string slicer aborts the process. See Sentry issue
/// UNICLIPBOARD-RUST-1V and the regression tests in
/// `crate::clipboard::cf_html::tests::cf_html_endhtml_panic_repro`.
///
/// We grab the raw CF_HTML bytes with `clipboard-win` (the lower-level crate
/// already in our dependency tree for image writes) and hand them to
/// [`super::super::cf_html::read_cf_html_payload_from_bytes`], which slices on
/// bytes and uses `String::from_utf8_lossy` so a bad offset becomes a
/// `U+FFFD` instead of a panic.
///
/// Returns `Ok(None)` when the clipboard does not carry CF_HTML (the caller
/// only reaches this function after `ctx.has(ContentFormat::Html)` reported
/// true, so this should be very rare — but is still a normal "no data"
/// outcome rather than an error).
pub(crate) fn read_html_windows_native() -> Result<Option<String>> {
    use clipboard_win::{formats, get_clipboard};

    // `Html::new()` registers (or fetches the existing) "HTML Format" code
    // via `RegisterClipboardFormatW`. `ClipboardContext::new` already did
    // this at startup, so the second registration is idempotent.
    let html_fmt = clipboard_win::formats::Html::new()
        .ok_or_else(|| anyhow::anyhow!("Failed to register CF_HTML clipboard format"))?;

    let bytes: Vec<u8> = match get_clipboard(formats::RawData(html_fmt.code())) {
        Ok(bytes) => bytes,
        Err(err) => {
            return Err(anyhow::anyhow!(
                "Failed to read CF_HTML raw bytes via clipboard-win: {}",
                err
            ));
        }
    };

    debug!(
        cf_html_size_bytes = bytes.len(),
        "Read CF_HTML raw bytes from Windows clipboard (byte-safe path)"
    );

    Ok(super::super::cf_html::read_cf_html_payload_from_bytes(
        &bytes,
    ))
}

/// Windows-specific: capture an image rep from the clipboard using the
/// fastest available path.
///
/// Layered strategy, in order of CPU cost:
///
/// 1. **Native `"PNG"` format** — zero decode, zero encode. Hits for every
///    modern screenshot source (Chrome, Firefox, Edge, Office 2019+,
///    Snipping Tool 11, Snipaste, 微信/QQ).
/// 2. **CF_DIBV5 → PNG via `dib_to_png`** — one decode + one fast PNG
///    encode (CompressionType::Fast + FilterType::NoFilter). Hits for
///    Win+PrtScr, legacy apps, and the screenshots in the production
///    Sentry trace that motivated this work.
/// 3. **`clipboard-rs::ClipboardContext::get_image() + img.to_png()`** —
///    one decode + RGBA round-trip + one default-deflate PNG encode.
///    Last resort; the slowest path. Kept only as a defense against
///    exotic image formats that neither (1) nor (2) recognize.
///
/// Mirrors the macOS fast path in `common.rs` (raw `public.png` →
/// `tiff_to_png` → `get_image()+to_png()`), which has the same shape.
///
/// Returns:
/// * `Ok(Some(png_bytes))` — one of the three tiers produced PNG bytes.
/// * `Ok(None)` — every tier reported "no image data on clipboard". The
///   caller normally won't observe this because they checked
///   `ctx.has(ContentFormat::Image)` first; if they do, treat as a
///   transient miss (lazy data provider).
/// * `Err(_)` — all three tiers failed with non-"absent" errors.
pub(crate) fn try_read_image_windows_optimized(
    ctx: &mut clipboard_rs::ClipboardContext,
) -> Result<Option<Vec<u8>>> {
    // Tier 1: native PNG format — zero encoding work.
    match read_image_windows_native_png() {
        Ok(Some(bytes)) => return Ok(Some(bytes)),
        Ok(None) => {
            // No "PNG" format on clipboard, fall through to CF_DIB.
        }
        Err(err) => {
            warn!(
                error = %err,
                "Native \"PNG\" fast-read failed; falling back to CF_DIB path"
            );
        }
    }

    // Tier 2: CF_DIB(V5) → fast PNG encode.
    match read_image_windows_as_png() {
        Ok(bytes) => return Ok(Some(bytes)),
        Err(err) => {
            // Common case: clipboard simply doesn't have CF_DIB (e.g. only a
            // text rep but the high-level `has(Image)` flag was set by a
            // stale or lazily-resolved format). Debug, not warn.
            debug!(
                error = %err,
                "CF_DIB fast path unavailable; falling back to clipboard-rs slow path"
            );
        }
    }

    // Tier 3: clipboard-rs full decode + re-encode. Last resort.
    use clipboard_rs::common::RustImage;
    use clipboard_rs::Clipboard;
    match ctx.get_image() {
        Ok(img) => match img.to_png() {
            Ok(png) => {
                let bytes = png.get_bytes().to_vec();
                debug!(
                    size_bytes = bytes.len(),
                    "Read image via clipboard-rs get_image()+to_png() (slow path)"
                );
                Ok(Some(bytes))
            }
            Err(err) => Err(anyhow::anyhow!(
                "clipboard-rs: image available but to_png() failed: {}",
                err
            )),
        },
        Err(err) => Err(anyhow::anyhow!(
            "clipboard-rs: ContentFormat::Image reported available but get_image() failed: {}",
            err
        )),
    }
}

/// Windows-specific: read the modern `"PNG"` clipboard format raw, without
/// decoding or re-encoding.
///
/// **Why this exists.** Most modern screenshot sources on Windows (Chrome,
/// Firefox, Edge, Paint.NET, Office 2019+, Snipping Tool 11, Snipaste,
/// 微信/QQ 截图工具, …) write **both** `CF_DIBV5` and a custom `"PNG"` format
/// to the clipboard — the latter carrying ready-to-use PNG bytes. The
/// existing `clipboard_rs::ClipboardContext::get_image()` path ignores this:
/// it always goes through `clipboard-rs`'s internal `RustImageData::from_bytes`
/// which decodes whatever it gets into a `DynamicImage`, after which we
/// re-encode to PNG via `to_png()`. For a 4K screenshot that round-trip
/// (PNG decode → RGBA buffer → PNG encode) costs ~3.3 s in dev profile and
/// hundreds of ms in release — entirely avoidable when the source already
/// handed us the PNG bytes.
///
/// This function mirrors the macOS fast path in `common.rs` that prefers
/// `get_buffer("public.png")` before falling back to TIFF decoding.
///
/// Returns:
/// * `Ok(Some(bytes))` — `"PNG"` format was present, raw PNG byte buffer copied.
/// * `Ok(None)` — `"PNG"` format is not on the clipboard right now (e.g. Win+PrtScr,
///   legacy app that only writes CF_DIB). The caller should fall back to the
///   CF_DIB path.
/// * `Err(_)` — registration succeeded but the format was reported available
///   and reading still failed (genuine OS error).
fn read_image_windows_native_png() -> Result<Option<Vec<u8>>> {
    use clipboard_win::{formats, get_clipboard, is_format_avail, raw as cb_raw};

    // `"PNG"` is the de-facto custom format Microsoft Office / Chrome /
    // Firefox / Paint.NET / Snipaste / 微信 all use. `register_format` is
    // idempotent — the same code is returned on every subsequent call.
    let Some(png_fmt_nz) = cb_raw::register_format("PNG") else {
        warn!("register_format(\"PNG\") returned None; native PNG fast path unavailable");
        return Ok(None);
    };
    let png_fmt = png_fmt_nz.get();

    if !is_format_avail(png_fmt) {
        // Common case for CF_DIB-only sources (Win+PrtScr, legacy apps).
        return Ok(None);
    }

    let bytes: Vec<u8> = get_clipboard(formats::RawData(png_fmt)).map_err(|e| {
        anyhow::anyhow!(
            "\"PNG\" clipboard format reported available but read failed: {}",
            e
        )
    })?;

    debug!(
        size_bytes = bytes.len(),
        "Read raw \"PNG\" format from Windows clipboard (zero-encode fast path)"
    );
    Ok(Some(bytes))
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

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::MimeType;

    fn rep(format: &str, mime: Option<&str>) -> ObservedClipboardRepresentation {
        ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from_str(format),
            mime.map(|m| MimeType(m.to_string())),
            Vec::new(),
        )
    }

    #[test]
    fn resolves_text_rtf_from_format_id() {
        // 与 common.rs::read_snapshot 写库时使用的 format_id="rtf" 对齐；
        // 与 macos.rs 同名测试镜像，保证两个平台的 multi-rep 派发结果一致。
        assert_eq!(
            resolve_multi_rep_mime(&rep("rtf", None)),
            Some(MimeClass::TextRtf)
        );
        assert_eq!(
            resolve_multi_rep_mime(&rep("public.rtf", None)),
            Some(MimeClass::TextRtf)
        );
    }

    #[test]
    fn explicit_text_rtf_mime_takes_priority_over_format_id() {
        // 显式 mime 必须优先于 format_id 推断（与 macos.rs 对称），
        // 避免被未来的 format_id 重命名意外打回 None。
        let r = rep("unknown-format-id", Some("text/rtf"));
        assert_eq!(resolve_multi_rep_mime(&r), Some(MimeClass::TextRtf));
    }

    #[test]
    fn classify_handles_parameterized_text_mime() {
        // Regression: Linux upstream advertises `text/plain;charset=utf-8`
        // (commit 388e65bf). The Windows multi-rep path previously matched
        // on the bare literal `"text/plain"` and missed the parameterized
        // form, dropping the rep into the skip-and-log branch — paste
        // would silently produce nothing on Windows when the synced rep
        // carried an explicit charset.
        let r = rep("text", Some("text/plain;charset=utf-8"));
        assert_eq!(resolve_multi_rep_mime(&r), Some(MimeClass::TextPlain));

        let r = rep("text", Some("Text/Plain; Charset=UTF-8"));
        assert_eq!(resolve_multi_rep_mime(&r), Some(MimeClass::TextPlain));
    }
}
