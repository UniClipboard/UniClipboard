//! 跨平台的 rep payload 读取 helper。
//!
//! 抽出来独立成 mod 是因为 `clipboard::common` 依赖 `clipboard-rs`，Phase 4 起被
//! `#[cfg(any(target_os = "macos", target_os = "windows"))]` gate 住；Linux 的
//! 原生 Wayland / x11rb 写入路径不引入 `clipboard-rs`，但同样需要按 `LocalFile`
//! source 读盘以避免 `expect_inline_bytes` panic（见 `rep_bytes` 文档）。
//!
//! 本模块只依赖 `uc-core` + `std`，无 cfg gate，三平台均可使用。

use anyhow::{Context, Result};
use std::borrow::Cow;
use uc_core::clipboard::{ClipboardPayloadSource, ObservedClipboardRepresentation};

/// 取 rep 字节，按 source 分流。
///
/// - `Inline` → 借用现有字节，零拷贝。
/// - `LocalFile` → 同步读盘，返回 owned `Vec<u8>`。
///
/// 为什么需要这个 helper：入站 `apply_inbound::materializer` 会给图片 rep 合成
/// `LocalFile` source（指向接收端 blob cache 中已 export 的文件），同一份 snapshot
/// 在 `clipboard_capture` ingest 进 blob store 之后还会被 `ClipboardWriteCoordinator`
/// 透传到 `SystemClipboardPort::write_snapshot` 往系统剪贴板写。如果继续直调
/// `rep.expect_inline_bytes()`，对 `LocalFile` 会触发 panic（参见 `uc-core` 上
/// `expect_inline_bytes` 的契约：仅 Inline 语境），daemon 整体崩溃。本 helper
/// 显式按 source 分流，让 macOS / Windows / Linux 写入路径都能消化 `LocalFile` rep。
///
/// 同步读盘的代价：`SystemClipboardPort::write_snapshot` 本就是同步签名
/// （`ClipboardWriteCoordinator` 在 tokio worker 里直调，NSPasteboard /
/// Win32 OpenClipboard 等系统 API 本就阻塞），对端图片 blob 已由 iroh-blobs
/// export 到本地 cache，读盘 = 顺序 IO，通常 < 几十 ms，与原系统 API 调用同量级。
/// 如未来出现极大 payload 阻塞 worker 的证据再换 `spawn_blocking`，目前不预先优化。
pub(crate) fn rep_bytes(rep: &ObservedClipboardRepresentation) -> Result<Cow<'_, [u8]>> {
    match rep.source() {
        ClipboardPayloadSource::Inline(b) => Ok(Cow::Borrowed(b.as_slice())),
        ClipboardPayloadSource::LocalFile { path, .. } => std::fs::read(path)
            .map(Cow::Owned)
            .with_context(|| format!("read LocalFile rep payload at {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    //! `rep_bytes` 回归测试 —— 历史回归：远端推过来的图片走
    //! `apply_inbound::materializer` 后会带一条 `LocalFile` source 的 image rep,
    //! 旧版 OS 写入路径用 `expect_inline_bytes()` 强取 Inline 字节,对 LocalFile
    //! 直接 panic,导致 daemon 整体崩溃。这里确保 helper 能消化两种 source、
    //! 并在文件缺失时返回 Err 而非 panic。

    use super::*;
    use uc_core::clipboard::MimeType;
    use uc_core::ids::{FormatId, RepresentationId};

    #[test]
    fn rep_bytes_borrows_inline_payload() {
        let r = ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from_str("public.png"),
            Some(MimeType("image/png".to_string())),
            vec![0xDE, 0xAD, 0xBE, 0xEF],
        );
        let bytes = rep_bytes(&r).expect("inline rep_bytes 必须返回 Ok");
        assert_eq!(bytes.as_ref(), &[0xDE, 0xAD, 0xBE, 0xEF][..]);
        assert!(matches!(bytes, Cow::Borrowed(_)));
    }

    #[test]
    fn rep_bytes_reads_local_file_payload() {
        let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        let payload: &[u8] = b"\x89PNG\r\n\x1a\nfake-png-body";
        std::fs::write(tmp.path(), payload).expect("write tempfile");
        let r = ObservedClipboardRepresentation::new_local_file(
            RepresentationId::new(),
            FormatId::from_str("image-from-file"),
            Some(MimeType("image/png".to_string())),
            tmp.path().to_path_buf(),
            payload.len() as u64,
        );
        let bytes = rep_bytes(&r).expect("LocalFile rep_bytes 必须返回 Ok");
        assert_eq!(bytes.as_ref(), payload);
        assert!(matches!(bytes, Cow::Owned(_)));
    }

    #[test]
    fn rep_bytes_errors_when_local_file_missing() {
        let r = ObservedClipboardRepresentation::new_local_file(
            RepresentationId::new(),
            FormatId::from_str("image-from-file"),
            Some(MimeType("image/png".to_string())),
            std::path::PathBuf::from("/nonexistent/uc-platform-payload-rep_bytes-test.png"),
            0,
        );
        let err = rep_bytes(&r).expect_err("缺失文件必须返回 Err 而非 panic");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("/nonexistent/uc-platform-payload-rep_bytes-test.png"),
            "错误信息应包含路径，便于排障；实际: {msg}"
        );
    }
}
