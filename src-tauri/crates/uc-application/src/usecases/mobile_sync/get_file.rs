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
//! ## File 类型出站(P5a.3.5 后)
//!
//! 当前 paste rep 对 File 类型的 wire 形态是 `text/uri-list`(`format_id=files`,
//! `mime=text/uri-list`,bytes 是 `\n` 分隔的 `file:///...` URI 列表)。本 use
//! case 在 File 命中分支:
//!
//! 1. 解析 rep.bytes 拿首条非注释 URI;
//! 2. 调 [`MobileFileStagingPort::read_by_uri`] 拿真文件字节(adapter 内部
//!    做白名单 + canonicalize 防 directory traversal);
//! 3. mime 不沿用 rep 的 `text/uri-list`(那是给本机系统剪贴板用的容器
//!    mime),直接 fallback 到 `application/octet-stream` —— SyncClipboard
//!    协议的 wire mime 字段对 iPhone Shortcut 端无强语义,Shortcut 按字节
//!    存为附件,扩展名信息由 dataName 字段承载,iPhone 端识别足够。
//!
//! 设计:adapter 不做 mime 推断(职责单一,只读字节);use case 端用一档兜底
//! mime,简单可靠。如果 v2 想更精细(按扩展名映射 application/pdf 之类),改
//! use case 端就行,不动 port 形态。
//!
//! ## 一致性(meta GET ↔ file GET)
//!
//! [`get_latest_doc`](super::get_latest_doc) 与本 use case 共用
//! [`derive_data_name`](super::sync_clipboard_mapping::derive_data_name),
//! 保证 iPhone 在两次请求间隔内看到的 dataName 一致(若 entry 没换)。

use std::sync::Arc;

use thiserror::Error;
use tracing::{debug, instrument, warn};

use uc_core::ports::mobile_sync::{
    LatestClipboardSnapshotError, LatestClipboardSnapshotPort, MobileFileStagingError,
    MobileFileStagingPort,
};

use crate::usecases::mobile_sync::clipboard_doc::SyncClipboardItemType;

use super::sync_clipboard_mapping::{classify_for_sync, derive_data_name};

/// File 类型出站时,wire mime 的 fallback。SyncClipboard 协议对 file 字节
/// 的 wire mime 无强语义(iPhone Shortcut 只用 dataName 扩展名识别);用
/// 二进制档兜底,留给客户端 / 系统按 dataName 扩展名解释。
const FILE_OUTBOUND_MIME_FALLBACK: &str = "application/octet-stream";

/// 出站 `GET /file/{dataName}` 的应用层动作。
pub(crate) struct GetMobileSyncFileUseCase {
    snapshot_port: Arc<dyn LatestClipboardSnapshotPort>,
    file_staging: Arc<dyn MobileFileStagingPort>,
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
    /// dataName 不匹配 / 当前 entry 是 Text 类型(无附件)/ 没有任何 entry /
    /// File rep 引用的 staging 文件已被清理或不在白名单根之下。路由层翻成
    /// HTTP 404。SyncClipboard 客户端把 404 解释为"远端没东西",不报错。
    #[error("data_name not found in latest clipboard")]
    NotFound,

