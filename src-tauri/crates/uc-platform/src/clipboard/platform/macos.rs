use super::super::common::CommonClipboardImpl;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use clipboard_rs::ClipboardContext;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSPasteboard, NSPasteboardItem, NSPasteboardTypeFileURL, NSPasteboardTypeHTML,
    NSPasteboardTypePNG, NSPasteboardTypeRTF, NSPasteboardTypeString, NSPasteboardTypeTIFF,
    NSPasteboardWriting,
};
use objc2_foundation::{NSArray, NSData};
use std::sync::{Arc, Mutex};
use tracing::{debug, debug_span, info, warn};
use uc_core::clipboard::{ObservedClipboardRepresentation, SystemClipboardSnapshot};
use uc_core::ports::SystemClipboardPort;

/// macOS clipboard implementation using clipboard-rs
pub struct MacOSClipboard {
    inner: Arc<Mutex<ClipboardContext>>,
}

impl MacOSClipboard {
    pub fn new() -> Result<Self> {
        let context = ClipboardContext::new()
            .map_err(|e| anyhow::anyhow!("Failed to create clipboard context: {}", e))?;
        Ok(Self {
            inner: Arc::new(Mutex::new(context)),
        })
    }
}

#[async_trait]
impl SystemClipboardPort for MacOSClipboard {
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
        let span = debug_span!("platform.macos.read_clipboard");
        span.in_scope(|| {
            let mut ctx = self.inner.lock().unwrap();
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
            "platform.macos.write_clipboard",
            representations = snapshot.representations.len(),
        );
        span.in_scope(|| {
            let mut ctx = self.inner.lock().unwrap();
            CommonClipboardImpl::write_snapshot(&mut ctx, snapshot)?;

            debug!("Wrote clipboard snapshot to system");
            Ok(())
        })
    }
}

/// 推断 rep 在 macOS 多 rep 写入路径下的"有效 MIME"。
///
/// 与 `common.rs` 单 rep 快路径、`windows.rs::resolve_multi_rep_mime` 保持一致的
/// 推断表：显式 mime → 使用；否则 format_id 映射。
fn resolve_multi_rep_mime(rep: &ObservedClipboardRepresentation) -> Option<&str> {
    rep.mime
        .as_ref()
        .map(|m| m.as_str())
        .or_else(|| match rep.format_id.as_str() {
            "public.utf8-plain-text" | "public.text" | "NSStringPboardType" | "text" => {
                Some("text/plain")
            }
            "public.html" | "Apple HTML pasteboard type" | "html" => Some("text/html"),
            // RTF：从 Word / Pages 等富文本源复制时常与 plain text + html 同时出现；
            // common.rs::read_snapshot 把它存为 format_id="rtf"，mime="text/rtf"。
            "public.rtf" | "rtf" => Some("text/rtf"),
            // PixPin 截图 / Windows 端复制图片等场景 format_id 为 "image"，mime 通常为
            // "image/png"。`common.rs::read_snapshot` 已把图像统一标准化为 PNG，因此 jpeg /
            // webp / gif 不会出现在 envelope 中（与 windows.rs 保持同样取舍）。
            "public.png" | "image" => Some("image/png"),
            "public.tiff" => Some("image/tiff"),
            // file-list 表示：接收端 materializer 会把 rep.bytes 改写为本机 file:// URI
            // 列表（每行一条），写入时为每个 URI 生成一个独立 NSPasteboardItem 承载
            // NSPasteboardTypeFileURL —— Finder / NSDocumentController 识别的规范形式。
            "public.file-url" | "NSFilenamesPboardType" | "files" => Some("text/uri-list"),
            _ => None,
        })
}

/// 把 text/uri-list rep 的字节解析为每行一条 URI 字符串。
///
/// 空行与前后空白忽略。保留原始字符串（不做 percent-decode），因为
/// `NSPasteboardTypeFileURL` 直接接受 UTF-8 编码的 `file://...` 串。
fn parse_uri_list(bytes: &[u8]) -> Result<Vec<String>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| anyhow!("text/uri-list rep is not valid UTF-8: {}", e))?;
    let uris: Vec<String> = text
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect();
    Ok(uris)
}

