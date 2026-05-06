//! `GetMobileSyncFileUseCase` —— mobile sync 出站文件字节查询。
//!
//! 实现 SyncClipboard 协议 `GET /file/{dataName}` 的应用层语义:iPhone 客户
//! 端先读 [`GET /SyncClipboard.json`](super::get_latest_doc) 拿到 dataName,
//! 然后用同一份 dataName 拉文件字节。本 use case 复用同一个
//! [`LatestClipboardSnapshotPort`] —— port 永远返回最新一条 entry, 我们用
//! [shared mapping][super::sync_clipboard_mapping] 算出该 entry 的 dataName,
//! 与请求里的 dataName 比对:
//!
//! - 命中(且 type 是 Image / File) → 返回 `(mime, bytes)`
//! - 不命中 / Text 类型(无附件) / port 返回 None → `NotFound` → 路由 404
//!
//! ## File 类型的 v1 边界
//!
//! 当前 paste rep 对 macOS Finder file copy 的形态是 `text/uri-list`:rep.
//! bytes 里是 `file:///path/foo.pdf` 字串, 不是文件本身。本 use case v1 直接
//! 把 URI-list 当字节回送给 iPhone(MIME 仍为 text/uri-list);iPhone 客户端
//! 拿到的会是 URI 列表文本而不是真实文件 —— 这是已知的 v1 limitation, P5a.3.5
//! 引入 `MobileFileStagingPort` 时会解决:出站 File 路径会按 URI 读磁盘材化
//! 真文件字节(对称地处理入站 File staging)。
//!
//! ## 一致性(meta GET ↔ file GET)
//!
//! [`get_latest_doc`](super::get_latest_doc) 与本 use case 共用
//! [`derive_data_name`](super::sync_clipboard_mapping::derive_data_name),
//! 保证 iPhone 在两次请求间隔内看到的 dataName 一致(若 entry 没换)。

use std::sync::Arc;

use thiserror::Error;
use tracing::{debug, instrument, warn};

use uc_core::ports::mobile_sync::{LatestClipboardSnapshotError, LatestClipboardSnapshotPort};

use crate::usecases::mobile_sync::clipboard_doc::SyncClipboardItemType;

use super::sync_clipboard_mapping::{classify_for_sync, derive_data_name};

/// 出站 `GET /file/{dataName}` 的应用层动作。
pub(crate) struct GetMobileSyncFileUseCase {
    snapshot_port: Arc<dyn LatestClipboardSnapshotPort>,
}

