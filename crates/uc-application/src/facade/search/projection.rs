//! `SearchProjectionBuilder` — the application-side authority for building
//! `SearchPipelineInput` from live and persisted clipboard sources.
//!
//! daemon 等外部入口不直接拼装搜索 pipeline 输入,统一从 application 调用。

use uc_core::clipboard::link_utils::detect_link_urls;
use uc_core::clipboard::{
    ClipboardEntry, ClipboardSelection, ClipboardSelectionDecision, PayloadAvailability,
    PersistedClipboardRepresentation, SystemClipboardSnapshot,
};
use uc_core::search::document::ContentType;
use uc_core::search::tag::{TagId, TagRule, TaggableContent};
use uc_infra::search::text_extractor::SearchPipelineInput;

/// A [`TagRule`] that marks content carrying one or more web URLs with the
/// builtin `link` tag. The membership decision and the `linkUrls` render
/// metadata share [`detect_link_urls`], so they stay in lock-step.
struct LinkRule {
    tag_id: TagId,
}

impl LinkRule {
    fn new() -> Self {
        Self {
            tag_id: TagId::link(),
        }
    }
}

impl TagRule for LinkRule {
    fn tag_id(&self) -> &TagId {
        &self.tag_id
    }

    fn evaluate(&self, content: &TaggableContent<'_>) -> bool {
        !detect_link_urls(content.uri_list, content.plain_text).is_empty()
    }
}

/// A [`TagRule`] that marks entries carrying image content with the builtin
/// `image` tag.
///
/// Unlike `content_type` — which faithfully reflects the *paste* representation,
/// so a copied image file (uri-list paste rep) is physically a `File` — this tag
/// answers "does this entry contain an image?". It therefore surfaces both pure
/// bitmaps and image files (a copied `.png`, or a multi-file selection that
/// includes one) under the image filter, mirroring the way the `link` tag is
/// orthogonal to the physical content type.
struct ImageRule {
    tag_id: TagId,
}

impl ImageRule {
    fn new() -> Self {
        Self {
            tag_id: TagId::image(),
        }
    }
}

impl TagRule for ImageRule {
    fn tag_id(&self) -> &TagId {
        &self.tag_id
    }

    fn evaluate(&self, content: &TaggableContent<'_>) -> bool {
        content.has_image
    }
}

/// A [`TagRule`] that marks entries carrying rich text / HTML with the builtin
/// `code` tag. Plain-text snippets that look like source code also carry this
/// tag, matching the history card's best-effort code presentation.
struct CodeRule {
    tag_id: TagId,
}

impl CodeRule {
    fn new() -> Self {
        Self {
            tag_id: TagId::code(),
        }
    }
}

impl TagRule for CodeRule {
    fn tag_id(&self) -> &TagId {
        &self.tag_id
    }

    fn evaluate(&self, content: &TaggableContent<'_>) -> bool {
        content.content_type == ContentType::Html || looks_like_code(content.plain_text)
    }
}

fn looks_like_code(text: Option<&str>) -> bool {
    let Some(text) = text else {
        return false;
    };
    let trimmed = text.trim();
    if trimmed.len() < 12 {
        return false;
    }

    let lines: Vec<&str> = trimmed.lines().take(12).collect();
    // Keep this list free of words that appear in ordinary prose ("return",
    // "from", …): a single common word must not be enough to tag a note as code.
    let has_code_keyword = [
        "function ",
        "const ",
        "interface ",
        "import ",
        "export ",
        "def ",
        "fn ",
        "impl ",
        "struct ",
        "func ",
        "package ",
        "SELECT ",
        "INSERT INTO ",
        "UPDATE ",
        "DELETE FROM ",
        "CREATE TABLE ",
    ]
    .iter()
    .any(|keyword| trimmed.contains(keyword));
    let has_code_punctuation = trimmed.contains('{')
        || trimmed.contains('}')
        || trimmed.contains("=>")
        || trimmed.contains("->")
        || trimmed.contains("::")
        || trimmed.contains("</")
        || trimmed.contains("/>")
        || trimmed.contains("#include");
    let indented_lines = lines
        .iter()
        .filter(|line| line.starts_with("  ") || line.starts_with('\t'))
        .count();
    // `": "` is intentionally excluded — it is far more common in prose
    // ("Notes: …") than the punctuation/operators below that signal real code.
    let assignment_like = trimmed.contains(" = ")
        || trimmed.contains(" := ")
        || trimmed.contains("==")
        || trimmed.contains("!=");
    let comment_like = lines.iter().any(|line| {
        let s = line.trim_start();
        s.starts_with("//") || s.starts_with("/*") || s.starts_with("# ") || s.starts_with("-- ")
    });

    (has_code_keyword && (has_code_punctuation || assignment_like || indented_lines > 0))
        || (has_code_punctuation && indented_lines > 0)
        || (comment_like && (has_code_keyword || has_code_punctuation))
}

