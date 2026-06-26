//! `SearchProjectionBuilder` — the application-side authority for building
//! `SearchPipelineInput` from live and persisted clipboard sources.
//!
//! daemon 等外部入口不直接拼装搜索 pipeline 输入,统一从 application 调用。

use uc_core::clipboard::{
    ClipboardEntry, ClipboardSelection, ClipboardSelectionDecision,
    PersistedClipboardRepresentation, SystemClipboardSnapshot,
};
use uc_core::search::document::ContentType;
use uc_infra::search::text_extractor::SearchPipelineInput;

/// Infer the `ContentType` from a primary MIME type string.
///
/// Rules:
/// - `text/plain` and related plain text => `Text`
/// - `text/html` => `Html`
/// - non-file URL content (http/https scheme) => `Link`
/// - `text/uri-list` containing `file://` paths => `File`
/// - `image/*` => `Image`
/// - anything else => `Other`
fn infer_content_type(mime: &str, uri_list: &[String], has_file_paths: bool) -> ContentType {
    let mime_lower = mime.to_lowercase();
    if mime_lower.starts_with("image/") {
        return ContentType::Image;
    }
    if mime_lower == "text/html" {
        return ContentType::Html;
    }
    if mime_lower == "text/plain" || mime_lower.starts_with("text/plain;") {
        return ContentType::Text;
    }
    // URI list: distinguish file paths from web URLs.
    // Note: callers pre-extract file:// URIs into file_paths (so uri_list only has
    // http/https URLs). has_file_paths signals that at least one file:// URI was found.
    if mime_lower == "text/uri-list" || mime_lower == "file/uri-list" {
        if has_file_paths || uri_list.iter().any(|u| u.trim().starts_with("file://")) {
            return ContentType::File;
        }
        // Only web URLs remain => Link
        return ContentType::Link;
    }
    // Non-file URL — classify by content
    for uri in uri_list {
        let trimmed = uri.trim();
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            return ContentType::Link;
        }
        if trimmed.starts_with("file://") {
            return ContentType::File;
        }
    }
    ContentType::Other
}

/// Collect lowercased unique file extensions from a list of file paths.
fn collect_extensions(file_paths: &[String], file_names: &[String]) -> Vec<String> {
    let mut exts: Vec<String> = Vec::new();
    let all_names: Vec<&str> = file_paths
        .iter()
        .chain(file_names.iter())
        .map(|s| {
            // For file paths, take just the file name component
            if s.contains('/') || s.contains('\\') {
                let normalized = s.replace('\\', "/");
                // SAFETY: split by '/' and take last segment
                normalized.rfind('/').map(|pos| &s[pos + 1..]).unwrap_or(s)
            } else {
                s.as_str()
            }
        })
        .collect();

    for name in all_names {
        if let Some(dot_pos) = name.rfind('.') {
            if dot_pos > 0 {
                let ext = name[dot_pos + 1..].to_lowercase();
                if !ext.is_empty() && !exts.contains(&ext) {
                    exts.push(ext);
                }
            }
        }
    }
    exts.sort();
    exts.dedup();
    exts
}

/// Searchable fields gathered while scanning a clipboard entry's
/// representations. The MIME-dispatch rules live here so the live-capture and
/// persisted projection paths share one implementation instead of duplicating
/// the per-representation extraction loop.
#[derive(Default)]
struct SearchableContent {
    plain_text: Option<String>,
    html_text: Option<String>,
    uri_list: Vec<String>,
    file_paths: Vec<String>,
    file_names: Vec<String>,
    text_preview: Option<String>,
}

