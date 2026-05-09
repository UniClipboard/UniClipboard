//! Use case for listing clipboard entry projections with cross-repo aggregation.

use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, warn};
use uc_core::clipboard::link_utils::{is_all_urls, is_single_url, parse_uri_list};
use uc_core::clipboard::PayloadAvailability;
use uc_core::network::protocol::MIME_IMAGE_PREFIX;
use uc_core::ports::{
    ClipboardEntryRepositoryPort, ClipboardRepresentationRepositoryPort,
    ClipboardSelectionRepositoryPort, FileTransferRepositoryPort, ThumbnailRepositoryPort,
};

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
    pub(crate) link_urls: Option<Vec<String>>,
    pub(crate) link_domains: Option<Vec<String>>,
    pub(crate) file_sizes: Option<Vec<i64>>,
    pub(crate) image_width: Option<i32>,
    pub(crate) image_height: Option<i32>,
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
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
    file_transfer_repo: Arc<dyn FileTransferRepositoryPort>,
    max_limit: usize,
}

fn detect_link_urls(content_type: &str, inline_data: Option<&[u8]>) -> Option<Vec<String>> {
    let full_text = inline_data.and_then(|d| std::str::from_utf8(d).ok())?;
    let ct = content_type.to_ascii_lowercase();

    if ct.starts_with("text/uri-list") {
        let urls: Vec<String> = parse_uri_list(full_text)
            .into_iter()
            .filter(|u| !u.starts_with("file://"))
            .collect();
        if urls.is_empty() {
            None
        } else {
            Some(urls)
        }
    } else if ct.starts_with("text/plain") {
        if is_all_urls(full_text) {
            let urls: Vec<String> = full_text
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect();
            Some(urls)
        } else if is_single_url(full_text) {
            Some(vec![full_text.trim().to_string()])
        } else {
            None
        }
    } else {
        None
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

impl ListClipboardEntryProjectionsUseCase {
    pub(crate) fn new(
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
        thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
        file_transfer_repo: Arc<dyn FileTransferRepositoryPort>,
    ) -> Self {
        Self {
            entry_repo,
            selection_repo,
            representation_repo,
            thumbnail_repo,
            file_transfer_repo,
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
                .map(|mt| {
                    mt.as_str()
                        .to_ascii_lowercase()
                        .starts_with(MIME_IMAGE_PREFIX)
                })
                .unwrap_or(false);

            let preview = if let Some(data) = representation.inline_data.as_ref() {
                String::from_utf8_lossy(data).trim().to_string()
            } else if is_image {
                format!("Image ({} bytes)", representation.size_bytes)
            } else {
                entry
                    .title
                    .as_ref()
                    .map(|title| title.trim().to_string())
                    .filter(|title| !title.is_empty())
                    .unwrap_or_else(|| {
                        "Text content (full payload in background processing)".to_string()
                    })
            };

            let content_type = representation
                .mime_type
                .as_ref()
                .map(|mt| mt.as_str().to_string())
                .unwrap_or_else(|| "unknown".to_string());

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

            let is_uri_list = content_type
                .to_ascii_lowercase()
                .starts_with("text/uri-list");
            let link_urls = detect_link_urls(&content_type, representation.inline_data.as_deref());

            let file_sizes = if is_uri_list {
                representation
                    .inline_data
                    .as_deref()
                    .map(compute_file_sizes)
            } else {
                None
            };

            let has_detail = representation.blob_id.is_some()
                || matches!(
                    representation.payload_state(),
                    PayloadAvailability::Staged | PayloadAvailability::Processing
                );

            // paste_rep 是用户点"粘贴"时实际写入系统剪贴板的 representation。
            // 当它降级为 Lost 时, restore 会回 410 + "内容已不可用"; 列表灰显
            // 这条 entry 让用户在点之前就能知道。如果 paste_rep == preview_rep,
            // 复用刚拿到的 representation; 否则单独查一次 (paste_rep 通常和
            // preview_rep 不同—— 比如复制文本时同时有 plain + RTF)。
            let paste_rep_state = if selection.selection.paste_rep_id
                == selection.selection.preview_rep_id
            {
                Some(representation.payload_state.clone())
            } else {
                match self
                    .representation_repo
                    .get_representation(&entry.event_id, &selection.selection.paste_rep_id)
                    .await
                {
                    Ok(Some(rep)) => Some(rep.payload_state),
                    Ok(None) => None,
                    Err(e) => {
                        warn!(
                            entry_id = %entry_id_str,
                            paste_rep_id = %selection.selection.paste_rep_id,
                            error = %e,
                            "Failed to fetch paste_rep state for projection; treating as healthy"
                        );
                        None
                    }
                }
            };
            let payload_state = match paste_rep_state {
                Some(PayloadAvailability::Lost) => Some("Lost".to_string()),
                _ => None,
            };

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

            projections.push(EntryProjectionDto {
                id: entry_id_str,
                preview,
                has_detail,
                size_bytes: representation.size_bytes,
                captured_at,
                content_type,
                thumbnail_url,
                is_encrypted: false,
                is_favorited: false,
                updated_at: captured_at,
                active_time,
                file_transfer_status,
                file_transfer_reason,
                file_transfer_ids,
                link_urls,
                link_domains: None,
                file_sizes,
                image_width,
                image_height,
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