/// The builtin tag rules evaluated for every entry: [`LinkRule`] (web URLs),
/// [`ImageRule`] (image content), and [`CodeRule`] (HTML/source-like text).
/// User-defined rules are a later extension point.
fn builtin_rules() -> Vec<Box<dyn TagRule>> {
    vec![
        Box::new(LinkRule::new()),
        Box::new(ImageRule::new()),
        Box::new(CodeRule::new()),
    ]
}

/// Evaluate `rules` against `content`, collecting the ids of the tags that apply.
fn evaluate_tags(content: &TaggableContent<'_>, rules: &[Box<dyn TagRule>]) -> Vec<TagId> {
    rules
        .iter()
        .filter(|rule| rule.evaluate(content))
        .map(|rule| rule.tag_id().clone())
        .collect()
}

/// Marker mirrored into the `payload_state` render column: `Some("Lost")` only
/// when the paste representation is permanently lost, `None` for every healthy
/// state. Mirrors the list projection's `paste_rep_state_to_payload_state` so
/// list and search render the same "this entry can no longer be pasted" signal.
fn payload_state_marker(state: &PayloadAvailability) -> Option<String> {
    matches!(state, PayloadAvailability::Lost).then(|| "Lost".to_string())
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
    /// Full character count of the preview representation's plain text — the same
    /// source `text_preview` is truncated from. Set alongside `text_preview` so
    /// the count always matches the displayed text; `None` for entries with no
    /// inline plain text.
    char_count: Option<i64>,
    /// True when any representation is an image. An image entry is browsable and
    /// filterable even with no searchable text. This drives the derived `image`
    /// tag — NOT the content_type: a copied image file keeps `content_type =
    /// File` (faithful to its uri-list paste rep) and is surfaced under the
    /// image filter via the tag instead.
    has_image: bool,
    /// True when a `text/html` representation is present. Tracked by MIME
    /// presence (like `has_image`), not by captured bytes, so classification is
    /// stable even when the html payload is later lost — matching the domain
    /// category precedence in `uc_core::clipboard::category`.
    has_html: bool,
    /// True when a `text/plain` representation is present (MIME presence).
    has_text: bool,
}