impl SearchableContent {
    /// Fold one representation's inline bytes into the accumulators by MIME
    /// type. `is_preview` marks the preview representation, whose plain text
    /// seeds `text_preview`. Non-UTF-8 or empty payloads are ignored.
    fn ingest(&mut self, mime: &str, inline_bytes: Option<&[u8]>, is_preview: bool) {
        let mime = mime.to_lowercase();
        if mime == "text/plain" || mime.starts_with("text/plain;") {
            if let Ok(text) = std::str::from_utf8(inline_bytes.unwrap_or(&[])) {
                if !text.is_empty() {
                    if is_preview {
                        self.text_preview = Some(text.chars().take(200).collect());
                    }
                    self.plain_text = Some(text.to_string());
                }
            }
        } else if mime == "text/html" {
            if let Ok(text) = std::str::from_utf8(inline_bytes.unwrap_or(&[])) {
                if !text.is_empty() {
                    self.html_text = Some(text.to_string());
                }
            }
        } else if mime == "text/uri-list" || mime == "file/uri-list" {
            if let Ok(text) = std::str::from_utf8(inline_bytes.unwrap_or(&[])) {
                for line in text.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if line.starts_with("file://") {
                        // Convert file:// URI to a path, extracting the file name.
                        if let Ok(url) = url::Url::parse(line) {
                            if let Ok(path) = url.to_file_path() {
                                if let Some(name) = path.file_name() {
                                    self.file_names.push(name.to_string_lossy().to_string());
                                }
                                self.file_paths.push(path.to_string_lossy().to_string());
                            }
                        }
                    } else {
                        self.uri_list.push(line.to_string());
                    }
                }
            }
        }
    }

    /// True when nothing searchable was gathered.
    fn is_empty(&self) -> bool {
        self.plain_text.is_none()
            && self.html_text.is_none()
            && self.uri_list.is_empty()
            && self.file_paths.is_empty()
            && self.file_names.is_empty()
    }

    /// Assemble the final `SearchPipelineInput`, or `None` if nothing
    /// searchable was gathered. `mime_type` is resolved by the caller from its
    /// own source (live snapshot vs persisted reps).
    fn into_pipeline_input(
        self,
        entry: &ClipboardEntry,
        mime_type: String,
    ) -> Option<SearchPipelineInput> {
        if self.is_empty() {
            return None;
        }
        let file_extensions = collect_extensions(&self.file_paths, &self.file_names);
        let content_type =
            infer_content_type(&mime_type, &self.uri_list, !self.file_paths.is_empty());
        Some(SearchPipelineInput {
            entry_id: entry.entry_id.clone(),
            event_id: entry.event_id.clone(),
            active_time_ms: entry.active_time_ms,
            captured_at_ms: entry.created_at_ms,
            content_type,
            mime_type,
            file_extensions,
            plain_text: self.plain_text,
            html_text: self.html_text,
            uri_list: self.uri_list,
            file_paths: self.file_paths,
            file_names: self.file_names,
            text_preview: self.text_preview,
        })
    }
}

/// The single daemon-side authority for building `SearchPipelineInput`.
///
/// Both methods are static associated functions — this struct has no instance state.
pub struct SearchProjectionBuilder;

impl SearchProjectionBuilder {
    /// Build a `SearchPipelineInput` from a live clipboard capture event.
    ///
    /// Called immediately after a successful `CaptureClipboardUseCase` so the
    /// live `SystemClipboardSnapshot` is still available.
    ///
    /// Returns `None` when the snapshot contains no searchable content (no plain
    /// text, HTML, URL, file path, or file name segments).
    pub fn build_from_capture(
        entry: &ClipboardEntry,
        snapshot: &SystemClipboardSnapshot,
        selection: &ClipboardSelection,
    ) -> Option<SearchPipelineInput> {
        let preview_rep_id = &selection.preview_rep_id;

        let mut content = SearchableContent::default();
        for rep in &snapshot.representations {
            let mime = rep.mime.as_ref().map(|m| m.as_str()).unwrap_or_default();
            content.ingest(mime, rep.inline_bytes(), rep.id == *preview_rep_id);
        }

        // Determine the mime type from the preview representation.
        let mime_type = snapshot
            .representations
            .iter()
            .find(|r| r.id == *preview_rep_id)
            .and_then(|r| r.mime.as_ref())
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());

        content.into_pipeline_input(entry, mime_type)
    }

    /// Build a `SearchPipelineInput` from persisted clipboard data.
    ///
    /// Called during rebuild when only the stored representations (not the original
    /// live snapshot) are available.
    ///
    /// Returns `None` when the persisted data contains no searchable content.
    pub fn build_from_persisted(
        entry: &ClipboardEntry,
        selection: &ClipboardSelectionDecision,
        reps: &[PersistedClipboardRepresentation],
    ) -> Option<SearchPipelineInput> {
        let preview_rep_id = &selection.selection.preview_rep_id;

        let mut content = SearchableContent::default();
        for rep in reps {
            let mime = rep
                .mime_type
                .as_ref()
                .map(|m| m.as_str())
                .unwrap_or_default();
            content.ingest(mime, rep.inline_data.as_deref(), rep.id == *preview_rep_id);
        }

        // Use entry.title as text_preview fallback if we have no inline text.
        if content.text_preview.is_none() {
            content.text_preview = entry.title.clone();
        }

        // Determine the mime type from the preview representation.
        let mime_type = reps
            .iter()
            .find(|r| r.id == *preview_rep_id)
            .and_then(|r| r.mime_type.as_ref())
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());

        content.into_pipeline_input(entry, mime_type)
    }
}
