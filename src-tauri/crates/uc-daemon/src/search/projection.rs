//! `SearchProjectionBuilder` — the single daemon-side authority for building
//! `SearchPipelineInput` from live and persisted clipboard sources.
//!
//! No other daemon module is allowed to construct `SearchPipelineInput` directly.
//! All construction goes through the static methods on this struct.

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

        // Find the preview representation in the snapshot
        let preview_rep = snapshot
            .representations
            .iter()
            .find(|r| r.id == *preview_rep_id);

        // Gather representations for analysis
        let mut plain_text: Option<String> = None;
        let mut html_text: Option<String> = None;
        let mut uri_list: Vec<String> = Vec::new();
        let mut file_paths: Vec<String> = Vec::new();
        let mut file_names: Vec<String> = Vec::new();
        let mut text_preview: Option<String> = None;

        for rep in &snapshot.representations {
            let mime = rep
                .mime
                .as_ref()
                .map(|m| m.as_str().to_lowercase())
                .unwrap_or_default();

            if mime == "text/plain" || mime.starts_with("text/plain;") {
                if let Ok(text) = std::str::from_utf8(&rep.bytes) {
                    let text = text.to_string();
                    if !text.is_empty() {
                        if rep.id == *preview_rep_id {
                            text_preview = Some(text.chars().take(200).collect());
                        }
                        plain_text = Some(text);
                    }
                }
            } else if mime == "text/html" {
                if let Ok(text) = std::str::from_utf8(&rep.bytes) {
                    let text = text.to_string();
                    if !text.is_empty() {
                        html_text = Some(text);
                    }
                }
            } else if mime == "text/uri-list" || mime == "file/uri-list" {
                if let Ok(text) = std::str::from_utf8(&rep.bytes) {
                    for line in text.lines() {
                        let line = line.trim();
                        if !line.is_empty() && !line.starts_with('#') {
                            if line.starts_with("file://") {
                                // Convert file:// URI to path
                                if let Ok(url) = url::Url::parse(line) {
                                    if let Ok(path) = url.to_file_path() {
                                        let path_str = path.to_string_lossy().to_string();
                                        // Extract file name
                                        if let Some(name) = path.file_name() {
                                            file_names.push(name.to_string_lossy().to_string());
                                        }
                                        file_paths.push(path_str);
                                    }
                                }
                            } else {
                                uri_list.push(line.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Determine the mime type from the preview representation
        let mime_type = preview_rep
            .and_then(|r| r.mime.as_ref())
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());

        let file_extensions = collect_extensions(&file_paths, &file_names);
        let content_type = infer_content_type(&mime_type, &uri_list, !file_paths.is_empty());

        // If no searchable content, return None
        if plain_text.is_none()
            && html_text.is_none()
            && uri_list.is_empty()
            && file_paths.is_empty()
            && file_names.is_empty()
        {
            return None;
        }

        Some(SearchPipelineInput {
            entry_id: entry.entry_id.clone(),
            event_id: entry.event_id.clone(),
            active_time_ms: entry.active_time_ms,
            captured_at_ms: entry.created_at_ms,
            content_type,
            mime_type,
            file_extensions,
            plain_text,
            html_text,
            uri_list,
            file_paths,
            file_names,
            text_preview,
        })
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

        let mut plain_text: Option<String> = None;
        let mut html_text: Option<String> = None;
        let mut uri_list: Vec<String> = Vec::new();
        let mut file_paths: Vec<String> = Vec::new();
        let mut file_names: Vec<String> = Vec::new();
        let mut text_preview: Option<String> = None;

        for rep in reps {
            let mime = rep
                .mime_type
                .as_ref()
                .map(|m| m.as_str().to_lowercase())
                .unwrap_or_default();

            let inline_bytes: Option<&[u8]> = rep.inline_data.as_deref();

            if mime == "text/plain" || mime.starts_with("text/plain;") {
                if let Some(bytes) = inline_bytes {
                    if let Ok(text) = std::str::from_utf8(bytes) {
                        let text = text.to_string();
                        if !text.is_empty() {
                            if rep.id == *preview_rep_id {
                                text_preview = Some(text.chars().take(200).collect());
                            }
                            plain_text = Some(text);
                        }
                    }
                }
            } else if mime == "text/html" {
                if let Some(bytes) = inline_bytes {
                    if let Ok(text) = std::str::from_utf8(bytes) {
                        let text = text.to_string();
                        if !text.is_empty() {
                            html_text = Some(text);
                        }
                    }
                }
            } else if mime == "text/uri-list" || mime == "file/uri-list" {
                if let Some(bytes) = inline_bytes {
                    if let Ok(text) = std::str::from_utf8(bytes) {
                        for line in text.lines() {
                            let line = line.trim();
                            if !line.is_empty() && !line.starts_with('#') {
                                if line.starts_with("file://") {
                                    if let Ok(parsed_url) = url::Url::parse(line) {
                                        if let Ok(path) = parsed_url.to_file_path() {
                                            let path_str = path.to_string_lossy().to_string();
                                            if let Some(name) = path.file_name() {
                                                file_names.push(name.to_string_lossy().to_string());
                                            }
                                            file_paths.push(path_str);
                                        }
                                    }
                                } else {
                                    uri_list.push(line.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        // Use entry.title as text_preview fallback if we have no inline text
        if text_preview.is_none() {
            text_preview = entry.title.clone();
        }

        // Determine the mime type from the preview representation
        let mime_type = reps
            .iter()
            .find(|r| r.id == *preview_rep_id)
            .and_then(|r| r.mime_type.as_ref())
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());

        let file_extensions = collect_extensions(&file_paths, &file_names);
        let content_type = infer_content_type(&mime_type, &uri_list, !file_paths.is_empty());

        // If no searchable content, return None
        if plain_text.is_none()
            && html_text.is_none()
            && uri_list.is_empty()
            && file_paths.is_empty()
            && file_names.is_empty()
        {
            return None;
        }

        Some(SearchPipelineInput {
            entry_id: entry.entry_id.clone(),
            event_id: entry.event_id.clone(),
            active_time_ms: entry.active_time_ms,
            captured_at_ms: entry.created_at_ms,
            content_type,
            mime_type,
            file_extensions,
            plain_text,
            html_text,
            uri_list,
            file_paths,
            file_names,
            text_preview,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::clipboard::SelectionPolicyVersion;
    use uc_core::ids::{EntryId, EventId, FormatId, RepresentationId};
    use uc_core::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};

    fn make_entry(entry_id: &str, event_id: &str) -> ClipboardEntry {
        ClipboardEntry {
            entry_id: EntryId::from(entry_id),
            event_id: EventId::from(event_id),
            created_at_ms: 1_000_000,
            active_time_ms: 500,
            title: Some("test title".to_string()),
            total_size: 100,
        }
    }

    fn make_text_snapshot(rep_id: RepresentationId, text: &str) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 1_000_000,
            representations: vec![ObservedClipboardRepresentation::new(
                rep_id,
                FormatId::from_str("text/plain"),
                Some("text/plain".parse().unwrap()),
                text.as_bytes().to_vec(),
            )],
        }
    }

    fn make_selection(preview_rep_id: RepresentationId) -> ClipboardSelection {
        ClipboardSelection {
            primary_rep_id: preview_rep_id.clone(),
            secondary_rep_ids: vec![],
            preview_rep_id,
            paste_rep_id: RepresentationId::new(),
            policy_version: SelectionPolicyVersion::V1,
        }
    }

    fn make_persisted_text_rep(
        rep_id: RepresentationId,
        text: &str,
    ) -> PersistedClipboardRepresentation {
        use std::str::FromStr;
        PersistedClipboardRepresentation::new(
            rep_id,
            FormatId::from_str("text/plain"),
            Some(MimeType::from_str("text/plain").unwrap()),
            text.len() as i64,
            Some(text.as_bytes().to_vec()),
            None,
        )
    }

    #[test]
    fn search_projection_builder_builds_from_capture_and_persisted_sources() {
        let rep_id = RepresentationId::new();
        let entry = make_entry("entry-1", "event-1");

        // --- Test build_from_capture ---
        let snapshot = make_text_snapshot(rep_id.clone(), "hello world from clipboard");
        let selection = make_selection(rep_id.clone());

        let input = SearchProjectionBuilder::build_from_capture(&entry, &snapshot, &selection);
        assert!(
            input.is_some(),
            "build_from_capture should return Some for text content"
        );
        let input = input.unwrap();
        assert_eq!(input.entry_id, EntryId::from("entry-1"));
        assert_eq!(input.event_id, EventId::from("event-1"));
        assert_eq!(
            input.plain_text.as_deref(),
            Some("hello world from clipboard")
        );
        assert_eq!(input.content_type, ContentType::Text);
        assert_eq!(input.mime_type, "text/plain");

        // --- Test build_from_persisted ---
        let rep_id2 = RepresentationId::new();
        let persisted_rep = make_persisted_text_rep(rep_id2.clone(), "hello from persisted");
        let persisted_selection =
            ClipboardSelectionDecision::new(EntryId::from("entry-1"), make_selection(rep_id2));

        let input2 = SearchProjectionBuilder::build_from_persisted(
            &entry,
            &persisted_selection,
            &[persisted_rep],
        );
        assert!(
            input2.is_some(),
            "build_from_persisted should return Some for text content"
        );
        let input2 = input2.unwrap();
        assert_eq!(input2.entry_id, EntryId::from("entry-1"));
        assert_eq!(input2.plain_text.as_deref(), Some("hello from persisted"));
        assert_eq!(input2.content_type, ContentType::Text);

        // --- Test None for empty content ---
        let empty_snapshot = SystemClipboardSnapshot {
            ts_ms: 1_000_000,
            representations: vec![],
        };
        let empty_rep_id = RepresentationId::new();
        let empty_selection = make_selection(empty_rep_id);
        let none_result =
            SearchProjectionBuilder::build_from_capture(&entry, &empty_snapshot, &empty_selection);
        assert!(
            none_result.is_none(),
            "build_from_capture should return None for empty snapshot"
        );
    }

    #[test]
    fn infer_content_type_text() {
        assert_eq!(
            infer_content_type("text/plain", &[], false),
            ContentType::Text
        );
    }

    #[test]
    fn infer_content_type_html() {
        assert_eq!(
            infer_content_type("text/html", &[], false),
            ContentType::Html
        );
    }

    #[test]
    fn infer_content_type_image() {
        assert_eq!(
            infer_content_type("image/png", &[], false),
            ContentType::Image
        );
    }

    #[test]
    fn infer_content_type_link_from_uri_list() {
        assert_eq!(
            infer_content_type("text/uri-list", &["https://example.com".to_string()], false,),
            ContentType::Link
        );
    }

    #[test]
    fn infer_content_type_file_from_uri_list() {
        // Simulates the old path where file:// was not pre-extracted
        assert_eq!(
            infer_content_type(
                "text/uri-list",
                &["file:///tmp/test.txt".to_string()],
                false,
            ),
            ContentType::File
        );
    }

    #[test]
    fn infer_content_type_file_via_has_file_paths() {
        // The real code path: file:// URIs are pre-extracted into file_paths,
        // so uri_list is empty but has_file_paths is true.
        assert_eq!(
            infer_content_type("text/uri-list", &[], true),
            ContentType::File
        );
    }
}