/// 把 `&[u8]` 包装成 `NSData`。
///
/// 实测 API（objc2-foundation 0.3.2 `src/data.rs`）：
/// - `NSData::with_bytes(bytes: &[u8]) -> Retained<NSData>`
///   内部调用 `initWithBytes:length:`，安全封装，无需 unsafe 调用方。
///
/// 其他候选项（均已在源码中确认存在但不用于此处）：
/// - `NSData::from_vec(Vec<u8>)` —— 需要 `block2` feature，不引入该依赖
/// - `NSData::dataWithBytes_length(ptr, len)` —— unsafe，需处理裸指针
fn make_nsdata(bytes: &[u8]) -> Retained<NSData> {
    NSData::with_bytes(bytes)
}

/// macOS 原子多 representation 写入。
///
/// ## 实测 API 版本（objc2-app-kit 0.3.2 / objc2-foundation 0.3.2 / objc2 0.6.3）
///
/// 核实的关键函数签名：
/// - `NSPasteboard::generalPasteboard() -> Retained<NSPasteboard>`（关联函数，`pub fn`，无 unsafe，无 MainThreadMarker 要求）
/// - `NSPasteboard::clearContents(&self) -> NSInteger`（`pub fn`，返回 changeCount）
/// - `NSPasteboard::writeObjects(&self, objects: &NSArray<ProtocolObject<dyn NSPasteboardWriting>>) -> bool`
/// - `NSPasteboardItem::new() -> Retained<Self>`
/// - `NSPasteboardItem::setData_forType(&self, data: &NSData, type: &NSPasteboardType) -> bool`
/// - `NSPasteboardTypeString` / `NSPasteboardTypeHTML`：`extern "C" { pub static ...: &'static NSPasteboardType; }`
/// - `NSData::with_bytes(bytes: &[u8]) -> Retained<NSData>`（objc2-foundation `src/data.rs` 安全封装）
/// - `NSArray::from_retained_slice(&[Retained<T>]) -> Retained<NSArray<T>>`（objc2-foundation `src/array.rs`）
/// - `ProtocolObject::from_retained(obj: Retained<T>) -> Retained<ProtocolObject<P>>`（objc2 `src/runtime/protocol_object.rs`，`pub fn`，无 unsafe）
///
/// 线程安全：`NSPasteboard` 在 objc2-app-kit 0.3.2 中**不**声明 MainThreadOnly
/// （源码中的 `extern_class!` 宏内没有 MainThreadMarker 约束）；所有写入方法均为
/// `pub fn`（非 `pub unsafe fn`），因此可在后台线程调用，与 tokio 异步环境兼容。
/// 若未来 objc2-app-kit 升级版本引入 MainThreadOnly 约束，需改为通过 dispatch_main
/// 或 `MainThreadMarker::new_unchecked()` 处理。
///
/// ## 为何用 NSPasteboardItem + writeObjects:
///
/// macOS 真正的"原子多 rep 写入"API 是 `NSPasteboard::writeObjects:` —— 把一个
/// 承载了多个 type/data 对的 `NSPasteboardItem` 一次性提交给 pasteboard。
/// 这保证目的地应用（无论是终端、TextEdit 纯文本模式、还是富文本 Pages）在同一
/// changeCount 下看到的是**同一组 representation**，而不是被分步写入时的中间状态。
///
/// 与 Windows 路径的对比：
/// - Windows 需要"提前 drop clipboard-rs ctx + dummy_ctx 绕路"，因为 Win32 OpenClipboard
///   是独占句柄模型，不能同时被两个调用者持有。
/// - macOS `NSPasteboard::generalPasteboard()` 返回系统单例，与 `clipboard-rs` 底层
///   同样通过该单例操作，**不存在句柄争抢**，因此本函数可直接 grab generalPasteboard
///   而无需先 drop clipboard-rs ctx。
///
/// ## 支持范围
///
/// - `text/plain` → `NSPasteboardTypeString`
/// - `text/html`  → `NSPasteboardTypeHTML`
/// - `image/png`  → `NSPasteboardTypePNG`（每个 image rep 独立成一个 NSPasteboardItem，
///   原始字节直接喂给 AppKit，避免 NSImage 中转的 colorspace / alpha 翻译误差）
/// - `image/tiff` → `NSPasteboardTypeTIFF`
/// - `text/uri-list` → 每个 URI 一个独立 `NSPasteboardItem`，承载 `NSPasteboardTypeFileURL`
///   （Apple 官方推荐的多文件写入形式）
///
/// `text/rtf` → `NSPasteboardTypeRTF`（合并到 `text_item`，与 plain/html 共享同一
///   NSPasteboardItem，让目的应用从同一个 item 看到一致的多格式表示）
///
/// `image/jpeg` / `image/webp` / `image/gif` 仍不支持：上游 `common.rs::read_snapshot`
/// 已把图像统一标准化为 PNG（与 windows.rs 同步取舍），envelope 不会出现这些 mime。
///
/// ## clearContents() 副作用防御
///
/// 函数开头扫描 snapshot 是否包含至少一条可写 rep（text/plain、text/html、
/// text/uri-list、image/png 或 image/tiff）。若扫描结果为空，直接 bail——
/// **不调用 clearContents()**，避免把用户原本的 clipboard 内容抹掉却什么都写不进去
/// （与 Windows 任务的 `empty()` 副作用防御同构）。
pub(crate) fn write_snapshot_multi_macos(snapshot: SystemClipboardSnapshot) -> Result<()> {
    // 预扫描：snapshot 中至少要有一条可写 rep（text/plain、text/html、text/uri-list、
    // image/png 或 image/tiff）。否则直接 bail，不打开 / 不 clear pasteboard。
    let has_writable = snapshot.representations.iter().any(|rep| {
        matches!(
            resolve_multi_rep_mime(rep),
            Some("text/plain")
                | Some("text/html")
                | Some("text/rtf")
                | Some("text/uri-list")
                | Some("image/png")
                | Some("image/tiff")
        )
    });

    if !has_writable {
        let skipped: Vec<String> = snapshot
            .representations
            .iter()
            .map(|r| r.format_id.as_str().to_string())
            .collect();
        warn!(
            rep_count = snapshot.representations.len(),
            skipped = ?skipped,
            "macOS 多 rep 写入：无可写 rep；未清空系统 pasteboard（防副作用兜底）"
        );
        anyhow::bail!(
            "macOS 多 rep 写入：无可写 rep（支持 text/plain, text/html, text/rtf, \
             text/uri-list, image/png, image/tiff）；未清空系统 pasteboard；\
             跳过的 rep = {:?}",
            skipped
        );
    }

    // 1. 拿 general pasteboard 单例（macOS NSPasteboard 无独占句柄模型，可直接获取）
    // objc2-app-kit 0.3.2 中 generalPasteboard() 是 `pub fn`（非 unsafe），直接调用。
    let pasteboard: Retained<NSPasteboard> = NSPasteboard::generalPasteboard();

    // 2. 清空旧内容；忽略返回的 changeCount（仅标识版本号，不代表错误）
    let _ = pasteboard.clearContents();

    // 3. 构造 items。
    //
    // Apple 文档：多文件写入应为每个 file URL 创建独立 NSPasteboardItem，每个 item
    // 承载一个 NSPasteboardTypeFileURL（`public.file-url`，value 为 UTF-8 编码的
    // `file://...` 字符串字节）。
    //
    // 我们为 text/plain + text/html 创建一个 "文本 item"（合并承载多个 type），这样
    // 纯文本目的地（TextEdit / Terminal）从一个 item 就能拿到文本；而 files 按 Apple
    // 规范各自一个 item。
    let text_item: Retained<NSPasteboardItem> = NSPasteboardItem::new();
    let mut image_items: Vec<Retained<NSPasteboardItem>> = Vec::new();
    let mut file_items: Vec<Retained<NSPasteboardItem>> = Vec::new();

    let mut wrote_any = false;
    let mut skipped: Vec<String> = Vec::new();
    let mut file_uri_total = 0usize;
    let mut image_total = 0usize;

    for rep in &snapshot.representations {
        match resolve_multi_rep_mime(rep) {
            Some("text/plain") => {
                // text/plain 的字节是 UTF-8，NSPasteboardTypeString 期望 UTF-8 字节，
                // 直接写原始字节，不经 NSString 转换（避免对非法 UTF-8 误报）。
                let data = make_nsdata(&rep.bytes);
                // NSPasteboardTypeString 是 extern "C" 静态变量，访问需要 unsafe 块。
                // setData_forType 本身是 `pub fn`（安全方法）。
                let ok = unsafe { text_item.setData_forType(&data, NSPasteboardTypeString) };
                if ok {
                    debug!(bytes = rep.bytes.len(), "写入 NSPasteboardTypeString 成功");
                    wrote_any = true;
                } else {
                    warn!(
                        bytes = rep.bytes.len(),
                        "setData_forType(NSPasteboardTypeString) 返回 false"
                    );
                    skipped.push(rep.format_id.as_str().to_string());
                }
            }
            Some("text/html") => {
                let data = make_nsdata(&rep.bytes);
                // NSPasteboardTypeHTML 是 extern "C" 静态变量，访问需要 unsafe 块。
                let ok = unsafe { text_item.setData_forType(&data, NSPasteboardTypeHTML) };
                if ok {
                    debug!(bytes = rep.bytes.len(), "写入 NSPasteboardTypeHTML 成功");
                    wrote_any = true;
                } else {
                    warn!(
                        bytes = rep.bytes.len(),
                        "setData_forType(NSPasteboardTypeHTML) 返回 false"
                    );
                    skipped.push(rep.format_id.as_str().to_string());
                }
            }
            Some("text/rtf") => {
                // RTF 与 plain/html 同属"同一份内容的多种文本表示"，合并到 text_item。
                // Word / Pages / 写字板等富文本目的地优先读 RTF；纯文本目的地（终端 /
                // TextEdit 纯文本模式）继续用 NSPasteboardTypeString。原始 RTF 字节是
                // ASCII 安全的（RTF 1.x 规范，非 ASCII 都做 \uN 转义），直接喂给 NSData。
                // NSPasteboardTypeRTF 是 extern "C" 静态变量，访问需要 unsafe 块。
                let data = make_nsdata(&rep.bytes);
                let ok = unsafe { text_item.setData_forType(&data, NSPasteboardTypeRTF) };
                if ok {
                    debug!(bytes = rep.bytes.len(), "写入 NSPasteboardTypeRTF 成功");
                    wrote_any = true;
                } else {
                    warn!(
                        bytes = rep.bytes.len(),
                        "setData_forType(NSPasteboardTypeRTF) 返回 false"
                    );
                    skipped.push(rep.format_id.as_str().to_string());
                }
            }
            Some("image/png") => {
                // image rep 独立成一个 NSPasteboardItem，不合并进 text_item。
                // 同一 item 的多个 type 在 NSPasteboard 语义里表达"同一份内容的多种表示"，
                // 把 PNG bytes 与 plain text 混在一个 item 会让 reader 误判一致性。
                // PNG 字节直接喂给 NSPasteboardTypePNG，AppKit 内部 lazy-decode，比
                // 经 NSImage 中转更稳（避免 alpha / colorspace 翻译误差）。
                let item: Retained<NSPasteboardItem> = NSPasteboardItem::new();
                let data = make_nsdata(&rep.bytes);
                // NSPasteboardTypePNG 是 extern "C" 静态变量，访问需要 unsafe 块。
                let ok = unsafe { item.setData_forType(&data, NSPasteboardTypePNG) };
                if ok {
                    debug!(bytes = rep.bytes.len(), "写入 NSPasteboardTypePNG 成功");
                    wrote_any = true;
                    image_total += 1;
                    image_items.push(item);
                } else {
                    warn!(
                        bytes = rep.bytes.len(),
                        "setData_forType(NSPasteboardTypePNG) 返回 false"
                    );
                    skipped.push(rep.format_id.as_str().to_string());
                }
            }
            Some("image/tiff") => {
                let item: Retained<NSPasteboardItem> = NSPasteboardItem::new();
                let data = make_nsdata(&rep.bytes);
                // NSPasteboardTypeTIFF 是 extern "C" 静态变量，访问需要 unsafe 块。
                let ok = unsafe { item.setData_forType(&data, NSPasteboardTypeTIFF) };
                if ok {
                    debug!(bytes = rep.bytes.len(), "写入 NSPasteboardTypeTIFF 成功");
                    wrote_any = true;
                    image_total += 1;
                    image_items.push(item);
                } else {
                    warn!(
                        bytes = rep.bytes.len(),
                        "setData_forType(NSPasteboardTypeTIFF) 返回 false"
                    );
                    skipped.push(rep.format_id.as_str().to_string());
                }
            }
            Some("text/uri-list") => {
                let uris = match parse_uri_list(&rep.bytes) {
                    Ok(list) => list,
                    Err(e) => {
                        warn!(
                            error = %e,
                            bytes = rep.bytes.len(),
                            format_id = %rep.format_id,
                            "macOS 多 rep 写入：text/uri-list 解析失败，跳过该 rep"
                        );
                        skipped.push(rep.format_id.as_str().to_string());
                        continue;
                    }
                };
                if uris.is_empty() {
                    info!(
                        format_id = %rep.format_id,
                        "macOS 多 rep 写入：text/uri-list 为空，跳过该 rep"
                    );
                    skipped.push(rep.format_id.as_str().to_string());
                    continue;
                }
                file_uri_total += uris.len();
                for uri in uris {
                    let file_item: Retained<NSPasteboardItem> = NSPasteboardItem::new();
                    let data = make_nsdata(uri.as_bytes());
                    // NSPasteboardTypeFileURL 是 extern "C" 静态变量，访问需要 unsafe 块。
                    let ok = unsafe { file_item.setData_forType(&data, NSPasteboardTypeFileURL) };
                    if ok {
                        debug!(uri_bytes = uri.len(), "写入 NSPasteboardTypeFileURL 成功");
                        wrote_any = true;
                        file_items.push(file_item);
                    } else {
                        warn!(
                            uri = %uri,
                            "setData_forType(NSPasteboardTypeFileURL) 返回 false，跳过该 URI"
                        );
                    }
                }
            }
            other => {
                info!(
                    format_id = %rep.format_id,
                    mime = ?other,
                    bytes = rep.bytes.len(),
                    "macOS 多 rep 写入：跳过不支持的 rep（当前支持 text/plain, text/html, text/rtf, text/uri-list, image/png, image/tiff）"
                );
                skipped.push(rep.format_id.as_str().to_string());
            }
        }
    }

    if !wrote_any {
        // 所有候选 rep 的 setData_forType 均返回 false —— 极罕见。
        // 此时 item 为空，writeObjects 不会提交有意义内容，但 clearContents 已执行。
        anyhow::bail!(
            "macOS 多 rep 写入：所有候选 rep setData_forType 均失败；\
             pasteboard 已被清空但无法写入；跳过的 rep = {:?}",
            skipped
        );
    }

    // 4. 把所有 NSPasteboardItem 打包为 NSArray<ProtocolObject<dyn NSPasteboardWriting>>。
    //
    // 实测 API（objc2 0.6.3）：
    // - `ProtocolObject::from_retained(obj: Retained<T>) -> Retained<ProtocolObject<P>>`
    //   （`pub fn`，内部为 `Retained::cast_unchecked`，编译期检查 T: ImplementedBy<P>）
    // - `NSArray::from_retained_slice(&[Retained<T>]) -> Retained<NSArray<T>>`
    //   （objc2-foundation `src/array.rs`）
    //   注意：此处需要先把所有 item 收进 Vec，再取切片。
    //
    // Item 顺序：text_item 在前（text/plain + text/html），image_items 居中，
    // file_items 在后。NSPasteboard::writeObjects 会原子地把整个数组提交到
    // pasteboard；目的地应用按需选择各 item 的对应 type。
    //
    // 注意：text_item 即使为空也一并提交 —— 只有 image / file rep 的场景下它不会
    // 携带 string/html type，readers 自然不会从中拿到内容，与只写 image_items /
    // file_items 等价。真实场景里几乎总会伴随至少一种文本表示（参见 materialize
    // 注入的 rep_count >= 2）。
    let mut items_vec: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> =
        Vec::with_capacity(1 + image_items.len() + file_items.len());
    items_vec.push(ProtocolObject::from_retained(text_item));
    for item in image_items {
        items_vec.push(ProtocolObject::from_retained(item));
    }
    for item in file_items {
        items_vec.push(ProtocolObject::from_retained(item));
    }
    let items_array: Retained<NSArray<ProtocolObject<dyn NSPasteboardWriting>>> =
        NSArray::from_retained_slice(&items_vec);

    // 5. 原子提交；writeObjects: 返回 false 表示失败，上抛 Err
    // objc2-app-kit 0.3.2 中 writeObjects 是 `pub fn`（非 unsafe）。
    let ok = pasteboard.writeObjects(&items_array);
    if !ok {
        return Err(anyhow!(
            "NSPasteboard.writeObjects 返回 false；pasteboard 可能处于不一致状态\
             （已 clearContents 但本次写入失败）"
        ));
    }

    if !skipped.is_empty() {
        debug!(
            skipped_count = skipped.len(),
            skipped = ?skipped,
            "macOS 多 rep 写入：部分 rep 已跳过（不支持或 setData 失败）"
        );
    }

    info!(
        total_reps = snapshot.representations.len(),
        skipped_count = skipped.len(),
        file_uri_total,
        image_total,
        "macOS 原子多 rep 写入完成"
    );

    Ok(())
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
    fn resolves_image_png_from_format_id() {
        // PixPin 截图 / common.rs 标准化后的 format_id 路径。
        assert_eq!(
            resolve_multi_rep_mime(&rep("public.png", None)),
            Some("image/png")
        );
        assert_eq!(
            resolve_multi_rep_mime(&rep("image", None)),
            Some("image/png")
        );
    }

    #[test]
    fn resolves_image_tiff_from_format_id() {
        assert_eq!(
            resolve_multi_rep_mime(&rep("public.tiff", None)),
            Some("image/tiff")
        );
    }

    #[test]
    fn explicit_image_mime_takes_priority_over_format_id() {
        // 显式 mime 优先于 format_id 推断（与 windows.rs 对称）。
        let r = rep("unknown-format-id", Some("image/png"));
        assert_eq!(resolve_multi_rep_mime(&r), Some("image/png"));
    }

    #[test]
    fn resolves_text_rtf_from_format_id() {
        // 与 common.rs::read_snapshot 写库时使用的 format_id="rtf" 对齐。
        assert_eq!(resolve_multi_rep_mime(&rep("rtf", None)), Some("text/rtf"));
        assert_eq!(
            resolve_multi_rep_mime(&rep("public.rtf", None)),
            Some("text/rtf")
        );
    }

    #[test]
    fn explicit_text_rtf_mime_takes_priority_over_format_id() {
        // 上游（common.rs）总会给 RTF rep 显式打 mime="text/rtf"；显式 mime 必须优先
        // 于 format_id 推断，避免被未来的 format_id 重命名意外打回 None。
        let r = rep("unknown-format-id", Some("text/rtf"));
        assert_eq!(resolve_multi_rep_mime(&r), Some("text/rtf"));
    }

    #[test]
    fn windows_private_reps_remain_unsupported() {
        // 这些 Windows 平台私有 rep 必须继续返回 None，让多 rep 写入器跳过它们
        // 而不是误把它们当作可写——issue #484 期望行为里明确这一点。
        assert_eq!(resolve_multi_rep_mime(&rep("DataObject", None)), None);
        assert_eq!(resolve_multi_rep_mime(&rep("PixPinData", None)), None);
        assert_eq!(resolve_multi_rep_mime(&rep("Ole Private Data", None)), None);
    }
}
