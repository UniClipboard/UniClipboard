//! Use case for listing clipboard entry projections with cross-repo aggregation.

use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, warn};
use uc_core::clipboard::link_utils::{detect_link_urls as detect_web_urls, parse_uri_list};
use uc_core::clipboard::{
    ClipboardEntryContentCategory, MimeType, PayloadAvailability, PersistedClipboardRepresentation,
};
use uc_core::ports::clipboard::{
    EntryFileSetRepositoryPort, GetRepresentationPort, ListClipboardEntriesPort,
};
use uc_core::ports::{
    ClipboardSelectionRepositoryPort, GetEntryTransferSummaryPort, ThumbnailRepositoryPort,
};
use uc_core::search::document::ContentType;
use uc_core::search::tag::TaggableContent;

use crate::content_tags::evaluate_builtin_content_tags;
use crate::file_set_query::load_has_directory_structure;

/// Application-layer DTO for clipboard entry projection.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EntryProjectionDto {
    pub(crate) id: String,
    pub(crate) preview: String,
    pub(crate) has_detail: bool,
    pub(crate) size_bytes: i64,
    pub(crate) captured_at: i64,
    pub(crate) content_type: String,
    pub(crate) thumbnail_url: Option<String>,
    pub(crate) is_encrypted: bool,
    pub(crate) is_favorited: bool,
    pub(crate) updated_at: i64,
    pub(crate) active_time: i64,
    pub(crate) file_transfer_status: Option<String>,
    pub(crate) file_transfer_reason: Option<String>,
    pub(crate) file_transfer_ids: Vec<String>,
    pub(crate) content_tags: Vec<String>,
    pub(crate) link_urls: Option<Vec<String>>,
    pub(crate) link_domains: Option<Vec<String>>,
    pub(crate) file_sizes: Option<Vec<i64>>,
    pub(crate) image_width: Option<i32>,
    pub(crate) image_height: Option<i32>,
    /// Whether this file entry was captured as a directory (its manifest carries
    /// members expanded from a directory root). Sourced from the single
    /// `EntryFileSet::has_directory_structure()` authority. The sender UI hides
    /// the (per-member-aliased, thus meaningless) byte percentage for directory
    /// sends and renders status only; single-file / flat-multi-file entries keep
    /// their real percentage. `false` for non-file entries and when the manifest
    /// is absent or unreadable.
    pub(crate) is_directory: bool,
    /// `paste_rep` 的 payload_state, 仅在该 representation 已不可恢复时输出
    /// (`Lost`)。其他状态返回 `None` —— 由前端默认渲染。这是面向 UI 列表
    /// "灰显损坏 entry" 的最小信号；list 渲染基于 preview_rep, 但实际粘贴
    /// 用的是 paste_rep, 必须用 paste_rep 的 state 判断"点了能不能用"。
    pub(crate) payload_state: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ListProjectionsError {
    #[error("Invalid limit: {0}")]
    InvalidLimit(String),

    #[error("Repository error: {0}")]
    RepositoryError(String),
}

pub(crate) struct ListClipboardEntryProjectionsUseCase {
    entry_repo: Arc<dyn ListClipboardEntriesPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    representation_repo: Arc<dyn GetRepresentationPort>,
    thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
    file_transfer_repo: Arc<dyn GetEntryTransferSummaryPort>,
    entry_file_set_repo: Arc<dyn EntryFileSetRepositoryPort>,
    max_limit: usize,
}

/// Detect web (http/https) URLs carried by a representation, delegating to the
/// uc-core single contract so the list projection and the search `link` tag
/// agree on what counts as a link (§4.5). Returns `None` when no web URL is
/// present or the data is not URL-bearing text.
fn detect_link_urls(content_type: &str, inline_data: Option<&[u8]>) -> Option<Vec<String>> {
    let full_text = inline_data.and_then(|d| std::str::from_utf8(d).ok())?;
    let ct = content_type.to_ascii_lowercase();
    let urls = if ct.starts_with("text/uri-list") {
        detect_web_urls(&parse_uri_list(full_text), None)
    } else if ct.starts_with("text/plain") {
        detect_web_urls(&[], Some(full_text))
    } else {
        Vec::new()
    };
    if urls.is_empty() {
        None
    } else {
        Some(urls)
    }
}

