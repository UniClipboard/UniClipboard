use super::model::{SelectionPolicyVersion, SelectionTarget};
use crate::{
    clipboard::{
        ClipboardSelection, ObservedClipboardRepresentation, PolicyError, SystemClipboardSnapshot,
    },
    ids::RepresentationId,
    ports::SelectRepresentationPolicyPort,
};
use std::cmp::Ordering;

/// Inline `image-from-file` representations larger than this should not
/// receive the UiPreview boost — they almost certainly came from an
/// out-of-policy capture path and would slow UI render. Mirrors
/// `MAX_INLINE_IMAGE_BYTES` in uc-platform `common.rs`; if these two
/// drift, capture-time guard is the primary defence and policy is the
/// safety net.
///
/// `size_bytes()` returns `i64`, so the constant type is aligned.
const INLINE_PREVIEW_MAX_BYTES: i64 = 256 * 1024;

/// v1 策略：稳定、可解释、保守
///
/// v1 的核心：
/// - UI Preview 优先简洁预览：files > plain > image > rich > uri > unknown
/// - Default Paste 优先保留格式：files > rich > plain > image > uri > unknown
/// - stable sort: score desc, size asc, format_id asc, id asc
#[derive(Debug, Default)]
pub struct SelectRepresentationPolicyV1;

impl SelectRepresentationPolicyV1 {
    pub fn new() -> Self {
        Self
    }

    fn is_usable(rep: &ObservedClipboardRepresentation) -> bool {
        if rep.size_bytes() <= 0 {
            return false;
        }
        true
    }

    fn classify(rep: &ObservedClipboardRepresentation) -> RepKind {
        // 注意：v1 刻意不引入平台特例，只基于 mime_type + 少量 format_id 兜底
        let mime = match rep.mime.as_ref() {
            Some(m) => m,
            None => return RepKind::Unknown,
        };

        // 文件列表（常见：text/uri-list）
        if mime.eq_ignore_ascii_case("text/uri-list") || mime.starts_with("text/uri-list") {
            return RepKind::FileList;
        }

        // 图片（image/*）
        if mime.starts_with("image/") {
            return RepKind::Image;
        }

        // 富文本（html/rtf）
        if mime.eq_ignore_ascii_case("text/html") || mime.eq_ignore_ascii_case("text/rtf") {
            return RepKind::RichText;
        }

        // 纯文本（text/plain 或其他 text/*）
        if mime.eq_ignore_ascii_case("text/plain") || mime.starts_with("text/") {
            return RepKind::PlainText;
        }

        // URI（有些平台会给 text/x-uri / application/x-url 等；v1 只做轻量识别）
        if mime.contains("uri") || mime.contains("url") {
            return RepKind::Uri;
        }

        // format_id 兜底（非常保守）
        // 例如某些实现会把文件列表映射到 format_id="files"
        if rep.format_id.eq_ignore_ascii_case("files") || rep.format_id.contains("uri-list") {
            return RepKind::FileList;
        }

        RepKind::Unknown
    }

    fn score(rep: &ObservedClipboardRepresentation, kind: RepKind, target: SelectionTarget) -> i32 {
        match (target, kind) {
            // UiPreview:
            // - 从文件路径补读出的图片内容（format_id="image-from-file"）优先于 FileList，
            //   这样复制图片文件时仍然展示真实图片预览。
            // - 当 FileList 明确表示“单个图片文件”时，原始剪贴板 Image 也应优先，
            //   以便 macOS Finder 复制 PNG/JPG 等图片文件时继续展示预览。
            // - 其他场景下原始剪贴板 Image（例如普通文件复制时 Finder 自动注入的图标 TIFF）
            //   低于 FileList，避免图标抢占文件名/文件条目。
            //
            // Size guard (260426-1gz): an `image-from-file` larger than
            // `INLINE_PREVIEW_MAX_BYTES` indicates an out-of-policy capture
            // path (capture should never inline that much). Strip the 100 boost
            // so it falls back to the generic Image score (80) — FileList (95)
            // wins, matching the "small inline preview, large via thumbnail"
            // contract enforced at capture time.
            (SelectionTarget::UiPreview, RepKind::Image)
                if rep.format_id.eq_ignore_ascii_case("image-from-file")
                    && rep.size_bytes() <= INLINE_PREVIEW_MAX_BYTES =>
            {
                100
            }
            (SelectionTarget::UiPreview, RepKind::FileList) => 95,
            (SelectionTarget::UiPreview, RepKind::PlainText) => 90,
            (SelectionTarget::UiPreview, RepKind::Image) => 80,
            (SelectionTarget::UiPreview, RepKind::RichText) => 70,
            (SelectionTarget::UiPreview, RepKind::Uri) => 60,
            (SelectionTarget::UiPreview, RepKind::Unknown) => 10,

            // DefaultPaste: RichText 优先（保留格式），其次 PlainText（兼容性），最后 Image
            //
            // `image-from-file` 永远不应该成为 paste rep（260426-1gz 安全网）：
            // - DefaultPaste 决定接收端粘贴目标 + envelope 的“代表 rep”；
            //   `image-from-file` 是 capture 兜底产物（本地从 file URI 读出来的
            //   pixel buffer），让对端把它当主体写入剪贴板会绕开真正的 file
            //   transfer 通路。对端要图片应通过 `files` rep + file transfer
            //   拿原文件，而不是 inline pixel。
            // - 给 30 分（介于 Unknown=10 与所有真实 rep 之间）：保证只要 snapshot
            //   里还有任何真实可用 rep，它都不会被选成 paste；只有当整个 snapshot
            //   退化到只剩 image-from-file 时才回退到它（避免 select_one 返回 None）。
            (SelectionTarget::DefaultPaste, RepKind::Image)
                if rep.format_id.eq_ignore_ascii_case("image-from-file") =>
            {
                30
            }
            (SelectionTarget::DefaultPaste, RepKind::FileList) => 100,
            (SelectionTarget::DefaultPaste, RepKind::RichText) => 90,
            (SelectionTarget::DefaultPaste, RepKind::PlainText) => 80,
            (SelectionTarget::DefaultPaste, RepKind::Image) => 70,
            (SelectionTarget::DefaultPaste, RepKind::Uri) => 60,
            (SelectionTarget::DefaultPaste, RepKind::Unknown) => 10,
        }
    }