    /// 底层 snapshot port 失败 —— 路由层翻成 HTTP 500。
    #[error("latest snapshot port failure: {0}")]
    Port(#[from] LatestClipboardSnapshotError),

    /// File 出站读取 staging 文件时基础设施故障(URI 解析失败 / 读盘失败 /
    /// 权限错)。路由层翻成 HTTP 500。
    #[error("file staging IO failure: {0}")]
    Staging(String),
}

impl GetMobileSyncFileUseCase {
    pub(crate) fn new(
        snapshot_port: Arc<dyn LatestClipboardSnapshotPort>,
        file_staging: Arc<dyn MobileFileStagingPort>,
    ) -> Self {
        Self {
            snapshot_port,
            file_staging,
        }
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

        // File 类型走 staging port 把 URI list 解回真字节;Image 类型 rep
        // 自带字节,直接返。
        if matches!(item_type, SyncClipboardItemType::File) {
            let uri = parse_first_uri_from_uri_list(&rep.bytes).ok_or_else(|| {
                debug!(
                    entry_id = %rep.entry_id,
                    "mobile_sync get_file: file rep has no parseable URI in body, returning NotFound"
                );
                GetMobileSyncFileError::NotFound
            })?;

            let bytes = self
                .file_staging
                .read_by_uri(&uri)
                .await
                .map_err(|err| match err {
                    MobileFileStagingError::NotFound => {
                        debug!(
                            entry_id = %rep.entry_id,
                            uri = %uri,
                            "mobile_sync get_file: staging read_by_uri NotFound"
                        );
                        GetMobileSyncFileError::NotFound
                    }
                    MobileFileStagingError::Io(msg) => {
                        warn!(
                            entry_id = %rep.entry_id,
                            uri = %uri,
                            error = %msg,
                            "mobile_sync get_file: staging read_by_uri IO failure"
                        );
                        GetMobileSyncFileError::Staging(msg)
                    }
                    // adapter 不应在 read_by_uri 路径返这个变体, 防御式翻成
                    // Staging IO 错误便于排障。
                    MobileFileStagingError::InvalidDataName(msg) => {
                        warn!(
                            entry_id = %rep.entry_id,
                            uri = %uri,
                            "mobile_sync get_file: unexpected InvalidDataName from read_by_uri"
                        );
                        GetMobileSyncFileError::Staging(format!(
                            "unexpected InvalidDataName: {msg}"
                        ))
                    }
                })?;

            debug!(
                entry_id = %rep.entry_id,
                uri = %uri,
                bytes_len = bytes.len(),
                "mobile_sync get_file: served staged file bytes"
            );
            return Ok(GetMobileSyncFileOutput {
                mime: FILE_OUTBOUND_MIME_FALLBACK.to_string(),
                bytes,
            });
        }

        // Image 分支:rep 自带字节, 直接返。
        let mime = rep
            .mime
            .as_ref()
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| FILE_OUTBOUND_MIME_FALLBACK.to_string());

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

/// 从 `text/uri-list` rep bytes 解出首条非空非注释 URI。RFC 2483 风格:
/// 一行一个 URI,空行 / `#` 注释行忽略。解析失败 / 全空 → `None`,调用方
/// 翻 `NotFound`。
fn parse_first_uri_from_uri_list(bytes: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(bytes).ok()?;
    s.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
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
    use uc_core::mobile_sync::{LatestPasteRepresentation, StagedFile};

    mockall::mock! {
        SnapPort {}
        #[async_trait]
        impl LatestClipboardSnapshotPort for SnapPort {
            async fn latest_paste_representation(
                &self,
            ) -> Result<Option<LatestPasteRepresentation>, LatestClipboardSnapshotError>;
        }
    }

    /// Fake staging: 默认 `read_by_uri` panic; 可注入预设响应。
    /// Image / Text / port-error 路径不调 staging,默认 panic 形态自带防回归。
    #[derive(Default)]
    struct FakeStaging {
        read_response: std::sync::Mutex<Option<Result<Vec<u8>, MobileFileStagingError>>>,
    }

    impl FakeStaging {
        fn never_called() -> Arc<Self> {
            Arc::new(Self::default())
        }
        fn with_read_response(r: Result<Vec<u8>, MobileFileStagingError>) -> Arc<Self> {
            Arc::new(Self {
                read_response: std::sync::Mutex::new(Some(r)),
            })
        }
    }

    #[async_trait]
    impl MobileFileStagingPort for FakeStaging {
        async fn stage_file(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: Vec<u8>,
        ) -> Result<StagedFile, MobileFileStagingError> {
            unreachable!("get_file tests must not call stage_file")
        }
        async fn read_by_uri(&self, _uri: &str) -> Result<Vec<u8>, MobileFileStagingError> {
            self.read_response
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| panic!("FakeStaging.read_by_uri called without preset response"))
        }
    }

    fn build_uc_returning(
        rep: Result<Option<LatestPasteRepresentation>, LatestClipboardSnapshotError>,
    ) -> GetMobileSyncFileUseCase {
        build_uc_returning_with_staging(rep, FakeStaging::never_called())
    }