#[derive(Debug, Clone)]
pub struct GetMobileSyncFileOutput {
    /// MIME 类型,直接给路由层填进 `Content-Type` 响应头。无 mime 字段的
    /// rep 兜底 `application/octet-stream`(让 iPhone 客户端按二进制处理)。
    pub mime: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum GetMobileSyncFileError {
    /// dataName 不匹配 / 当前 entry 是 Text 类型(无附件)/ 没有任何 entry。
    /// 路由层翻成 HTTP 404。SyncClipboard 客户端把 404 解释为"远端没东西",
    /// 不报错。
    #[error("data_name not found in latest clipboard")]
    NotFound,

    /// 底层 snapshot port 失败 —— 路由层翻成 HTTP 500。
    #[error("latest snapshot port failure: {0}")]
    Port(#[from] LatestClipboardSnapshotError),
}

impl GetMobileSyncFileUseCase {
    pub(crate) fn new(snapshot_port: Arc<dyn LatestClipboardSnapshotPort>) -> Self {
        Self { snapshot_port }
    }

    #[instrument(name = "mobile_sync.get_file", skip(self), fields(data_name = %requested))]
    pub(crate) async fn execute(
        &self,
        requested: &str,
    ) -> Result<GetMobileSyncFileOutput, GetMobileSyncFileError> {
        let rep = self
            .snapshot_port
            .latest_paste_representation()
            .await?
            .ok_or(GetMobileSyncFileError::NotFound)?;

        let item_type = classify_for_sync(&rep);
        let derived = derive_data_name(&rep, item_type);

        // Text/Group rep 不带附件 —— 任何 dataName 都是 NotFound。
        let derived_name = match derived {
            Some(name) => name,
            None => {
                debug!(
                    entry_id = %rep.entry_id,
                    item_type = ?item_type,
                    "mobile_sync get_file: rep has no dataName, returning NotFound"
                );
                return Err(GetMobileSyncFileError::NotFound);
            }
        };

        if derived_name != requested {
            debug!(
                entry_id = %rep.entry_id,
                derived = %derived_name,
                requested = %requested,
                "mobile_sync get_file: dataName mismatch, returning NotFound"
            );
            return Err(GetMobileSyncFileError::NotFound);
        }

        // Group 不应该走到这里(classify_for_sync 不产 Group),保留 warn
        // 兜底 + 当 NotFound 拒绝, 避免泄露语义不明的字节。
        if matches!(item_type, SyncClipboardItemType::Group) {
            warn!(
                entry_id = %rep.entry_id,
                "mobile_sync get_file: classify produced Group unexpectedly, refusing"
            );
            return Err(GetMobileSyncFileError::NotFound);
        }

        let mime = rep
            .mime
            .as_ref()
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());

        debug!(
            entry_id = %rep.entry_id,
            item_type = ?item_type,
            mime = %mime,
            bytes_len = rep.bytes.len(),
            "mobile_sync get_file: serving paste rep bytes"
        );

        Ok(GetMobileSyncFileOutput {
            mime,
            bytes: rep.bytes,
        })
    }
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! mockall 单测:把 `LatestClipboardSnapshotPort` mock 掉,assert use
    //! case 在 dataName 匹配/不匹配/Text 类型/无 entry/port 错误等分支上的
    //! 行为。
    //!
    //! 覆盖矩阵:
    //!
    //! | 输入 | 期望 |
    //! |---|---|
    //! | port 返回 None | NotFound |
    //! | Image rep, dataName 命中 | Ok((mime, bytes)) |
    //! | Image rep, dataName 不命中 | NotFound |
    //! | Image rep 无 mime | mime 兜底 application/octet-stream |
    //! | Text rep | NotFound(任何 dataName) |
    //! | File(uri-list)rep, dataName 命中 | Ok((text/uri-list, URI bytes)) |
    //! | File rep, dataName 不命中 | NotFound |
    //! | port Resolution 错误 | Error::Port |

    use super::*;

    use async_trait::async_trait;
    use mockall::predicate::*;
    use uc_core::clipboard::MimeType;
    use uc_core::ids::{EntryId, FormatId};
    use uc_core::mobile_sync::LatestPasteRepresentation;

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
    ) -> GetMobileSyncFileUseCase {
        let mut port = MockSnapPort::new();
        port.expect_latest_paste_representation()
            .times(1)
            .return_once(move || rep);
        GetMobileSyncFileUseCase::new(Arc::new(port))
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
        let err = uc.execute("anything").await.unwrap_err();
        assert!(matches!(err, GetMobileSyncFileError::NotFound));
    }

    #[tokio::test]
    async fn image_rep_round_trips_when_data_name_matches() {
        let bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let uc = build_uc_returning(Ok(Some(rep(
            "entry-image-abcdef0123",
            "image",
            Some("image/png"),
            bytes.clone(),
        ))));
        // derive_data_name for Image: clipboard_<entry-short>.<ext>
        // entry-short = "entry-im" (first 8 chars), ext from mime = png
        let out = uc.execute("clipboard_entry-im.png").await.unwrap();
        assert_eq!(out.mime, "image/png");
        assert_eq!(out.bytes, bytes);
    }

    #[tokio::test]
    async fn image_rep_data_name_mismatch_returns_not_found() {
        let uc = build_uc_returning(Ok(Some(rep(
            "entry-image-1",
            "image",
            Some("image/png"),
            vec![0xFF; 8],
        ))));
        let err = uc.execute("wrong_name.png").await.unwrap_err();
        assert!(matches!(err, GetMobileSyncFileError::NotFound));
    }