    fn file_list_represents_single_previewable_image(
        rep: &ObservedClipboardRepresentation,
    ) -> bool {
        if Self::classify(rep) != RepKind::FileList {
            return false;
        }

        // file-list rep 的字节是 uri-list 文本,LocalFile source 不会出现在 file-list 类别
        // (LocalFile 来自 image-from-file capture 路径,format_id 是 image-from-file 而非 files)。
        let Some(rep_bytes) = rep.inline_bytes() else {
            return false;
        };
        let bytes = match std::str::from_utf8(rep_bytes) {
            Ok(bytes) => bytes,
            Err(_) => return false,
        };

        let mut paths = bytes
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter_map(|line| url::Url::parse(line).ok()?.to_file_path().ok());

        let Some(first_path) = paths.next() else {
            return false;
        };

        if paths.next().is_some() {
            return false;
        }

        first_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff"
                )
            })
            .unwrap_or(false)
    }

    fn select_one<'a>(
        snapshot: &'a SystemClipboardSnapshot,
        target: SelectionTarget,
    ) -> Option<&'a ObservedClipboardRepresentation> {
        let mut reps: Vec<&ObservedClipboardRepresentation> = snapshot
            .representations
            .iter()
            .filter(|r| Self::is_usable(r))
            .collect();

        if reps.is_empty() {
            return None;
        }

        let has_single_previewable_image_file = snapshot
            .representations
            .iter()
            .any(Self::file_list_represents_single_previewable_image);

        reps.sort_by(|a, b| {
            let ka = Self::classify(a);
            let kb = Self::classify(b);

            // 1) 分数：desc
            let sa = if target == SelectionTarget::UiPreview
                && ka == RepKind::Image
                && has_single_previewable_image_file
            {
                100
            } else {
                Self::score(a, ka, target)
            };
            let sb = if target == SelectionTarget::UiPreview
                && kb == RepKind::Image
                && has_single_previewable_image_file
            {
                100
            } else {
                Self::score(b, kb, target)
            };
            match sb.cmp(&sa) {
                Ordering::Equal => {}
                ord => return ord,
            }

            // 2) size：asc（更轻更不容易卡 UI；paste 也更稳）
            match a.size_bytes().cmp(&b.size_bytes()) {
                Ordering::Equal => {}
                ord => return ord,
            }

            // 3) format_id：asc（保证稳定）
            match a.format_id.cmp(&b.format_id) {
                Ordering::Equal => {}
                ord => return ord,
            }

            // 4) id：asc（最终稳定）
            a.id.cmp(&b.id)
        });

        reps.into_iter().next()
    }
}

