//! `GetLatestMobileSyncDocUseCase` —— mobile sync 出站元数据查询。
//!
//! 负责实现 SyncClipboard 协议 `GET /SyncClipboard.json` 的应用层语义:
//! 把"最近一条剪贴板 paste-priority rep"翻成
//! [`SyncClipboardMeta`](super::clipboard_doc::SyncClipboardMeta) 给路由层
//! 序列化成 wire JSON 后回给 iPhone 客户端。
//!
//! ## 路径概览
//!
//! ```text
//! webserver GET /SyncClipboard.json
//!   ↓ Basic Auth
//!   ↓
//! MobileSyncFacade::get_latest_sync_doc           (P5a.6)
//!   ↓
//! GetLatestMobileSyncDocUseCase::execute          (本 use case)
//!   ↓
//! LatestClipboardSnapshotPort::latest_paste_representation
//!   ↓
//! adapter (P5a.8): clipboard_entry → selection → representation → blob
//! ```
//!
//! ## 类型映射(rep mime/format → SyncClipboard `type`)
//!
//! | rep 形态 | 翻成 | text 字段 | dataName | hasData |
//! |---|---|---|---|---|
//! | `text/uri-list` 或 `format_id == files` | `File` | filename | `Some(filename)` | `true` |
//! | `image/*` | `Image` | filename | `Some(filename)` | `true` |
//! | 其他 | `Text` | utf-8 内容 | `None` | `false` |
//!
//! 富文本(`text/html` / `text/rtf`)走 Text 分支:iPhone 客户端拿 HTML 当
//! 文本看,语义上仍是"可粘贴的文本",不至于让 GET 失败影响整条同步链路。
//! 真要保留富文本格式留给 v2(可加新 type 或扩 `dataName` 携带 alt 表示)。
//!
//! ## 方案 X(不区分来源)
//!
//! 本 use case 永远查"最新一条 entry",无论来源是本地复制 / mobile sync
//! 入站 / P2P 入站。dedup 由 `ApplyInbound` 的 content_hash 在入站时已经
//! 处理 —— 同一段内容多次 PUT 不会反复 capture,所以 GET 路径无需"过滤掉
//! mobile 来源避免回环",反而会让 Mac 复制的内容也无法回流给 iPhone。

use std::sync::Arc;

use sha2::{Digest, Sha256};
use thiserror::Error;
use tracing::{debug, instrument, warn};

use uc_core::clipboard::{is_file_mime_or_format, MimeType};
use uc_core::ids::{EntryId, FormatId};
use uc_core::mobile_sync::LatestPasteRepresentation;
use uc_core::ports::mobile_sync::{LatestClipboardSnapshotError, LatestClipboardSnapshotPort};

use crate::usecases::mobile_sync::clipboard_doc::{SyncClipboardItemType, SyncClipboardMeta};

/// 出站 `GET /SyncClipboard.json` 的应用层动作。
pub(crate) struct GetLatestMobileSyncDocUseCase {
    snapshot_port: Arc<dyn LatestClipboardSnapshotPort>,
}

#[derive(Debug, Error)]
pub(crate) enum GetLatestMobileSyncDocError {
    /// 当前没有任何 clipboard entry —— 路由层翻成 HTTP 404。SyncClipboard
    /// 客户端会把 404 解释为"远端还没东西",不报错。
    #[error("no clipboard entry available")]
    NotFound,

    /// 底层 snapshot port 失败 —— 路由层翻成 HTTP 500。
    #[error("latest snapshot port failure: {0}")]
    Port(#[from] LatestClipboardSnapshotError),
}

impl GetLatestMobileSyncDocUseCase {
    pub(crate) fn new(snapshot_port: Arc<dyn LatestClipboardSnapshotPort>) -> Self {
        Self { snapshot_port }
    }