fn compute_file_sizes(inline_data: &[u8]) -> Vec<i64> {
    let text = match std::str::from_utf8(inline_data) {
        Ok(t) => t,
        Err(_) => return vec![],
    };
    parse_uri_list(text)
        .iter()
        .map(|uri| match url::Url::parse(uri) {
            Ok(parsed) if parsed.scheme() == "file" => match parsed.to_file_path() {
                Ok(path) => match std::fs::metadata(&path) {
                    Ok(meta) => meta.len() as i64,
                    Err(_) => -1,
                },
                Err(_) => -1,
            },
            _ => -1,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UriListPreview {
    text: String,
    bytes: Vec<u8>,
}

/// Whether a representation's MIME marks it as a `uri-list` (the file-list paste
/// format). Gates whether the representation's inline bytes may serve as a File
/// entry's parseable preview.
fn mime_is_uri_list(rep: &PersistedClipboardRepresentation) -> bool {
    rep.mime_type.as_ref().is_some_and(MimeType::is_uri_list)
}

/// The parsed uri-list carried inline by `rep`, or `None` when `rep` is not a
/// uri-list representation or has no non-empty URI entries.
fn uri_list_preview(rep: &PersistedClipboardRepresentation) -> Option<UriListPreview> {
    if !mime_is_uri_list(rep) {
        return None;
    }
    let inline_data = rep.inline_data.as_ref()?;
    let text = std::str::from_utf8(inline_data).ok()?;
    let uris = parse_uri_list(text);
    if uris.is_empty() {
        None
    } else {
        let text = uris.join("\n");
        Some(UriListPreview {
            bytes: text.as_bytes().to_vec(),
            text,
        })
    }
}

fn file_uri_list_preview(
    content_category: ClipboardEntryContentCategory,
    preview_rep: &PersistedClipboardRepresentation,
    paste_rep: &PersistedClipboardRepresentation,
) -> Option<UriListPreview> {
    if content_category == ClipboardEntryContentCategory::File {
        uri_list_preview(preview_rep).or_else(|| uri_list_preview(paste_rep))
    } else {
        None
    }
}

/// Resolve the list-view preview text for an entry.
///
/// A File entry must expose its `uri-list` here, because the client parses real
/// file names off `preview`. Most File entries already do — their `preview_rep`
/// *is* the uri-list. The exception is a copied image *file*: it is categorised
/// `File`, yet the UI-preview representation policy picks the image itself as the
/// `preview_rep` (an image is a nicer thumbnail than a path). That rep carries no
/// uri-list text, so without this fallback the preview would be the
/// `"Image (N bytes)"` placeholder, the client would parse no file name, and a
/// single image file would render as a plain file card — self-correcting only on
/// reload, which reads the search projection's own file-name list. Falling back
/// to the `paste_rep`'s uri-list keeps the list view and the search view in
/// agreement.
fn resolve_preview_text(
    content_category: ClipboardEntryContentCategory,
    preview_rep: &PersistedClipboardRepresentation,
    is_image: bool,
    paste_rep: &PersistedClipboardRepresentation,
    image_dimensions: Option<(i32, i32)>,
) -> String {
    if let Some(uri_list) = file_uri_list_preview(content_category, preview_rep, paste_rep) {
        return uri_list.text;
    }
    if let Some(data) = preview_rep.inline_data.as_ref() {
        return String::from_utf8_lossy(data).trim().to_string();
    }
    if is_image {
        return image_placeholder(image_dimensions, preview_rep.size_bytes);
    }
    "Text content (full payload in background processing)".to_string()
}

/// Placeholder preview for an image entry that carries no inline text. Prefers
/// the original pixel dimensions (`W×H`); falls back to a byte count when the
/// dimensions are unknown (e.g. thumbnail metadata missing).
fn image_placeholder(dimensions: Option<(i32, i32)>, size_bytes: i64) -> String {
    match dimensions {
        Some((width, height)) if width > 0 && height > 0 => {
            format!("Image ({}×{})", width, height)
        }
        _ => format!("Image ({} bytes)", size_bytes),
    }
}

fn content_type_from_entry_category(category: ClipboardEntryContentCategory) -> ContentType {
    match category {
        ClipboardEntryContentCategory::Text => ContentType::Text,
        ClipboardEntryContentCategory::Image => ContentType::Image,
        ClipboardEntryContentCategory::File => ContentType::File,
        ClipboardEntryContentCategory::RichText => ContentType::Html,
        ClipboardEntryContentCategory::Other => ContentType::Other,
    }
}

fn content_tags_for_projection(
    category: ClipboardEntryContentCategory,
    content_type: &str,
    inline_data: Option<&[u8]>,
    has_image: bool,
) -> Vec<String> {
    let lower = content_type.to_ascii_lowercase();
    let text = inline_data.and_then(|d| std::str::from_utf8(d).ok());
    let uri_list = if lower.starts_with("text/uri-list") || lower.starts_with("file/uri-list") {
        text.map(parse_uri_list).unwrap_or_default()
    } else {
        Vec::new()
    };
    let plain_text = if lower.starts_with("text/plain") {
        text
    } else {
        None
    };

    evaluate_builtin_content_tags(&TaggableContent {
        content_type: content_type_from_entry_category(category),
        uri_list: &uri_list,
        plain_text,
        has_image,
        has_directory: false,
    })
    .into_iter()
    .map(|tag| tag.to_string())
    .collect()
}

impl ListClipboardEntryProjectionsUseCase {
    pub(crate) fn new(
        entry_repo: Arc<dyn ListClipboardEntriesPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        representation_repo: Arc<dyn GetRepresentationPort>,
        thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
        file_transfer_repo: Arc<dyn GetEntryTransferSummaryPort>,
        entry_file_set_repo: Arc<dyn EntryFileSetRepositoryPort>,
    ) -> Self {
        Self {
            entry_repo,
            selection_repo,
            representation_repo,
            thumbnail_repo,
            file_transfer_repo,
            entry_file_set_repo,
            max_limit: 1000,
        }
    }

    pub(crate) async fn execute(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<EntryProjectionDto>, ListProjectionsError> {
        if limit == 0 {
            return Err(ListProjectionsError::InvalidLimit(format!(
                "Must be at least 1, got {}",
                limit
            )));
        }

        if limit > self.max_limit {
            return Err(ListProjectionsError::InvalidLimit(format!(
                "Must be at most {}, got {}",
                self.max_limit, limit
            )));
        }

        let entries = self
            .entry_repo
            .list_entries(limit, offset)
            .await
            .map_err(|e| ListProjectionsError::RepositoryError(e.to_string()))?;

        let mut projections = Vec::with_capacity(entries.len());

        for entry in entries {
            let entry_id_str = entry.entry_id.inner().clone();
            let event_id_str = entry.event_id.inner().clone();
            let captured_at = entry.created_at_ms;
            let active_time = entry.active_time_ms;

            let selection = match self.selection_repo.get_selection(&entry.entry_id).await {
                Ok(Some(selection)) => selection,
                Ok(None) => {
                    warn!(
                        entry_id = %entry_id_str,
                        "Skipping entry without selection while listing projections"
                    );
                    continue;
                }
                Err(e) => {
                    warn!(
                        entry_id = %entry_id_str,
                        error = %e,
                        "Skipping entry due to selection lookup failure"
                    );
                    continue;
                }
            };

            let preview_rep_id = selection.selection.preview_rep_id.inner().clone();
            let representation = match self
                .representation_repo
                .get_representation(&entry.event_id, &selection.selection.preview_rep_id)
                .await
            {
                Ok(Some(rep)) => rep,
                Ok(None) => {
                    warn!(
                        event_id = %event_id_str,
                        preview_rep_id = %preview_rep_id,
                        "Skipping entry because preview representation is missing"
                    );
                    continue;
                }
                Err(e) => {
                    warn!(
                        event_id = %event_id_str,
                        preview_rep_id = %preview_rep_id,
                        error = %e,
                        "Skipping entry due to preview representation lookup failure"
                    );
                    continue;
                }
            };

            let is_image = representation
                .mime_type
                .as_ref()
                .is_some_and(MimeType::is_image);

            // Fetch the paste_rep once when it differs from the preview_rep. It
            // serves two consumers below: the uri-list preview text for File
            // entries whose preview_rep carries no parseable names (image files),
            // and the paste_rep Lost-state signal. When preview_rep == paste_rep
            // we reuse the already-fetched `representation`.
            let same_paste_rep =
                selection.selection.paste_rep_id == selection.selection.preview_rep_id;
            let fetched_paste_rep: Option<PersistedClipboardRepresentation> = if same_paste_rep {
                None
            } else {
                match self
                    .representation_repo
                    .get_representation(&entry.event_id, &selection.selection.paste_rep_id)
                    .await
                {
                    Ok(Some(rep)) => Some(rep),
                    Ok(None) => None,
                    Err(e) => {
                        warn!(
                            entry_id = %entry_id_str,
                            paste_rep_id = %selection.selection.paste_rep_id,
                            error = %e,
                            "Failed to fetch paste_rep for projection; treating as healthy"
                        );
                        None
                    }
                }
            };
            let paste_rep_ref = fetched_paste_rep.as_ref().unwrap_or(&representation);

            // Resolve the thumbnail + original image dimensions before the preview
            // text, so an image entry's placeholder preview can show its pixel
            // dimensions (W×H) rather than a raw byte count.
            let (thumbnail_url, image_width, image_height) = if is_image {
                match self
                    .thumbnail_repo
                    .get_by_representation_id(&selection.selection.preview_rep_id)
                    .await
                {
                    Ok(Some(metadata)) => (
                        Some(format!("/clipboard/thumbnails/{}", preview_rep_id)),
                        Some(metadata.original_width),
                        Some(metadata.original_height),
                    ),
                    Ok(None) => (None, None, None),
                    Err(err) => {
                        tracing::error!(
                            error = %err,
                            entry_id = %entry_id_str,
                            "Failed to fetch thumbnail metadata"
                        );
                        (None, None, None)
                    }
                }
            } else {
                (None, None, None)
            };

            let preview = resolve_preview_text(
                entry.content_category,
                &representation,
                is_image,
                paste_rep_ref,
                image_width.zip(image_height),
            );
            let file_uri_list =
                file_uri_list_preview(entry.content_category, &representation, paste_rep_ref);

            let preview_mime = representation
                .mime_type
                .as_ref()
                .map(|mt| mt.as_str().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let link_urls = detect_link_urls(&preview_mime, representation.inline_data.as_deref());
            let content_tags = content_tags_for_projection(
                entry.content_category,
                &preview_mime,
                representation.inline_data.as_deref(),
                is_image,
            );

            let file_sizes = file_uri_list
                .as_ref()
                .map(|uri_list| compute_file_sizes(&uri_list.bytes));

            let has_detail = representation.blob_id.is_some()
                || matches!(
                    representation.payload_state(),
                    PayloadAvailability::Staged | PayloadAvailability::Processing
                );

            // The paste_rep is the representation written to the system
            // clipboard when users restore an entry. If it degrades to Lost,
            // restore returns 410, so the list greys the row out before click.
            // Reuse the paste_rep fetched above: preview_rep state when both
            // ids match, paste_rep state when it was loaded, and a transparent
            // healthy default when it is missing or unreadable.
            let paste_rep_state = if same_paste_rep {
                Some(representation.payload_state.clone())
            } else {
                fetched_paste_rep
                    .as_ref()
                    .map(|rep| rep.payload_state.clone())
            };
            let payload_state = paste_rep_state_to_payload_state(paste_rep_state.as_ref());

            let (file_transfer_status, file_transfer_reason, file_transfer_ids) = match self
                .file_transfer_repo
                .get_entry_transfer_summary(&entry_id_str)
                .await
            {
                Ok(Some(summary)) => (
                    Some(summary.aggregate_status.as_str().to_string()),
                    summary.failure_reason,
                    summary.transfer_ids,
                ),
                Ok(None) => (None, None, vec![]),
                Err(e) => {
                    warn!(
                        entry_id = %entry_id_str,
                        error = %e,
                        "Failed to query file transfer summary for entry in list"
                    );
                    (None, None, vec![])
                }
            };

            // Only file entries can carry a directory manifest; skip the extra
            // repo read for text/image/etc. Mirror the search projection's
            // degradation: on a load error or absent manifest, project as
            // non-directory rather than failing the whole list.
            let is_directory = if entry.content_category == ClipboardEntryContentCategory::File {
                load_has_directory_structure(self.entry_file_set_repo.as_ref(), &entry.entry_id)
                    .await
                    .unwrap_or_else(|e| {
                        warn!(
                            entry_id = %entry_id_str,
                            error = %e,
                            "Failed to load file set for directory flag; projecting as non-directory"
                        );
                        false
                    })
            } else {
                false
            };

            projections.push(EntryProjectionDto {
                id: entry_id_str,
                preview,
                has_detail,
                size_bytes: representation.size_bytes,
                captured_at,
                content_type: entry.content_category.as_db_str().to_string(),
                thumbnail_url,
                is_encrypted: false,
                is_favorited: entry.is_favorited,
                updated_at: captured_at,
                active_time,
                file_transfer_status,
                file_transfer_reason,
                file_transfer_ids,
                content_tags,
                link_urls,
                link_domains: None,
                file_sizes,
                image_width,
                image_height,
                is_directory,
                payload_state,
            });
        }

        debug!(
            limit,
            offset,
            projections = projections.len(),
            "Listed clipboard entry projections"
        );

        Ok(projections)
    }
}

/// Map a paste-rep `PayloadAvailability` to the compact UI signal
/// (only `Lost` is surfaced; everything else renders normally).
///
/// 抽出为独立函数, 让"`Lost` 必须冒泡, 其它状态保持透明"这条业务契约能被
/// 单独单测; 同时 `execute()` 的两条分支 (paste == preview / 单独 fetch)
/// 都复用这同一段映射, 防止两边漂移。
fn paste_rep_state_to_payload_state(state: Option<&PayloadAvailability>) -> Option<String> {
    match state {
        Some(PayloadAvailability::Lost) => Some("Lost".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use uc_core::clipboard::{
        ClipboardEntry, ClipboardRepositoryError, ClipboardSelection, ClipboardSelectionDecision,
        ContentHash, EntryFileSet, EntryFileSetError, EntryFileSetLine, EntryFileSetLineKind,
        FileSetMemberKind, FileSetMemberLocation, HashAlgorithm, MimeType, SelectionPolicyVersion,
        ThumbnailMetadata,
    };
    use uc_core::ids::{BlobId, EntryId, EventId, FormatId, RepresentationId};
    use uc_core::ports::file_transfer::{EntryTransferSummary, FileTransferProjectionError};

    fn uri_list_rep(body: &str) -> PersistedClipboardRepresentation {
        PersistedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            body.len() as i64,
            Some(body.as_bytes().to_vec()),
            None,
        )
    }

    /// An image *file*'s preview_rep: image MIME, payload in blob (no inline text).
    fn image_blob_rep(size: i64) -> PersistedClipboardRepresentation {
        PersistedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("image"),
            Some(MimeType("image/png".to_string())),
            size,
            None,
            None,
        )
    }

    fn plain_text_rep(body: &str) -> PersistedClipboardRepresentation {
        PersistedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("text"),
            Some(MimeType("text/plain".to_string())),
            body.len() as i64,
            Some(body.as_bytes().to_vec()),
            None,
        )
    }

    mockall::mock! {
        EntryRepo {}

        #[async_trait]
        impl ListClipboardEntriesPort for EntryRepo {
            async fn list_entries(
                &self,
                limit: usize,
                offset: usize,
            ) -> Result<Vec<ClipboardEntry>, ClipboardRepositoryError>;
        }
    }

    mockall::mock! {
        SelectionRepo {}

        #[async_trait]
        impl ClipboardSelectionRepositoryPort for SelectionRepo {
            async fn get_selection(
                &self,
                entry_id: &EntryId,
            ) -> anyhow::Result<Option<ClipboardSelectionDecision>>;

            async fn delete_selection(&self, entry_id: &EntryId) -> anyhow::Result<()>;
        }
    }

    mockall::mock! {
        RepresentationRepo {}

        #[async_trait]
        impl GetRepresentationPort for RepresentationRepo {
            async fn get_representation(
                &self,
                event_id: &EventId,
                representation_id: &RepresentationId,
            ) -> Result<Option<PersistedClipboardRepresentation>, ClipboardRepositoryError>;
        }
    }

    mockall::mock! {
        ThumbnailRepo {}

        #[async_trait]
        impl ThumbnailRepositoryPort for ThumbnailRepo {
            async fn get_by_representation_id(
                &self,
                representation_id: &RepresentationId,
            ) -> anyhow::Result<Option<ThumbnailMetadata>>;

            async fn insert_thumbnail(&self, metadata: &ThumbnailMetadata) -> anyhow::Result<()>;
        }
    }

    mockall::mock! {
        TransferRepo {}

        #[async_trait]
        impl GetEntryTransferSummaryPort for TransferRepo {
            async fn get_entry_transfer_summary(
                &self,
                entry_id: &str,
            ) -> Result<Option<EntryTransferSummary>, FileTransferProjectionError>;
        }
    }

    mockall::mock! {
        FileSetRepo {}

        #[async_trait]
        impl EntryFileSetRepositoryPort for FileSetRepo {
            async fn save(
                &self,
                entry_id: &EntryId,
                file_set: &EntryFileSet,
            ) -> Result<(), EntryFileSetError>;

            async fn load(
                &self,
                entry_id: &EntryId,
            ) -> Result<Option<EntryFileSet>, EntryFileSetError>;
        }
    }

    /// A `FileSetRepo` that never reports a directory manifest (returns `None`
    /// for every `load`). Used by tests whose subject is not the directory flag.
    fn file_set_repo_none() -> MockFileSetRepo {
        let mut repo = MockFileSetRepo::new();
        repo.expect_load().returning(|_| Ok(None));
        repo
    }

    #[test]
    fn resolve_preview_image_file_falls_back_to_paste_rep_uri_list() {
        // Copied image file: preview_rep is the image (no uri-list), paste_rep is
        // the uri-list. The preview must expose the uri-list so the client parses
        // the real file name and renders a thumbnail, not a plain file card.
        let preview_rep = image_blob_rep(12345);
        let paste_rep = uri_list_rep("file:///home/u/photo.png");
        let preview = resolve_preview_text(
            ClipboardEntryContentCategory::File,
            &preview_rep,
            true,
            &paste_rep,
            None,
        );
        assert_eq!(preview, "file:///home/u/photo.png");
    }

    #[test]
    fn resolve_preview_plain_file_uses_preview_rep_uri_list() {
        // Regular file copy: preview_rep == paste_rep == uri-list. Unchanged.
        let rep = uri_list_rep("file:///home/u/report.pdf");
        let preview =
            resolve_preview_text(ClipboardEntryContentCategory::File, &rep, false, &rep, None);
        assert_eq!(preview, "file:///home/u/report.pdf");
    }

    #[test]
    fn resolve_preview_image_file_without_uri_list_shows_dimensions() {
        // paste_rep unreadable (falls back to the image preview_rep): no uri-list
        // is available, so the placeholder shows the image's pixel dimensions.
        let image_rep = image_blob_rep(2048);
        let preview = resolve_preview_text(
            ClipboardEntryContentCategory::File,
            &image_rep,
            true,
            &image_rep,
            Some((1920, 1080)),
        );
        assert_eq!(preview, "Image (1920×1080)");
    }

    #[test]
    fn resolve_preview_pure_image_entry_shows_dimensions() {
        let image_rep = image_blob_rep(999);
        let preview = resolve_preview_text(
            ClipboardEntryContentCategory::Image,
            &image_rep,
            true,
            &image_rep,
            Some((800, 600)),
        );
        assert_eq!(preview, "Image (800×600)");
    }

    #[test]
    fn resolve_preview_text_entry_uses_inline_text() {
        let rep = plain_text_rep("hello world");
        let preview =
            resolve_preview_text(ClipboardEntryContentCategory::Text, &rep, false, &rep, None);
        assert_eq!(preview, "hello world");
    }

    #[test]
    fn uri_list_preview_only_for_uri_list_mime() {
        assert_eq!(uri_list_preview(&image_blob_rep(10)), None);
        assert_eq!(
            uri_list_preview(&uri_list_rep("file:///a.txt")).map(|preview| preview.text),
            Some("file:///a.txt".to_string())
        );
    }

    #[tokio::test]
    async fn execute_uses_same_paste_rep_uri_list_for_preview_and_file_sizes() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("photo.png");
        std::fs::write(&path, b"abcde").expect("write");
        let uri = url::Url::from_file_path(&path)
            .expect("file path url")
            .to_string();

        let entry_id = EntryId::from("entry-1");
        let event_id = EventId::from("event-1");
        let preview_rep_id = RepresentationId::from("preview-image");
        let paste_rep_id = RepresentationId::from("paste-files");
        let preview_rep = PersistedClipboardRepresentation::new(
            preview_rep_id.clone(),
            FormatId::from("image"),
            Some(MimeType("image/png".to_string())),
            12345,
            None,
            Some(BlobId::from("blob-image")),
        );
        let paste_rep = PersistedClipboardRepresentation::new(
            paste_rep_id.clone(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            uri.len() as i64,
            Some(format!("# comment\n{uri}\n").into_bytes()),
            None,
        );
        let entry = ClipboardEntry::new(entry_id.clone(), event_id.clone(), 100, 12345)
            .with_content_category(ClipboardEntryContentCategory::File);
        let selection = ClipboardSelectionDecision::new(
            entry_id.clone(),
            ClipboardSelection {
                primary_rep_id: paste_rep_id.clone(),
                secondary_rep_ids: vec![preview_rep_id.clone()],
                preview_rep_id: preview_rep_id.clone(),
                paste_rep_id: paste_rep_id.clone(),
                policy_version: SelectionPolicyVersion::V1,
            },
        );

        let mut entry_repo = MockEntryRepo::new();
        entry_repo
            .expect_list_entries()
            .with(mockall::predicate::eq(10), mockall::predicate::eq(0))
            .times(1)
            .return_once(move |_, _| Ok(vec![entry]));

        let mut selection_repo = MockSelectionRepo::new();
        selection_repo
            .expect_get_selection()
            .with(mockall::predicate::eq(entry_id.clone()))
            .times(1)
            .return_once(move |_| Ok(Some(selection)));

        let mut representation_repo = MockRepresentationRepo::new();
        let expected_preview_event_id = event_id.clone();
        let expected_preview_rep_id = preview_rep_id.clone();
        representation_repo
            .expect_get_representation()
            .withf(move |event_id, rep_id| {
                event_id == &expected_preview_event_id && rep_id == &expected_preview_rep_id
            })
            .times(1)
            .return_once(move |_, _| Ok(Some(preview_rep)));
        let expected_paste_event_id = event_id.clone();
        let expected_paste_rep_id = paste_rep_id.clone();
        representation_repo
            .expect_get_representation()
            .withf(move |event_id, rep_id| {
                event_id == &expected_paste_event_id && rep_id == &expected_paste_rep_id
            })
            .times(1)
            .return_once(move |_, _| Ok(Some(paste_rep)));

        let mut thumbnail_repo = MockThumbnailRepo::new();
        thumbnail_repo
            .expect_get_by_representation_id()
            .with(mockall::predicate::eq(preview_rep_id))
            .times(1)
            .return_once(|_| Ok(None));

        let mut transfer_repo = MockTransferRepo::new();
        transfer_repo
            .expect_get_entry_transfer_summary()
            .with(mockall::predicate::eq("entry-1"))
            .times(1)
            .return_once(|_| Ok(None));

        let usecase = ListClipboardEntryProjectionsUseCase::new(
            Arc::new(entry_repo),
            Arc::new(selection_repo),
            Arc::new(representation_repo),
            Arc::new(thumbnail_repo),
            Arc::new(transfer_repo),
            Arc::new(file_set_repo_none()),
        );

        let projections = usecase.execute(10, 0).await.expect("projection succeeds");

        assert_eq!(projections.len(), 1);
        assert_eq!(projections[0].preview, uri);
        assert_eq!(projections[0].file_sizes, Some(vec![5]));
        assert!(!projections[0].is_directory);
    }

    /// A single-line manifest whose member is expanded from a directory root,
    /// so `has_directory_structure()` is true.
    fn directory_file_set() -> EntryFileSet {
        EntryFileSet {
            lines: vec![EntryFileSetLine {
                line_index: 1,
                original_text: "file:///root/child.txt".to_string(),
                member_location: Some(FileSetMemberLocation {
                    root_index: 0,
                    root_name: "root".to_string(),
                    relative_path: "child.txt".to_string(),
                    kind: FileSetMemberKind::File,
                }),
                kind: EntryFileSetLineKind::File {
                    content_hash: ContentHash {
                        alg: HashAlgorithm::Blake3V1,
                        bytes: [7u8; 32],
                    },
                    blob_id: None,
                    size_bytes: Some(10),
                },
            }],
        }
    }

    /// Project a single entry whose `preview_rep == paste_rep`, backed by
    /// `file_set_repo` for the directory-flag lookup. Keeps the directory tests
    /// focused on `is_directory` without repeating the full mock wiring.
    async fn project_single_entry(
        category: ClipboardEntryContentCategory,
        mime: &str,
        body: &str,
        file_set_repo: MockFileSetRepo,
    ) -> Vec<EntryProjectionDto> {
        let entry_id = EntryId::from("entry-1");
        let event_id = EventId::from("event-1");
        let rep_id = RepresentationId::from("rep-1");
        let rep = PersistedClipboardRepresentation::new(
            rep_id.clone(),
            FormatId::from("f"),
            Some(MimeType(mime.to_string())),
            body.len() as i64,
            Some(body.as_bytes().to_vec()),
            None,
        );
        let entry = ClipboardEntry::new(entry_id.clone(), event_id.clone(), 100, rep.size_bytes)
            .with_content_category(category);
        let selection = ClipboardSelectionDecision::new(
            entry_id.clone(),
            ClipboardSelection {
                primary_rep_id: rep_id.clone(),
                secondary_rep_ids: vec![],
                preview_rep_id: rep_id.clone(),
                paste_rep_id: rep_id.clone(),
                policy_version: SelectionPolicyVersion::V1,
            },
        );

        let mut entry_repo = MockEntryRepo::new();
        entry_repo
            .expect_list_entries()
            .times(1)
            .return_once(move |_, _| Ok(vec![entry]));

        let mut selection_repo = MockSelectionRepo::new();
        selection_repo
            .expect_get_selection()
            .times(1)
            .return_once(move |_| Ok(Some(selection)));

        let mut representation_repo = MockRepresentationRepo::new();
        representation_repo
            .expect_get_representation()
            .times(1)
            .return_once(move |_, _| Ok(Some(rep)));

        // Non-image reps never touch the thumbnail repo.
        let thumbnail_repo = MockThumbnailRepo::new();

        let mut transfer_repo = MockTransferRepo::new();
        transfer_repo
            .expect_get_entry_transfer_summary()
            .times(1)
            .return_once(|_| Ok(None));

        let usecase = ListClipboardEntryProjectionsUseCase::new(
            Arc::new(entry_repo),
            Arc::new(selection_repo),
            Arc::new(representation_repo),
            Arc::new(thumbnail_repo),
            Arc::new(transfer_repo),
            Arc::new(file_set_repo),
        );

        usecase.execute(10, 0).await.expect("projection succeeds")
    }

    #[tokio::test]
    async fn execute_marks_file_entry_with_directory_manifest() {
        let mut file_set_repo = MockFileSetRepo::new();
        file_set_repo
            .expect_load()
            .times(1)
            .return_once(|_| Ok(Some(directory_file_set())));

        let projections = project_single_entry(
            ClipboardEntryContentCategory::File,
            "text/uri-list",
            "file:///root/child.txt",
            file_set_repo,
        )
        .await;

        assert_eq!(projections.len(), 1);
        assert!(projections[0].is_directory);
    }

    #[tokio::test]
    async fn execute_flat_file_entry_is_not_directory() {
        // A file manifest without directory structure projects as non-directory,
        // preserving the single-file / flat-multi-file percentage path.
        let projections = project_single_entry(
            ClipboardEntryContentCategory::File,
            "text/uri-list",
            "file:///home/u/report.pdf",
            file_set_repo_none(),
        )
        .await;

        assert_eq!(projections.len(), 1);
        assert!(!projections[0].is_directory);
    }

    #[tokio::test]
    async fn execute_non_file_entry_skips_file_set_lookup() {
        // Text/image/etc. can never carry a directory manifest, so the hot list
        // path must not pay for the extra repo read.
        let mut file_set_repo = MockFileSetRepo::new();
        file_set_repo.expect_load().never();

        let projections = project_single_entry(
            ClipboardEntryContentCategory::Text,
            "text/plain",
            "hello world",
            file_set_repo,
        )
        .await;

        assert_eq!(projections.len(), 1);
        assert!(!projections[0].is_directory);
    }

    #[tokio::test]
    async fn execute_directory_lookup_error_degrades_to_non_directory() {
        // A manifest load failure must not fail the whole list; the entry falls
        // back to non-directory (percentage path) rather than erroring out.
        let mut file_set_repo = MockFileSetRepo::new();
        file_set_repo
            .expect_load()
            .times(1)
            .return_once(|_| Err(EntryFileSetError::Storage("boom".to_string())));

        let projections = project_single_entry(
            ClipboardEntryContentCategory::File,
            "text/uri-list",
            "file:///root/child.txt",
            file_set_repo,
        )
        .await;

        assert_eq!(projections.len(), 1);
        assert!(!projections[0].is_directory);
    }

    #[test]
    fn image_placeholder_prefers_dimensions() {
        assert_eq!(
            image_placeholder(Some((1920, 1080)), 5000),
            "Image (1920×1080)"
        );
    }

    #[test]
    fn image_placeholder_falls_back_to_bytes_without_dimensions() {
        assert_eq!(image_placeholder(None, 5000), "Image (5000 bytes)");
        // Zero / negative dimensions are treated as unknown.
        assert_eq!(
            image_placeholder(Some((0, 1080)), 5000),
            "Image (5000 bytes)"
        );
    }

    #[test]
    fn detect_link_urls_filters_file_uris_from_uri_list() {
        let body = "https://example.com/a\nfile:///tmp/x\nhttps://example.com/b\n";
        let urls = detect_link_urls("text/uri-list", Some(body.as_bytes()));
        assert_eq!(
            urls,
            Some(vec![
                "https://example.com/a".to_string(),
                "https://example.com/b".to_string()
            ])
        );
    }

    #[test]
    fn detect_link_urls_returns_none_when_uri_list_only_contains_files() {
        let body = "file:///tmp/a\nfile:///tmp/b\n";
        let urls = detect_link_urls("text/uri-list", Some(body.as_bytes()));
        assert_eq!(urls, None);
    }

    #[test]
    fn detect_link_urls_keeps_only_web_urls_matching_search_contract() {
        // After unifying on the uc-core contract, uri-list detection keeps only
        // web (http/https) URLs — file:// and other schemes (ftp, mailto, …) are
        // dropped — so the list projection's `linkUrls` and the search `link`
        // tag agree on what counts as a link (§4.5 parity).
        let body = "file:///home/u/a.txt\nftp://host/x\nhttps://example.com\n";
        let urls = detect_link_urls("text/uri-list", Some(body.as_bytes()));
        assert_eq!(urls, Some(vec!["https://example.com".to_string()]));
    }

    #[test]
    fn detect_link_urls_recognises_single_url_in_plain_text() {
        let urls = detect_link_urls("text/plain", Some(b"https://example.com/page"));
        assert_eq!(urls, Some(vec!["https://example.com/page".to_string()]));
    }

    #[test]
    fn detect_link_urls_returns_none_for_non_url_plain_text() {
        let urls = detect_link_urls("text/plain", Some(b"just some words"));
        assert_eq!(urls, None);
    }

    #[test]
    fn detect_link_urls_returns_none_for_unsupported_mime() {
        let urls = detect_link_urls("image/png", Some(b"\x89PNG"));
        assert_eq!(urls, None);
    }

    #[test]
    fn detect_link_urls_returns_none_when_inline_data_missing() {
        let urls = detect_link_urls("text/plain", None);
        assert_eq!(urls, None);
    }

    #[test]
    fn detect_link_urls_handles_invalid_utf8_gracefully() {
        // `from_utf8` 失败 ⇒ 函数返回 None, 不 panic
        let urls = detect_link_urls("text/plain", Some(&[0xff, 0xfe, 0xfd]));
        assert_eq!(urls, None);
    }

    #[test]
    fn content_tags_for_projection_tags_plain_text_code() {
        let tags = content_tags_for_projection(
            uc_core::clipboard::ClipboardEntryContentCategory::Text,
            "text/plain",
            Some(b"function greet(name) {\n  return `hello ${name}`;\n}"),
            false,
        );
        assert!(tags.contains(&"code".to_string()));
    }

    #[test]
    fn content_tags_for_projection_reuses_link_contract() {
        let tags = content_tags_for_projection(
            uc_core::clipboard::ClipboardEntryContentCategory::Text,
            "text/plain",
            Some(b"https://example.com"),
            false,
        );
        assert_eq!(tags, vec!["link".to_string()]);
    }

    #[test]
    fn compute_file_sizes_returns_neg_one_for_missing_file() {
        let body = "file:///definitely/not/here/abc.txt";
        let sizes = compute_file_sizes(body.as_bytes());
        assert_eq!(sizes, vec![-1]);
    }

    #[test]
    fn compute_file_sizes_returns_neg_one_for_non_file_uri() {
        let body = "https://example.com/x";
        let sizes = compute_file_sizes(body.as_bytes());
        assert_eq!(sizes, vec![-1]);
    }

    #[test]
    fn compute_file_sizes_reports_size_for_existing_file() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("hello.txt");
        std::fs::write(&path, b"abcde").expect("write");
        let uri = format!("file://{}", path.display());

        let sizes = compute_file_sizes(uri.as_bytes());
        assert_eq!(sizes, vec![5]);
    }

    #[test]
    fn compute_file_sizes_returns_empty_for_invalid_utf8() {
        let sizes = compute_file_sizes(&[0xff, 0xfe]);
        assert!(sizes.is_empty());
    }

    #[test]
    fn paste_rep_state_to_payload_state_only_surfaces_lost() {
        assert_eq!(
            paste_rep_state_to_payload_state(Some(&PayloadAvailability::Lost)),
            Some("Lost".to_string())
        );
        assert_eq!(
            paste_rep_state_to_payload_state(Some(&PayloadAvailability::Inline)),
            None
        );
        assert_eq!(
            paste_rep_state_to_payload_state(Some(&PayloadAvailability::Staged)),
            None
        );
        assert_eq!(
            paste_rep_state_to_payload_state(Some(&PayloadAvailability::BlobReady)),
            None
        );
        assert_eq!(
            paste_rep_state_to_payload_state(Some(&PayloadAvailability::Processing)),
            None
        );
        assert_eq!(
            paste_rep_state_to_payload_state(Some(&PayloadAvailability::Failed {
                last_error: "x".into(),
            })),
            None
        );
        // None 路径: paste_rep fetch 失败时映射为透明
        assert_eq!(paste_rep_state_to_payload_state(None), None);
    }
}