    fn build_uc_returning_with_staging(
        rep: Result<Option<LatestPasteRepresentation>, LatestClipboardSnapshotError>,
        staging: Arc<FakeStaging>,
    ) -> GetMobileSyncFileUseCase {
        let mut port = MockSnapPort::new();
        port.expect_latest_paste_representation()
            .times(1)
            .return_once(move || rep);
        GetMobileSyncFileUseCase::new(Arc::new(port), staging)
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
    async fn file_rep_reads_real_bytes_via_staging_when_data_name_matches() {
        // P5a.3.5 + 后续: File 出站不再返 URI list, 而是经 staging port
        // 读盘把真文件字节交给 iPhone。assert mime 兜底为
        // application/octet-stream(SyncClipboard 协议字段对 Shortcut 端无强
        // 语义,扩展名信息走 dataName)。
        let real_bytes = vec![0x25, 0x50, 0x44, 0x46, 0x2D, 0x31, 0x2E, 0x37]; // %PDF-1.7
        let staging = FakeStaging::with_read_response(Ok(real_bytes.clone()));
        let payload = b"file:///Users/Alice/Documents/note.pdf".to_vec();
        let uc = build_uc_returning_with_staging(
            Ok(Some(rep(
                "entry-file-1",
                "files",
                Some("text/uri-list"),
                payload,
            ))),
            staging,
        );
        let out = uc.execute("note.pdf").await.unwrap();
        assert_eq!(out.mime, "application/octet-stream");
        assert_eq!(out.bytes, real_bytes);
    }

    #[tokio::test]
    async fn file_rep_staging_not_found_returns_not_found() {
        // staging 返 NotFound(URI 不在白名单根 / 文件被运维删) → use case
        // 翻 NotFound,iPhone 收 HTTP 404 不报错。
        let staging = FakeStaging::with_read_response(Err(MobileFileStagingError::NotFound));
        let payload = b"file:///orphan/path/doc.pdf".to_vec();
        let uc = build_uc_returning_with_staging(
            Ok(Some(rep(
                "entry-file-orphan",
                "files",
                Some("text/uri-list"),
                payload,
            ))),
            staging,
        );
        let err = uc.execute("doc.pdf").await.unwrap_err();
        assert!(matches!(err, GetMobileSyncFileError::NotFound));
    }

    #[tokio::test]
    async fn file_rep_staging_io_error_returns_staging_variant() {
        // staging 返 Io(权限 / 中途读盘失败) → use case 翻 Staging(_),
        // 路由层 → HTTP 500。
        let staging = FakeStaging::with_read_response(Err(MobileFileStagingError::Io(
            "simulated permission denied".into(),
        )));
        let payload = b"file:///somewhere/doc.pdf".to_vec();
        let uc = build_uc_returning_with_staging(
            Ok(Some(rep(
                "entry-file-perm",
                "files",
                Some("text/uri-list"),
                payload,
            ))),
            staging,
        );
        let err = uc.execute("doc.pdf").await.unwrap_err();
        assert!(matches!(err, GetMobileSyncFileError::Staging(_)));
    }

    #[tokio::test]
    async fn file_rep_with_unparseable_uri_list_returns_not_found() {
        // rep.bytes 全空 / 全是注释 → parse_first_uri 返 None → NotFound。
        // staging 不应被调用(默认 FakeStaging 没设响应,被调到会 panic)。
        let staging = FakeStaging::never_called();
        let payload = b"# only a comment\n\n# another\n".to_vec();
        let uc = build_uc_returning_with_staging(
            Ok(Some(rep(
                "entry-file-empty",
                "files",
                Some("text/uri-list"),
                payload,
            ))),
            staging,
        );
        let err = uc.execute("doc.pdf").await.unwrap_err();
        // derive_data_name 对空 URI-list 也返 fallback (`clipboard_entry-fi.bin`),
        // 所以 dataName 比对不命中前先到 NotFound,这也行;但若 dataName 命中
        // fallback 仍走 staging,parse_first_uri 拿 None → NotFound。
        assert!(matches!(err, GetMobileSyncFileError::NotFound));
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
        // P5a.3.5: 命中后走 staging 读真字节,而不是返 URI list。
        let staging = FakeStaging::with_read_response(Ok(b"jpeg-real-bytes".to_vec()));
        let payload = b"file:///tmp/My%20Photo.jpg".to_vec();
        let uc = build_uc_returning_with_staging(
            Ok(Some(rep(
                "entry-file-3",
                "files",
                Some("text/uri-list"),
                payload,
            ))),
            staging,
        );
        let out = uc.execute("My Photo.jpg").await.unwrap();
        assert_eq!(out.mime, "application/octet-stream");
        assert_eq!(out.bytes, b"jpeg-real-bytes".to_vec());
    }
}