    #[instrument(name = "mobile_sync.get_latest_doc", skip_all)]
    pub(crate) async fn execute(&self) -> Result<SyncClipboardMeta, GetLatestMobileSyncDocError> {
        let rep = self
            .snapshot_port
            .latest_paste_representation()
            .await?
            .ok_or(GetLatestMobileSyncDocError::NotFound)?;

        let item_type = classify_for_sync(&rep);

        let (text, data_name, has_data, size) = match item_type {
            SyncClipboardItemType::Text => {
                // SyncClipboard 协议下 Text 字段直接放 utf-8 内容。
                // from_utf8_lossy 处理偶发的非 utf8 字节(rich-text 兜底
                // 路径可能撞上),不让整条 GET 失败。
                let text = String::from_utf8_lossy(&rep.bytes).into_owned();
                let bytes_len = rep.bytes.len() as u64;
                (text, None, false, bytes_len)
            }
            SyncClipboardItemType::Image => {
                let filename = derive_image_filename(&rep);
                let bytes_len = rep.bytes.len() as u64;
                // SyncClipboard 协议下,Image/File 的 `text` 字段约定为
                // 文件名(纯展示用,客户端不解析)。
                (filename.clone(), Some(filename), true, bytes_len)
            }
            SyncClipboardItemType::File => {
                let filename = parse_first_uri_filename(&rep.bytes)
                    .unwrap_or_else(|| derive_file_fallback_filename(&rep));
                let bytes_len = rep.bytes.len() as u64;
                (filename.clone(), Some(filename), true, bytes_len)
            }
            // classify_for_sync 永远不返回 Group(SyncClipboard 协议 Group
            // 是子条目聚合,我们不主动产出);保留 match arm 让 enum 全覆盖
            // 编译期可验证。
            SyncClipboardItemType::Group => {
                warn!("classify_for_sync produced Group unexpectedly; degrading to Text");
                let text = String::from_utf8_lossy(&rep.bytes).into_owned();
                let bytes_len = rep.bytes.len() as u64;
                (text, None, false, bytes_len)
            }
        };

        // SHA-256(bytes) —— 与 PUT 路径(clipboard_doc.rs)对齐;Text 时
        // bytes 就是 text utf-8,Image/File 时 bytes 就是文件字节。
        let hash = hex::encode(Sha256::digest(&rep.bytes));

        debug!(
            entry_id = %rep.entry_id,
            item_type = ?item_type,
            data_name = ?data_name,
            size = size,
            "mobile_sync get_latest_doc: resolved meta"
        );

        Ok(SyncClipboardMeta {
            item_type,
            text,
            data_name,
            has_data,
            size,
            hash: Some(hash),
        })
    }
}

// ─── classification + filename helpers ──────────────────────────────────

fn classify_for_sync(rep: &LatestPasteRepresentation) -> SyncClipboardItemType {
    // 复用 uc-core::clipboard::is_file_mime_or_format 的同款判定 —— 与
    // capture / policy v1 链上的"file rep 识别"完全一致,避免两套规则
    // 漂移。
    if is_file_mime_or_format(rep.mime.as_ref(), &rep.format_id) {
        return SyncClipboardItemType::File;
    }
    if mime_starts_with(&rep.mime, "image/") {
        return SyncClipboardItemType::Image;
    }
    // 其余(text/* / rich-text / 完全没有 mime 的兜底)归 Text。
    SyncClipboardItemType::Text
}

fn mime_starts_with(mime: &Option<MimeType>, prefix: &str) -> bool {
    mime.as_ref()
        .is_some_and(|m| m.as_str().starts_with(prefix))
}

/// 给 Image rep 拼一个稳定的下载文件名:`clipboard_<entry_short>.<ext>`。
///
/// `<ext>` 从 mime 派生(image/png → "png",image/jpeg → "jpg",...),其他
/// 罕见 mime 兜底 "bin"。`<entry_short>` 取 entry_id 字符串前 8 个字符,
/// 让同一份 entry 的 GET /SyncClipboard.json 与 GET /file/{name} 跨请求
/// 拿到相同的 dataName,避免 iPhone 客户端 dedup 出错。
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

/// File rep 解析失败时的兜底文件名:`clipboard_<entry_short>.bin`。
///
/// 不区分具体扩展名 —— uri-list 字节解析失败说明数据本身有结构问题,
/// 兜底名只为保证 dataName 字段非空,iPhone 客户端不会拒绝响应。
fn derive_file_fallback_filename(rep: &LatestPasteRepresentation) -> String {
    format!("clipboard_{}.bin", entry_id_short(&rep.entry_id))
}

fn entry_id_short(entry_id: &EntryId) -> String {
    let s = entry_id.to_string();
    let take_n = s.len().min(8);
    s.chars().take(take_n).collect()
}

/// File rep 的字节是 RFC 2483 风格的 URI-list (text/uri-list)。规则:
/// - 一行一个 URI,空行 / `#` 注释行忽略
/// - 取首条非空非注释行作为目标 URI
/// - 解析为 URL,取最后一个 path segment 作为文件名,百分号解码
///
/// 解析失败(空 list / 非 URI / path 为空)返回 None,调用方走 fallback 名。
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

/// `url::Url::path_segments` 返回的是已经 percent-encoded 的字符串
/// (例如 `My%20Photo.png`)。手写一份小巧的 percent decode,避免引入
/// `percent-encoding` 直接依赖 —— 上游 url 0.5 不再 re-export 它。
///
/// 仅处理 ASCII 内的 `%HH`,异常序列(如 `%2X` 不合法 hex)整体保留原样。
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

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! mockall 单测:把 `LatestClipboardSnapshotPort` mock 掉,assert use
    //! case 把不同 rep 形态正确翻成 SyncClipboardMeta。
    //!
    //! 覆盖矩阵:
    //!
    //! | 输入 rep | 期望 type | 关键断言 |
    //! |---|---|---|
    //! | port 返回 None | — | NotFound |
    //! | text/plain | Text | text=utf-8 / hash=sha256(bytes) / data_name=None |
    //! | image/png | Image | data_name=clipboard_<short>.png / has_data / size |
    //! | image 兜底 ext | Image | mime image/svg+xml → .bin |
    //! | text/uri-list 单文件 | File | filename 来自 URI 末段 + 百分号解码 |
    //! | text/uri-list 空 list | File | fallback 名 .bin |
    //! | text/html (rich-text 兜底) | Text | bytes 当 utf-8 |
    //! | port 失败 | — | Error::Port |
    //! | sha256 等于已知值 | — | "hello" → 2cf24dba... |
    //! | uri-list 多行 + 注释 | File | 跳过注释行,取首条非空 |