impl SelectRepresentationPolicyPort for SelectRepresentationPolicyV1 {
    fn select(
        &self,
        snapshot: &SystemClipboardSnapshot,
    ) -> Result<ClipboardSelection, PolicyError> {
        let preview = Self::select_one(snapshot, SelectionTarget::UiPreview)
            .ok_or(PolicyError::NoUsableRepresentation)?;

        let paste = Self::select_one(snapshot, SelectionTarget::DefaultPaste)
            .ok_or(PolicyError::NoUsableRepresentation)?;

        // 收集所有可用的 representations
        let usable_reps: Vec<&ObservedClipboardRepresentation> = snapshot
            .representations
            .iter()
            .filter(|r| Self::is_usable(r))
            .collect();

        // 找出除 primary 之外的其他 representation IDs
        let secondary_rep_ids: Vec<RepresentationId> = usable_reps
            .iter()
            .filter(|r| r.id != paste.id)
            .map(|r| r.id.clone())
            .collect();

        // v1：primary = paste，secondary 包含其他所有可用的 representations
        Ok(ClipboardSelection {
            primary_rep_id: paste.id.clone(),
            preview_rep_id: preview.id.clone(),
            paste_rep_id: paste.id.clone(),
            secondary_rep_ids,
            policy_version: SelectionPolicyVersion::V1,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepKind {
    FileList,
    Image,
    RichText,
    PlainText,
    Uri,
    Unknown,
}

#[cfg(test)]
mod tests {
    //! 260426-1gz size-guard 防御性回归用例。
    //!
    //! 这些用例覆盖：
    //! - DefaultPaste 永远不应让 `image-from-file` 成为 paste rep（即使 capture 端
    //!   未来又意外塞了一条大 inline image，policy 必须把它压在所有真实 rep 之下）；
    //! - UiPreview 的 100 分 boost 必须受 `INLINE_PREVIEW_MAX_BYTES` 限制——
    //!   超阈值的 inline image-from-file 退回普通 Image 分（80），让 FileList(95)
    //!   胜出，避免大 inline image 主导 UI 并阻塞渲染。
    use super::*;
    use crate::clipboard::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};
    use crate::ids::RepresentationId;
    use crate::ports::SelectRepresentationPolicyPort;

    fn rep(format_id: &str, mime: &str, bytes: Vec<u8>) -> ObservedClipboardRepresentation {
        ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            format_id.into(),
            Some(MimeType(mime.to_string())),
            bytes,
        )
    }

    /// `files` rep payload that is **not** a single previewable image URI-list.
    /// We use a non-image extension so that
    /// `file_list_represents_single_previewable_image` returns false; otherwise
    /// the legacy "single image file → boost Image to 100" UiPreview path would
    /// short-circuit our size guard for that target.
    fn non_image_files_payload() -> Vec<u8> {
        b"file:///tmp/note.txt".to_vec()
    }

    #[test]
    fn default_paste_never_picks_large_image_from_file() {
        let large_image = rep(
            "image-from-file",
            "image/png",
            vec![0u8; 1024 * 1024], // 1 MiB
        );
        let large_image_id = large_image.id.clone();

        let files = rep(
            "files",
            "text/uri-list",
            non_image_files_payload(), // ~20 bytes
        );
        let files_id = files.id.clone();

        let snapshot = SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![large_image, files],
            file_content_digests: Vec::new(),
        };

        let policy = SelectRepresentationPolicyV1::new();
        let selection = policy.select(&snapshot).expect("selection succeeds");

        assert_eq!(
            selection.paste_rep_id, files_id,
            "DefaultPaste must select `files` rep — `image-from-file` should never be paste"
        );
        assert_ne!(
            selection.paste_rep_id, large_image_id,
            "Large image-from-file must not become the paste rep"
        );
    }

    #[test]
    fn ui_preview_boost_is_size_capped_for_image_from_file() {
        // 1 MiB > INLINE_PREVIEW_MAX_BYTES (256 KiB) → no 100 boost.
        // image-from-file falls back to generic Image score (80);
        // FileList scores 95 → FileList wins UiPreview.
        let large_image = rep(
            "image-from-file",
            "image/png",
            vec![0u8; 1024 * 1024], // 1 MiB
        );
        let large_image_id = large_image.id.clone();

        let files = rep("files", "text/uri-list", non_image_files_payload());
        let files_id = files.id.clone();

        let snapshot = SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![large_image, files],
            file_content_digests: Vec::new(),
        };

        let policy = SelectRepresentationPolicyV1::new();
        let selection = policy.select(&snapshot).expect("selection succeeds");

        assert_eq!(
            selection.preview_rep_id, files_id,
            "UiPreview should pick FileList (95) over oversized image-from-file (no boost → 80)"
        );
        assert_ne!(
            selection.preview_rep_id, large_image_id,
            "Oversized image-from-file must not occupy UiPreview"
        );
    }

    #[test]
    fn ui_preview_boost_still_applies_to_small_image_from_file() {
        // Sanity check the guard does not regress the small-PNG screenshot path:
        // an `image-from-file` <= INLINE_PREVIEW_MAX_BYTES still beats FileList.
        let small_image = rep(
            "image-from-file",
            "image/png",
            vec![0u8; 64 * 1024], // 64 KiB ≤ 256 KiB
        );
        let small_image_id = small_image.id.clone();

        let files = rep("files", "text/uri-list", non_image_files_payload());

        let snapshot = SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![small_image, files],
            file_content_digests: Vec::new(),
        };

        let policy = SelectRepresentationPolicyV1::new();
        let selection = policy.select(&snapshot).expect("selection succeeds");

        assert_eq!(
            selection.preview_rep_id, small_image_id,
            "Small image-from-file should still receive UiPreview 100 boost"
        );
    }
}