impl SearchableContent {
    /// Fold one representation's inline bytes into the accumulators by MIME
    /// type. `is_preview` marks the preview representation, whose plain text
    /// seeds `text_preview`. Non-UTF-8 or empty payloads are ignored.
    fn ingest(&mut self, mime: &str, inline_bytes: Option<&[u8]>, is_preview: bool) {
        let mime = mime.to_lowercase();
        if mime.starts_with("image/") {
            // Only the presence of an image rep matters here (not its bytes): it
            // makes the entry browsable/filterable as an image even when no text
            // is present (a pure screenshot or bitmap).
            self.has_image = true;
        } else if mime == "text/plain" || mime.starts_with("text/plain;") {
            // Presence drives classification even if the payload is lost.
            self.has_text = true;
            if let Ok(text) = std::str::from_utf8(inline_bytes.unwrap_or(&[])) {
                if !text.is_empty() {
                    if is_preview {
                        self.text_preview = Some(text.chars().take(200).collect());
                        self.char_count = Some(text.chars().count() as i64);
                    }
                    self.plain_text = Some(text.to_string());
                }
            }
        } else if mime == "text/html" || mime.starts_with("text/html;") {
            // Presence drives classification even if the payload is lost.
            // Match parameterized variants too (e.g. `text/html; charset=utf-8`).
            self.has_html = true;
            if let Ok(text) = std::str::from_utf8(inline_bytes.unwrap_or(&[])) {
                if !text.is_empty() {
                    self.html_text = Some(text.to_string());
                }
            }
        } else if mime == "text/uri-list"
            || mime.starts_with("text/uri-list;")
            || mime == "file/uri-list"
            || mime.starts_with("file/uri-list;")
        {
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

    /// True when nothing indexable was gathered. An image-only entry is NOT
    /// empty: it has no searchable text but must still be indexed so browse and
    /// the `image` content-type filter can surface it.
    fn is_empty(&self) -> bool {
        self.plain_text.is_none()
            && self.html_text.is_none()
            && self.uri_list.is_empty()
            && self.file_paths.is_empty()
            && self.file_names.is_empty()
            && !self.has_image
    }

    /// Classify the single-valued physical `content_type` over the *entire*
    /// representation set, by precedence — never from one chosen representation.
    ///
    /// This mirrors the domain category precedence in
    /// `uc_core::clipboard::category` (`file > image > rich_text > text`).
    /// Deriving from a single "paste" representation is wrong because that rep is
    /// picked for *paste fidelity* by the selection policy, not for
    /// classification: a web-image copy carries both an `<img>` `text/html` rep
    /// (which the policy ranks highest, to paste as rich text) and the actual
    /// `image/*` bitmap — so paste-rep classification would call it `Html`, when
    /// the user copied an image. Precedence over the set gets it right:
    ///
    /// - any `file://` path        => `File` (image files / multi-file selections;
    ///   the image nature rides the derived `image` tag, not the type)
    /// - else any image rep        => `Image` (pure bitmap, screenshot, web image)
    /// - else `text/html`          => `Html` (rich text, no bitmap rep)
    /// - else plain text / web URL => `Text` (URL nature rides the `link` tag)
    /// - else                      => `Other`
    fn content_type(&self) -> ContentType {
        if !self.file_paths.is_empty() {
            ContentType::File
        } else if self.has_image {
            ContentType::Image
        } else if self.has_html {
            ContentType::Html
        } else if self.has_text || self.plain_text.is_some() || !self.uri_list.is_empty() {
            ContentType::Text
        } else {
            ContentType::Other
        }
    }

    /// Assemble the final `SearchPipelineInput`, or `None` if nothing
    /// searchable was gathered. `mime_type` is resolved by the caller from its
    /// own source (live snapshot vs persisted reps). `source_device` and
    /// `payload_state` are likewise resolved by the caller — the former requires
    /// an async port lookup, the latter differs between the live (healthy
    /// default) and rebuild (authoritative) paths.
    fn into_pipeline_input(
        self,
        entry: &ClipboardEntry,
        mime_type: String,
        source_device: Option<String>,
        payload_state: Option<String>,
    ) -> Option<SearchPipelineInput> {
        if self.is_empty() {
            return None;
        }
        let file_extensions = collect_extensions(&self.file_paths, &self.file_names);
        // content_type is classified over the whole representation set by
        // precedence (see `content_type`), not from the paste rep's MIME. The
        // "this entry contains an image" property is carried separately by the
        // derived `image` tag (see `ImageRule`), so the image filter surfaces
        // both pure bitmaps and image files without the latter being
        // misclassified or lost from the file filter.
        let content_type = self.content_type();
        let mut tags = evaluate_tags(
            &TaggableContent {
                content_type: content_type.clone(),
                uri_list: &self.uri_list,
                plain_text: self.plain_text.as_deref(),
                has_image: self.has_image,
            },
            &builtin_rules(),
        );
        // `favorited` is user-state, not a content rule, so it cannot come from
        // `builtin_rules`. Mirror the entry's persisted favorite flag into the
        // tag set: a live capture of a fresh entry carries `false`, while rebuild
        // backfills the authoritative value from the entry's stored state. This
        // is the rebuild leg of the favorited tag mirror (the toggle use case
        // owns the write-through leg).
        if entry.is_favorited {
            tags.push(TagId::favorited());
        }
        // Same detection contract as the `link` rule above, so the render column
        // and the tag never disagree on what counts as a link.
        let link_urls = detect_link_urls(&self.uri_list, self.plain_text.as_deref());
        Some(SearchPipelineInput {
            entry_id: entry.entry_id.clone(),
            event_id: entry.event_id.clone(),
            active_time_ms: entry.active_time_ms,
            captured_at_ms: entry.created_at_ms,
            content_type,
            tags,
            mime_type,
            file_extensions,
            plain_text: self.plain_text,
            html_text: self.html_text,
            uri_list: self.uri_list,
            file_paths: self.file_paths,
            file_names: self.file_names,
            text_preview: self.text_preview,
            char_count: self.char_count,
            link_urls,
            source_device,
            payload_state,
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
    /// live `SystemClipboardSnapshot` is still available. `source_device` is the
    /// originating device id resolved by the caller (`None` when unknown).
    ///
    /// `payload_state` is left at the healthy default (`None`): a freshly
    /// captured payload is always available, and the authoritative state is
    /// backfilled by rebuild if it later becomes lost.
    ///
    /// Returns `None` when the snapshot contains no searchable content (no plain
    /// text, HTML, URL, file path, or file name segments).
    pub fn build_from_capture(
        entry: &ClipboardEntry,
        snapshot: &SystemClipboardSnapshot,
        selection: &ClipboardSelection,
        source_device: Option<String>,
    ) -> Option<SearchPipelineInput> {
        let preview_rep_id = &selection.preview_rep_id;

        let mut content = SearchableContent::default();
        for rep in &snapshot.representations {
            let mime = rep.mime.as_ref().map(|m| m.as_str()).unwrap_or_default();
            content.ingest(mime, rep.inline_bytes(), rep.id == *preview_rep_id);
        }

        // Determine the mime type from the paste representation — the content's
        // primary data form. The preview representation prefers plain text, so a
        // rich-text entry (text/plain + text/html) would otherwise be misread as
        // `text` and dropped from the `html` filter.
        let mime_type = snapshot
            .representations
            .iter()
            .find(|r| r.id == selection.paste_rep_id)
            .and_then(|r| r.mime.as_ref())
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());

        content.into_pipeline_input(entry, mime_type, source_device, None)
    }

    /// Build a `SearchPipelineInput` from persisted clipboard data.
    ///
    /// Called during rebuild when only the stored representations (not the original
    /// live snapshot) are available. `source_device` is resolved by the caller
    /// from the same clipboard event as the live path, so the two stay in parity.
    ///
    /// `payload_state` is the authoritative value derived from the paste
    /// representation: `Some("Lost")` when its payload is permanently lost.
    ///
    /// Returns `None` when the persisted data contains no searchable content.
    pub fn build_from_persisted(
        entry: &ClipboardEntry,
        selection: &ClipboardSelectionDecision,
        reps: &[PersistedClipboardRepresentation],
        source_device: Option<String>,
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

        // Locate the paste representation once: it drives both the mime type
        // (content's primary data form — see `build_from_capture` for why preview
        // is wrong) and the authoritative payload_state.
        let paste_rep = reps
            .iter()
            .find(|r| r.id == selection.selection.paste_rep_id);
        let mime_type = paste_rep
            .and_then(|r| r.mime_type.as_ref())
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let payload_state = paste_rep.and_then(|r| payload_state_marker(&r.payload_state));

        content.into_pipeline_input(entry, mime_type, source_device, payload_state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::clipboard::{ObservedClipboardRepresentation, SelectionPolicyVersion};
    use uc_core::ids::{EntryId, EventId, FormatId, RepresentationId};
    use uc_core::MimeType;

    fn rep(fmt: &str, mime: &str, bytes: &[u8]) -> ObservedClipboardRepresentation {
        ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from(fmt),
            Some(MimeType(mime.to_string())),
            bytes.to_vec(),
        )
    }

    fn entry() -> ClipboardEntry {
        ClipboardEntry::new(EntryId::new(), EventId::new(), 0, None, 0)
    }

    /// Project a single-representation capture (all selection slots point at the
    /// one rep) into its `SearchPipelineInput`.
    fn project_one(fmt: &str, mime: &str, bytes: &[u8]) -> SearchPipelineInput {
        let r = rep(fmt, mime, bytes);
        let id = r.id.clone();
        let snapshot = SystemClipboardSnapshot {
            ts_ms: 1,
            representations: vec![r],
            file_content_digests: Vec::new(),
        };
        let selection = ClipboardSelection {
            primary_rep_id: id.clone(),
            secondary_rep_ids: Vec::new(),
            preview_rep_id: id.clone(),
            paste_rep_id: id,
            policy_version: SelectionPolicyVersion::V1,
        };
        SearchProjectionBuilder::build_from_capture(&entry(), &snapshot, &selection, None)
            .expect("snapshot has searchable content")
    }

    /// A rich-text copy carries `text/plain` + `text/html` and no bitmap rep, so
    /// the set-precedence classifier (file > image > html > text) lands on
    /// `Html`. The preview text still comes from the plain (preview) rep.
    #[test]
    fn rich_text_is_classified_as_html() {
        let plain = rep("text", "text/plain", b"hello world");
        let html = rep("html", "text/html", b"<p>hello world</p>");
        let plain_id = plain.id.clone();
        let html_id = html.id.clone();

        let snapshot = SystemClipboardSnapshot {
            ts_ms: 1,
            representations: vec![plain, html],
            file_content_digests: Vec::new(),
        };
        let selection = ClipboardSelection {
            primary_rep_id: html_id.clone(),
            secondary_rep_ids: Vec::new(),
            preview_rep_id: plain_id,
            paste_rep_id: html_id,
            policy_version: SelectionPolicyVersion::V1,
        };

        let input =
            SearchProjectionBuilder::build_from_capture(&entry(), &snapshot, &selection, None)
                .expect("snapshot has searchable content");

        assert_eq!(input.content_type, ContentType::Html);
        assert!(
            input.tags.contains(&TagId::code()),
            "rich text carries the code tag"
        );
        // Preview text still comes from the preview (plain) representation.
        assert_eq!(input.text_preview.as_deref(), Some("hello world"));
        // Short text: the char count equals the (untruncated) preview length.
        assert_eq!(input.char_count, Some(11));
    }

    /// `text_preview` is capped at 200 chars, but `char_count` must carry the
    /// FULL length so the UI shows the real total instead of a stuck "200". This
    /// is the regression test for "history card always shows 200 characters".
    #[test]
    fn long_text_reports_full_char_count_despite_truncated_preview() {
        let body = "x".repeat(250);
        let plain = rep("text", "text/plain", body.as_bytes());
        let plain_id = plain.id.clone();

        let snapshot = SystemClipboardSnapshot {
            ts_ms: 1,
            representations: vec![plain],
            file_content_digests: Vec::new(),
        };
        let selection = ClipboardSelection {
            primary_rep_id: plain_id.clone(),
            secondary_rep_ids: Vec::new(),
            preview_rep_id: plain_id.clone(),
            paste_rep_id: plain_id,
            policy_version: SelectionPolicyVersion::V1,
        };

        let input =
            SearchProjectionBuilder::build_from_capture(&entry(), &snapshot, &selection, None)
                .expect("snapshot has searchable content");

        // Preview is truncated to 200 chars...
        assert_eq!(input.text_preview.as_deref().map(str::len), Some(200));
        // ...but the char count reflects the full 250-character text.
        assert_eq!(input.char_count, Some(250));
    }

    /// A web-image copy (right-click → Copy Image) carries the actual `image/*`
    /// bitmap AND a `text/html` `<img>` wrapper (which the selection policy ranks
    /// as the paste rep, to paste as rich text) AND often a `text/plain` URL.
    /// Classifying from the paste rep would call it `Html` ("code"); precedence
    /// over the set — image beats html when there is no file — correctly lands on
    /// `Image` with the `image` tag. This is the regression test for "web image
    /// shows as code".
    #[test]
    fn web_image_copy_is_image_not_html() {
        let image = rep("image", "image/png", b"\x89PNG\r\n\x1a\n");
        let html = rep("html", "text/html", b"<img src=\"https://x.test/a.png\">");
        let plain = rep("text", "text/plain", b"https://x.test/a.png");
        let html_id = html.id.clone();
        let plain_id = plain.id.clone();

        let snapshot = SystemClipboardSnapshot {
            ts_ms: 1,
            representations: vec![image, html, plain],
            file_content_digests: Vec::new(),
        };
        // The selection policy ranks the rich-text (html) rep as the paste rep.
        let selection = ClipboardSelection {
            primary_rep_id: html_id.clone(),
            secondary_rep_ids: Vec::new(),
            preview_rep_id: plain_id,
            paste_rep_id: html_id,
            policy_version: SelectionPolicyVersion::V1,
        };

        let input =
            SearchProjectionBuilder::build_from_capture(&entry(), &snapshot, &selection, None)
                .expect("snapshot has searchable content");

        assert_eq!(
            input.content_type,
            ContentType::Image,
            "a copied web image is an Image, never Html, even when the paste rep is the <img> html"
        );
        assert!(input.tags.contains(&TagId::image()));
    }

    /// A pure image (image paste rep, no file rep) projects as `Image` and
    /// carries the `image` tag, so browse and the image filter surface it.
    /// Previously it gathered no searchable content and was dropped from the
    /// index entirely.
    #[test]
    fn image_only_entry_projects_as_image_with_image_tag() {
        let input = project_one("image", "image/png", b"\x89PNG\r\n\x1a\n");
        assert_eq!(input.content_type, ContentType::Image);
        assert!(
            input.tags.contains(&TagId::image()),
            "a pure bitmap carries the image tag"
        );
    }

    /// A copied image file carries both an `image/*` rep and a `text/uri-list`
    /// file path; the paste rep is the file rep. content_type is faithful to the
    /// paste rep (`File`) — a copied image file IS a file — while the derived
    /// `image` tag carries its image nature so the image filter still surfaces
    /// it and the file filter does not lose it.
    #[test]
    fn image_file_is_classified_as_file_with_image_tag() {
        let image = rep("image", "image/png", b"\x89PNG\r\n\x1a\n");
        let files = rep("files", "text/uri-list", b"file:///tmp/shot.png");
        let files_id = files.id.clone();

        let snapshot = SystemClipboardSnapshot {
            ts_ms: 1,
            representations: vec![image, files],
            file_content_digests: Vec::new(),
        };
        let selection = ClipboardSelection {
            primary_rep_id: files_id.clone(),
            secondary_rep_ids: Vec::new(),
            preview_rep_id: files_id.clone(),
            paste_rep_id: files_id,
            policy_version: SelectionPolicyVersion::V1,
        };

        let input =
            SearchProjectionBuilder::build_from_capture(&entry(), &snapshot, &selection, None)
                .expect("snapshot has searchable content");

        assert_eq!(
            input.content_type,
            ContentType::File,
            "a copied image file is physically a File (faithful to the paste rep)"
        );
        assert!(
            input.tags.contains(&TagId::image()),
            "the image nature is carried by the derived image tag"
        );
        assert_eq!(input.file_names, vec!["shot.png".to_string()]);
    }

    /// A multi-file copy that includes one image (uri-list with several
    /// file:// paths + an image rep) is a `File` with the full file list, plus
    /// the `image` tag because it contains an image. This is the regression the
    /// image-over-file priority got wrong: it would have shown a single broken
    /// image card and dropped the entry from the file filter.
    #[test]
    fn multi_file_with_one_image_is_file_with_image_tag() {
        let image = rep("image", "image/png", b"\x89PNG\r\n\x1a\n");
        let files = rep(
            "files",
            "text/uri-list",
            b"file:///tmp/notes.txt\nfile:///tmp/photo.png\n",
        );
        let files_id = files.id.clone();

        let snapshot = SystemClipboardSnapshot {
            ts_ms: 1,
            representations: vec![image, files],
            file_content_digests: Vec::new(),
        };
        let selection = ClipboardSelection {
            primary_rep_id: files_id.clone(),
            secondary_rep_ids: Vec::new(),
            preview_rep_id: files_id.clone(),
            paste_rep_id: files_id,
            policy_version: SelectionPolicyVersion::V1,
        };

        let input =
            SearchProjectionBuilder::build_from_capture(&entry(), &snapshot, &selection, None)
                .expect("snapshot has searchable content");

        assert_eq!(input.content_type, ContentType::File);
        assert!(input.tags.contains(&TagId::image()));
        assert_eq!(
            input.file_names,
            vec!["notes.txt".to_string(), "photo.png".to_string()],
            "the full multi-file list is preserved, not collapsed to one image"
        );
    }

    #[test]
    fn web_url_uri_list_is_text_with_link_tag() {
        let input = project_one("files", "text/uri-list", b"https://example.com\n");
        assert_eq!(input.content_type, ContentType::Text);
        assert_eq!(input.tags, vec![TagId::link()]);
    }

    #[test]
    fn plain_text_url_gets_link_tag() {
        let input = project_one("text", "text/plain", b"https://example.com");
        assert_eq!(input.content_type, ContentType::Text);
        assert_eq!(input.tags, vec![TagId::link()]);
    }

    #[test]
    fn plain_text_code_snippet_gets_code_tag() {
        let input = project_one(
            "text",
            "text/plain",
            b"function greet(name) {\n  return `hello ${name}`;\n}",
        );
        assert_eq!(input.content_type, ContentType::Text);
        assert!(
            input.tags.contains(&TagId::code()),
            "plain code text should be searchable through the code tag"
        );
    }

    #[test]
    fn prose_with_programming_words_has_no_code_tag() {
        // The colon + "from"/"return" once tripped the heuristic (`": "` counted
        // as assignment-like, "from"/"return" as code keywords). Ordinary notes
        // must not be tagged as code.
        let input = project_one(
            "text",
            "text/plain",
            b"Notes from today: please return the signed form after the meeting.",
        );
        assert_eq!(input.content_type, ContentType::Text);
        assert!(
            !input.tags.contains(&TagId::code()),
            "ordinary prose should not be searchable through the code tag"
        );
    }

    #[test]
    fn prose_with_url_has_no_link_tag() {
        let input = project_one("text", "text/plain", b"see https://example.com for more");
        assert_eq!(input.content_type, ContentType::Text);
        assert!(input.tags.is_empty());
    }

    #[test]
    fn file_uri_list_is_file_without_link_tag() {
        let input = project_one("files", "text/uri-list", b"file:///home/u/a.txt\n");
        assert_eq!(input.content_type, ContentType::File);
        assert!(input.tags.is_empty());
    }

    #[test]
    fn plain_text_without_url_has_no_tags() {
        let input = project_one("text", "text/plain", b"just some notes");
        assert_eq!(input.content_type, ContentType::Text);
        assert!(input.tags.is_empty());
    }

    /// A uri-list carrying both a `file://` path and a web URL populates the
    /// `file_names` and `link_urls` render columns; live capture writes the
    /// healthy `payload_state` default and passes `source_device` through. The
    /// `link_urls` column shares the `link` tag's detection contract, so a
    /// populated column implies the tag is present.
    #[test]
    fn capture_populates_render_metadata() {
        let r = rep(
            "files",
            "text/uri-list",
            b"file:///home/u/report.pdf\nhttps://example.com\n",
        );
        let id = r.id.clone();
        let snapshot = SystemClipboardSnapshot {
            ts_ms: 1,
            representations: vec![r],
            file_content_digests: Vec::new(),
        };
        let selection = ClipboardSelection {
            primary_rep_id: id.clone(),
            secondary_rep_ids: Vec::new(),
            preview_rep_id: id.clone(),
            paste_rep_id: id,
            policy_version: SelectionPolicyVersion::V1,
        };

        let input = SearchProjectionBuilder::build_from_capture(
            &entry(),
            &snapshot,
            &selection,
            Some("dev-x".to_string()),
        )
        .expect("snapshot has searchable content");

        assert_eq!(input.file_names, vec!["report.pdf".to_string()]);
        assert_eq!(input.link_urls, vec!["https://example.com".to_string()]);
        assert_eq!(input.source_device.as_deref(), Some("dev-x"));
        assert_eq!(input.payload_state, None, "live capture is healthy");
        assert!(input.tags.contains(&TagId::link()));
    }

    fn persisted(
        rep_id: &RepresentationId,
        fmt: &str,
        mime: &str,
        bytes: &[u8],
    ) -> PersistedClipboardRepresentation {
        PersistedClipboardRepresentation::new(
            rep_id.clone(),
            FormatId::from(fmt),
            Some(MimeType(mime.to_string())),
            bytes.len() as i64,
            Some(bytes.to_vec()),
            None,
        )
    }

    /// Rebuild derives the authoritative `payload_state` from the paste
    /// representation: `Some("Lost")` when its payload is permanently lost, even
    /// though the searchable content comes from a healthy preview representation.
    #[test]
    fn persisted_payload_state_surfaces_lost() {
        let preview = persisted(&RepresentationId::new(), "text", "text/plain", b"hello");
        let paste = PersistedClipboardRepresentation::new_with_state(
            RepresentationId::new(),
            FormatId::from("html"),
            Some(MimeType("text/html".to_string())),
            10,
            None,
            None,
            PayloadAvailability::Lost,
            None,
        )
        .expect("lost state is valid without inline data");
        let preview_id = preview.id.clone();
        let paste_id = paste.id.clone();
        let e = entry();
        let decision = ClipboardSelectionDecision::new(
            e.entry_id.clone(),
            ClipboardSelection {
                primary_rep_id: paste_id.clone(),
                secondary_rep_ids: Vec::new(),
                preview_rep_id: preview_id,
                paste_rep_id: paste_id,
                policy_version: SelectionPolicyVersion::V1,
            },
        );

        let input =
            SearchProjectionBuilder::build_from_persisted(&e, &decision, &[preview, paste], None)
                .expect("preview supplies searchable content");

        assert_eq!(input.payload_state.as_deref(), Some("Lost"));
        assert_eq!(input.content_type, ContentType::Html);
    }

    /// A healthy paste representation leaves `payload_state` unset.
    #[test]
    fn persisted_payload_state_healthy_is_none() {
        let rep_id = RepresentationId::new();
        let r = persisted(&rep_id, "text", "text/plain", b"hello");
        let e = entry();
        let decision = ClipboardSelectionDecision::new(
            e.entry_id.clone(),
            ClipboardSelection {
                primary_rep_id: rep_id.clone(),
                secondary_rep_ids: Vec::new(),
                preview_rep_id: rep_id.clone(),
                paste_rep_id: rep_id,
                policy_version: SelectionPolicyVersion::V1,
            },
        );

        let input = SearchProjectionBuilder::build_from_persisted(&e, &decision, &[r], None)
            .expect("inline text is searchable");

        assert_eq!(input.payload_state, None);
    }

    /// Rebuild leg of the favorited mirror: a persisted entry whose stored
    /// favorite flag is set carries the `favorited` tag; an unfavorited entry
    /// does not.
    #[test]
    fn persisted_favorited_entry_carries_favorited_tag() {
        let fav_rep_id = RepresentationId::new();
        let fav_rep = persisted(&fav_rep_id, "text", "text/plain", b"hello");
        let favorited = entry().with_favorited(true);
        let fav_dec = ClipboardSelectionDecision::new(
            favorited.entry_id.clone(),
            ClipboardSelection {
                primary_rep_id: fav_rep_id.clone(),
                secondary_rep_ids: Vec::new(),
                preview_rep_id: fav_rep_id.clone(),
                paste_rep_id: fav_rep_id,
                policy_version: SelectionPolicyVersion::V1,
            },
        );
        let fav_input =
            SearchProjectionBuilder::build_from_persisted(&favorited, &fav_dec, &[fav_rep], None)
                .expect("inline text is searchable");
        assert!(
            fav_input.tags.contains(&TagId::favorited()),
            "a persisted favorited entry carries the favorited tag"
        );

        let plain_rep_id = RepresentationId::new();
        let plain_rep = persisted(&plain_rep_id, "text", "text/plain", b"hello");
        let plain = entry();
        let plain_dec = ClipboardSelectionDecision::new(
            plain.entry_id.clone(),
            ClipboardSelection {
                primary_rep_id: plain_rep_id.clone(),
                secondary_rep_ids: Vec::new(),
                preview_rep_id: plain_rep_id.clone(),
                paste_rep_id: plain_rep_id,
                policy_version: SelectionPolicyVersion::V1,
            },
        );
        let plain_input =
            SearchProjectionBuilder::build_from_persisted(&plain, &plain_dec, &[plain_rep], None)
                .expect("inline text is searchable");
        assert!(
            !plain_input.tags.contains(&TagId::favorited()),
            "an unfavorited entry carries no favorited tag"
        );
    }

    /// The live and rebuild paths must derive identical render metadata from the
    /// same content and the same `source_device` lookup, so a rebuilt index row
    /// renders the same card as the live-indexed one (§4.5 parity).
    #[test]
    fn live_and_rebuild_render_parity() {
        let bytes = b"file:///home/u/a.txt\nhttps://example.com\n";
        let rep_id = RepresentationId::new();
        let e = entry();
        let make_selection = || ClipboardSelection {
            primary_rep_id: rep_id.clone(),
            secondary_rep_ids: Vec::new(),
            preview_rep_id: rep_id.clone(),
            paste_rep_id: rep_id.clone(),
            policy_version: SelectionPolicyVersion::V1,
        };

        let observed = ObservedClipboardRepresentation::new(
            rep_id.clone(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            bytes.to_vec(),
        );
        let snapshot = SystemClipboardSnapshot {
            ts_ms: 1,
            representations: vec![observed],
            file_content_digests: Vec::new(),
        };
        let live = SearchProjectionBuilder::build_from_capture(
            &e,
            &snapshot,
            &make_selection(),
            Some("dev-1".to_string()),
        )
        .expect("live snapshot is searchable");

        let stored = persisted(&rep_id, "files", "text/uri-list", bytes);
        let decision = ClipboardSelectionDecision::new(e.entry_id.clone(), make_selection());
        let rebuilt = SearchProjectionBuilder::build_from_persisted(
            &e,
            &decision,
            &[stored],
            Some("dev-1".to_string()),
        )
        .expect("persisted reps are searchable");

        assert_eq!(live.file_names, rebuilt.file_names);
        assert_eq!(live.link_urls, rebuilt.link_urls);
        assert_eq!(live.source_device, rebuilt.source_device);
        assert_eq!(live.content_type, rebuilt.content_type);
    }
}