    use super::*;

    use async_trait::async_trait;
    use mockall::predicate::*;
    use uc_core::ids::{EntryId, FormatId};

    mockall::mock! {
        SnapPort {}
        #[async_trait]
        impl LatestClipboardSnapshotPort for SnapPort {
            async fn latest_paste_representation(
                &self,
            ) -> Result<Option<LatestPasteRepresentation>, LatestClipboardSnapshotError>;
        }
    }

    fn build_uc_returning(
        rep: Result<Option<LatestPasteRepresentation>, LatestClipboardSnapshotError>,
    ) -> GetLatestMobileSyncDocUseCase {
        let mut port = MockSnapPort::new();
        port.expect_latest_paste_representation()
            .times(1)
            .return_once(move || rep);
        GetLatestMobileSyncDocUseCase::new(Arc::new(port))
    }

    fn rep(
        entry_id: &str,
        format_id: &str,
        mime: Option<&str>,
        bytes: Vec<u8>,
    ) -> LatestPasteRepresentation {
        LatestPasteRepresentation {
            entry_id: EntryId::from(entry_id),
            format_id: FormatId::from(format_id),
            mime: mime.map(|s| MimeType(s.to_string())),
            bytes,
        }
    }

    #[tokio::test]
    async fn not_found_when_port_returns_none() {
        let uc = build_uc_returning(Ok(None));
        let err = uc.execute().await.unwrap_err();
        assert!(matches!(err, GetLatestMobileSyncDocError::NotFound));
    }