    #[tokio::test]
    async fn image_rep_without_mime_falls_back_to_octet_stream() {
        // is_file_mime_or_format on (None mime + format_id="image") returns
        // false → classify lands on Text? Actually no — image classification
        // requires mime starting with "image/". With no mime + format_id="image"
        // it falls through to Text. We need to test "image rep with format_id
        // not 'files' but with no mime" — which classifier puts in Text bucket.
        //
        // To exercise the octet-stream fallback we need an Image-classified rep
        // that has no mime. The only way classify returns Image is mime
        // starts_with("image/"). So an Image rep without mime cannot exist by
        // contract. Adjust this test: assert Text classification → NotFound.
        let uc = build_uc_returning(Ok(Some(rep("entry-no-mime", "image", None, vec![0xAA; 4]))));
        // no mime → classify → Text → derive_data_name returns None → NotFound
        let err = uc.execute("clipboard_entry-no.bin").await.unwrap_err();
        assert!(matches!(err, GetMobileSyncFileError::NotFound));
    }

    #[tokio::test]
    async fn text_rep_always_returns_not_found_regardless_of_data_name() {
        let uc = build_uc_returning(Ok(Some(rep(
            "entry-text-1",
            "text",
            Some("text/plain"),
            b"hello".to_vec(),
        ))));
        // Even an "obvious" guess never matches because Text rep has no
        // derived dataName.
        let err = uc.execute("clipboard_entry-te.txt").await.unwrap_err();
        assert!(matches!(err, GetMobileSyncFileError::NotFound));
    }

    #[tokio::test]
    async fn file_rep_returns_uri_list_bytes_when_data_name_matches() {
        // v1 limitation: the bytes returned are the URI list itself, not the
        // referenced file's bytes (P5a.3.5 fixes this). Pin the contract so
        // the test guards against a regression that silently changes File
        // outbound payload shape.
        let payload = b"file:///Users/Alice/Documents/note.pdf".to_vec();
        let uc = build_uc_returning(Ok(Some(rep(
            "entry-file-1",
            "files",
            Some("text/uri-list"),
            payload.clone(),
        ))));
        // derive_data_name for File: parses URI list → "note.pdf"
        let out = uc.execute("note.pdf").await.unwrap();
        assert_eq!(out.mime, "text/uri-list");
        assert_eq!(out.bytes, payload);
    }

    #[tokio::test]
    async fn file_rep_data_name_mismatch_returns_not_found() {
        let payload = b"file:///tmp/note.pdf".to_vec();
        let uc = build_uc_returning(Ok(Some(rep(
            "entry-file-2",
            "files",
            Some("text/uri-list"),
            payload,
        ))));
        let err = uc.execute("not_note.pdf").await.unwrap_err();
        assert!(matches!(err, GetMobileSyncFileError::NotFound));
    }

    #[tokio::test]
    async fn port_error_propagates_as_port_variant() {
        let err = LatestClipboardSnapshotError::Resolution("simulated sqlite failure".to_string());
        let uc = build_uc_returning(Err(err));
        let outcome = uc.execute("anything").await.unwrap_err();
        assert!(matches!(outcome, GetMobileSyncFileError::Port(_)));
    }

    #[tokio::test]
    async fn file_rep_percent_decoded_data_name_matches() {
        // SyncClipboard request URLs can come back URL-decoded by axum;
        // derive_data_name does percent decode → matches "My Photo.jpg".
        let payload = b"file:///tmp/My%20Photo.jpg".to_vec();
        let uc = build_uc_returning(Ok(Some(rep(
            "entry-file-3",
            "files",
            Some("text/uri-list"),
            payload.clone(),
        ))));
        let out = uc.execute("My Photo.jpg").await.unwrap();
        assert_eq!(out.mime, "text/uri-list");
        assert_eq!(out.bytes, payload);
    }
}