    #[tokio::test]
    async fn text_round_trip_with_sha256() {
        let uc = build_uc_returning(Ok(Some(rep(
            "entry-text-1",
            "text",
            Some("text/plain"),
            b"hello".to_vec(),
        ))));
        let meta = uc.execute().await.unwrap();
        assert_eq!(meta.item_type, SyncClipboardItemType::Text);
        assert_eq!(meta.text, "hello");
        assert_eq!(meta.data_name, None);
        assert!(!meta.has_data);
        assert_eq!(meta.size, 5);
        // SHA-256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        assert_eq!(
            meta.hash.as_deref(),
            Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
        );
    }

    #[tokio::test]
    async fn image_png_filename_and_has_data() {
        let bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let uc = build_uc_returning(Ok(Some(rep(
            "entry-image-abcdef0123",
            "image",
            Some("image/png"),
            bytes.clone(),
        ))));
        let meta = uc.execute().await.unwrap();
        assert_eq!(meta.item_type, SyncClipboardItemType::Image);
        assert!(meta.has_data);
        assert_eq!(meta.size, bytes.len() as u64);
        // Filename uses first 8 chars of entry_id + .png
        assert_eq!(meta.text, "clipboard_entry-im.png");
        assert_eq!(meta.data_name.as_deref(), Some("clipboard_entry-im.png"));
        // hash = sha256(bytes)
        assert_eq!(
            meta.hash.as_deref(),
            Some(&*hex::encode(Sha256::digest(&bytes))),
        );
    }

    #[tokio::test]
    async fn image_unknown_mime_falls_back_to_bin_extension() {
        let uc = build_uc_returning(Ok(Some(rep(
            "entry-svg-001",
            "image",
            Some("image/svg+xml"),
            vec![0xAB; 16],
        ))));
        let meta = uc.execute().await.unwrap();
        assert_eq!(meta.item_type, SyncClipboardItemType::Image);
        assert_eq!(meta.data_name.as_deref(), Some("clipboard_entry-sv.bin"));
    }

    #[tokio::test]
    async fn file_uri_list_extracts_last_segment() {
        let payload = b"file:///Users/Alice/Documents/note.pdf".to_vec();
        let uc = build_uc_returning(Ok(Some(rep(
            "entry-file-1",
            "files",
            Some("text/uri-list"),
            payload.clone(),
        ))));
        let meta = uc.execute().await.unwrap();
        assert_eq!(meta.item_type, SyncClipboardItemType::File);
        assert!(meta.has_data);
        assert_eq!(meta.data_name.as_deref(), Some("note.pdf"));
        assert_eq!(meta.text, "note.pdf");
        assert_eq!(meta.size, payload.len() as u64);
    }

    #[tokio::test]
    async fn file_uri_list_percent_decodes_filename() {
        // file:///path/My%20Photo.jpg → "My Photo.jpg"
        let payload = b"file:///tmp/My%20Photo.jpg".to_vec();
        let uc = build_uc_returning(Ok(Some(rep(
            "entry-file-2",
            "files",
            Some("text/uri-list"),
            payload,
        ))));
        let meta = uc.execute().await.unwrap();
        assert_eq!(meta.data_name.as_deref(), Some("My Photo.jpg"));
    }

    #[tokio::test]
    async fn file_uri_list_skips_comments_and_blank_lines() {
        let payload = b"# comment\n\nfile:///tmp/keep.txt\nfile:///tmp/ignore.txt".to_vec();
        let uc = build_uc_returning(Ok(Some(rep(
            "entry-file-3",
            "files",
            Some("text/uri-list"),
            payload,
        ))));
        let meta = uc.execute().await.unwrap();
        assert_eq!(meta.data_name.as_deref(), Some("keep.txt"));
    }

    #[tokio::test]
    async fn file_uri_list_empty_falls_back_to_bin_name() {
        let payload = b"# only comments\n\n".to_vec();
        let uc = build_uc_returning(Ok(Some(rep(
            "entry-file-empty",
            "files",
            Some("text/uri-list"),
            payload,
        ))));
        let meta = uc.execute().await.unwrap();
        assert_eq!(meta.item_type, SyncClipboardItemType::File);
        assert_eq!(meta.data_name.as_deref(), Some("clipboard_entry-fi.bin"));
    }

    #[tokio::test]
    async fn rich_text_html_classified_as_text() {
        // text/html should NOT become File or Image; goes through Text path.
        let body = b"<p>hi</p>".to_vec();
        let uc = build_uc_returning(Ok(Some(rep(
            "entry-html-1",
            "html",
            Some("text/html"),
            body.clone(),
        ))));
        let meta = uc.execute().await.unwrap();
        assert_eq!(meta.item_type, SyncClipboardItemType::Text);
        assert_eq!(meta.text, "<p>hi</p>");
        assert_eq!(meta.data_name, None);
        assert!(!meta.has_data);
        assert_eq!(meta.size, body.len() as u64);
    }

    #[tokio::test]
    async fn port_error_propagates_as_port_variant() {
        let err = LatestClipboardSnapshotError::Resolution("simulated sqlite failure".to_string());
        let uc = build_uc_returning(Err(err));
        let outcome = uc.execute().await.unwrap_err();
        assert!(matches!(outcome, GetLatestMobileSyncDocError::Port(_)));
    }

    #[tokio::test]
    async fn format_id_files_without_uri_list_mime_still_classifies_as_file() {
        // is_file_mime_or_format treats `format_id == "files"` as File even
        // when mime is missing. Pin this contract — drives capture
        // pipeline parity (some platforms emit the rep with no explicit mime).
        let payload = b"file:///tmp/orphan.zip".to_vec();
        let uc = build_uc_returning(Ok(Some(rep("entry-files-no-mime", "files", None, payload))));
        let meta = uc.execute().await.unwrap();
        assert_eq!(meta.item_type, SyncClipboardItemType::File);
        assert_eq!(meta.data_name.as_deref(), Some("orphan.zip"));
    }
}
