//! Clipboard content category set — a domain rule that maps a
//! `SystemClipboardSnapshot` onto the buckets of `ContentTypes`. Used by
//! application-layer dispatch / ingest use cases to gate against per-device
//! `send_content_types` / `receive_content_types` preferences.
//!
//! ## Why a *set*, not a single category
//!
//! A `SystemClipboardSnapshot` is a multi-rep object — the same logical
//! "thing copied" can carry plain text + HTML, an image + its file URL,
//! a file URI + a preview bitmap, etc. The on-the-wire dispatch sends
//! every rep the user copied, so the gate must reflect the full set
//! actually being transmitted, not a single "primary" label.
//!
//! Concrete cases:
//!
//! * 截图软件 → `image/png` + `text/uri-list` (PNG path) → `{File, Image}`
//! * 网页文字 → `text/plain` + `text/html` → `{Text, RichText}`
//! * 纯文件 → `text/uri-list` → `{File}`
//! * 拖拽 PDF + 预览图 → `text/uri-list` + `image/png` → `{File, Image}`
//! * 纯 markdown rep → `{Text}` (catch-all for `text/*` subtypes)
//!
//! ## Gate semantics: AND-of-allowed
//!
//! For a snapshot whose category set is `S`, the gate allows the
//! dispatch / ingest iff **every** member of `S` is allowed by
//! `ContentTypes`. Any single disabled category vetoes the whole
//! snapshot. Rationale: if the user disabled `image`, they don't want
//! the peer to receive a bitmap — period; the fact that the snapshot
//! also carries a file path doesn't change that.
//!
//! ## Per-rep precedence (set construction)
//!
//! Each rep contributes **at most one** category to the set, chosen by
//! the precedence below — the snapshot's final set is the union over
//! all reps:
//!
//! ```text
//! file > image > rich_text > link > plain_text > text/* catch-all
//! ```
//!
//! `text/uri-list` is `text/*` but semantically a file, so file wins.
//! `text/html` is `text/*` but semantically rich-text, so rich-text wins.
//! Anything that's still a `text/*` after those checks (e.g.
//! `text/markdown`, `text/csv`) falls into `Text` via the catch-all so
//! the `text` toggle still gates them.
//!
//! ### URL detection on platforms that don't expose URL MIMEs
//!
//! macOS' system pasteboard exposes copied URLs *only* as plain text
//! reps — there is no `public.url` rep on the wire. Without a hint, the
//! `link` toggle would be a no-op for the most common "I copied a URL"
//! case. To rescue it, every `text/*` rep that picks up the `Text`
//! bucket also runs through `is_link_content_rep`: if its full payload
//! (after `trim`) is a single URI literal, we additionally insert
//! `Link` into the set. Combined with AND-of-allowed, this means
//! disabling `link` blocks pure-URL clipboards on macOS too.
//!
//! "Looks like a URI": `scheme://...` (scheme = `[a-zA-Z][a-zA-Z0-9+\-.]*`)
//! or `mailto:` / `tel:` / `sms:`; bytes ≤ 4 KiB; no internal whitespace.
//! Mixed prose containing a URL (`"see https://x.com"`) does not match
//! and remains plain `Text`.
//!
//! ## Empty set
//!
//! If no rep matched any known category (raw bytes path, exotic
//! `application/x-…` payloads), the set is empty and the gate fails
//! open — mirrors the member-record-missing fail-open policy in the
//! application-layer dispatch / ingest use cases.

use crate::clipboard::system::{
    is_any_text_representation, is_file_representation, is_image_representation,
    is_link_content_representation, is_link_representation, is_plain_text_representation,
    is_rich_text_representation,
};
use crate::clipboard::SystemClipboardSnapshot;
use crate::settings::model::ContentTypes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardContentCategory {
    File,
    Image,
    Text,
    RichText,
    Link,
}

impl ClipboardContentCategory {
    pub fn allowed_by(&self, ct: &ContentTypes) -> bool {
        match self {
            ClipboardContentCategory::File => ct.file,
            ClipboardContentCategory::Image => ct.image,
            ClipboardContentCategory::Text => ct.text,
            ClipboardContentCategory::RichText => ct.rich_text,
            ClipboardContentCategory::Link => ct.link,
        }
    }

    pub fn as_label(&self) -> &'static str {
        match self {
            ClipboardContentCategory::File => "file",
            ClipboardContentCategory::Image => "image",
            ClipboardContentCategory::Text => "text",
            ClipboardContentCategory::RichText => "rich_text",
            ClipboardContentCategory::Link => "link",
        }
    }

    fn bit(self) -> u8 {
        match self {
            ClipboardContentCategory::File => 0,
            ClipboardContentCategory::Image => 1,
            ClipboardContentCategory::Text => 2,
            ClipboardContentCategory::RichText => 3,
            ClipboardContentCategory::Link => 4,
        }
    }
}

const ALL_CATEGORIES: &[ClipboardContentCategory] = &[
    ClipboardContentCategory::File,
    ClipboardContentCategory::Image,
    ClipboardContentCategory::Text,
    ClipboardContentCategory::RichText,
    ClipboardContentCategory::Link,
];

/// Bitset of content categories present in a snapshot. Empty = unknown
/// payload (fail-open). See module doc for AND-of-allowed gate semantics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClipboardContentCategorySet {
    flags: u8,
}

impl ClipboardContentCategorySet {
    pub const fn empty() -> Self {
        Self { flags: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.flags == 0
    }

    pub fn insert(&mut self, c: ClipboardContentCategory) {
        self.flags |= 1 << c.bit();
    }

    /// Iterate categories present in the set, in stable enum order.
    pub fn iter(&self) -> impl Iterator<Item = ClipboardContentCategory> + '_ {
        let flags = self.flags;
        ALL_CATEGORIES
            .iter()
            .copied()
            .filter(move |c| flags & (1 << c.bit()) != 0)
    }

    /// Comma-separated labels for logging, e.g. `"file,image"`. Empty set
    /// renders as `"unknown"` to keep log lines non-empty and grep-able.
    pub fn labels(&self) -> String {
        if self.is_empty() {
            return "unknown".to_string();
        }
        let mut out = String::new();
        for c in self.iter() {
            if !out.is_empty() {
                out.push(',');
            }
            out.push_str(c.as_label());
        }
        out
    }

    /// AND-of-allowed: empty set fails open; otherwise every present
    /// category must be allowed by `ct`.
    pub fn allowed_by(&self, ct: &ContentTypes) -> bool {
        if self.is_empty() {
            return true;
        }
        self.iter().all(|c| c.allowed_by(ct))
    }

    /// Categories present in the set *and* disabled by `ct`. Non-empty
    /// iff `allowed_by` returns `false`. Used for "why was this dropped"
    /// log lines.
    pub fn denied_by(&self, ct: &ContentTypes) -> Vec<ClipboardContentCategory> {
        self.iter().filter(|c| !c.allowed_by(ct)).collect()
    }

    /// Comma-separated labels for the subset returned by `denied_by`.
    pub fn denied_labels(&self, ct: &ContentTypes) -> String {
        let mut out = String::new();
        for c in self.denied_by(ct) {
            if !out.is_empty() {
                out.push(',');
            }
            out.push_str(c.as_label());
        }
        out
    }

    /// Build the set from a snapshot. Each rep contributes at most one
    /// category via the precedence chain documented at module level.
    pub fn from_snapshot(snap: &SystemClipboardSnapshot) -> Self {
        let mut s = Self::empty();
        for r in &snap.representations {
            if r.size_bytes() == 0 {
                continue;
            }
            if is_file_representation(r) {
                s.insert(ClipboardContentCategory::File);
            } else if is_image_representation(r) {
                s.insert(ClipboardContentCategory::Image);
            } else if is_rich_text_representation(r) {
                s.insert(ClipboardContentCategory::RichText);
            } else if is_link_representation(r) {
                s.insert(ClipboardContentCategory::Link);
            } else if is_plain_text_representation(r) || is_any_text_representation(r) {
                s.insert(ClipboardContentCategory::Text);
                // macOS exposes copied URLs only as plain text — recover
                // the Link signal via content heuristic so the per-device
                // `link` toggle isn't a no-op on macOS.
                if is_link_content_representation(r) {
                    s.insert(ClipboardContentCategory::Link);
                }
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};
    use crate::ids::{FormatId, RepresentationId};

    fn rep(format_id: &str, mime: Option<&str>, bytes: &[u8]) -> ObservedClipboardRepresentation {
        ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from(format_id),
            mime.map(|m| MimeType(m.to_string())),
            bytes.to_vec(),
        )
    }

    fn snap(reps: Vec<ObservedClipboardRepresentation>) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: reps,
        }
    }

    fn set_of(cats: &[ClipboardContentCategory]) -> ClipboardContentCategorySet {
        let mut s = ClipboardContentCategorySet::empty();
        for c in cats {
            s.insert(*c);
        }
        s
    }

    // ── set construction ───────────────────────────────────────────────

    #[test]
    fn plain_text_only_classifies_as_text() {
        let s = snap(vec![rep("text", Some("text/plain"), b"hi")]);
        assert_eq!(
            ClipboardContentCategorySet::from_snapshot(&s),
            set_of(&[ClipboardContentCategory::Text])
        );
    }

    #[test]
    fn rich_text_only_classifies_as_rich_text() {
        let s = snap(vec![rep("html", Some("text/html"), b"<p>hi</p>")]);
        assert_eq!(
            ClipboardContentCategorySet::from_snapshot(&s),
            set_of(&[ClipboardContentCategory::RichText])
        );
    }

    #[test]
    fn webpage_text_yields_text_and_rich_text() {
        // Browser/rich-editor copy: text/plain + text/html together.
        let s = snap(vec![
            rep("text", Some("text/plain"), b"hello"),
            rep("html", Some("text/html"), b"<p>hello</p>"),
        ]);
        assert_eq!(
            ClipboardContentCategorySet::from_snapshot(&s),
            set_of(&[
                ClipboardContentCategory::Text,
                ClipboardContentCategory::RichText
            ])
        );
    }

    #[test]
    fn screenshot_yields_file_and_image() {
        // Screenshot tools: PNG bitmap + a file URL pointing at the saved file.
        let s = snap(vec![
            rep("image", Some("image/png"), b"\x89PNG\x0D\x0A\x1A\x0A"),
            rep("files", Some("text/uri-list"), b"file:///tmp/shot.png"),
        ]);
        assert_eq!(
            ClipboardContentCategorySet::from_snapshot(&s),
            set_of(&[
                ClipboardContentCategory::File,
                ClipboardContentCategory::Image
            ])
        );
    }

    #[test]
    fn file_only_yields_file() {
        let s = snap(vec![rep(
            "files",
            Some("text/uri-list"),
            b"file:///tmp/a.bin",
        )]);
        assert_eq!(
            ClipboardContentCategorySet::from_snapshot(&s),
            set_of(&[ClipboardContentCategory::File])
        );
    }

    #[test]
    fn link_with_text_fallback_yields_text_and_link() {
        let s = snap(vec![
            rep("text", Some("text/plain"), b"https://x.com"),
            rep("url", Some("text/x-url"), b"https://x.com"),
        ]);
        assert_eq!(
            ClipboardContentCategorySet::from_snapshot(&s),
            set_of(&[
                ClipboardContentCategory::Text,
                ClipboardContentCategory::Link
            ])
        );
    }

    #[test]
    fn exotic_text_subtype_falls_back_to_text() {
        // text/markdown isn't plain / html / rtf / link / file — catch-all
        // surfaces it as Text so the `text` toggle still gates it.
        let s = snap(vec![rep("md", Some("text/markdown"), b"# hi")]);
        assert_eq!(
            ClipboardContentCategorySet::from_snapshot(&s),
            set_of(&[ClipboardContentCategory::Text])
        );
    }

    // ── URL content heuristic ──────────────────────────────────────────

    #[test]
    fn plain_text_that_is_a_url_yields_text_and_link() {
        // macOS-style: copied URL surfaces only as a plain-text rep.
        let s = snap(vec![rep(
            "text",
            Some("text/plain"),
            b"https://example.com",
        )]);
        assert_eq!(
            ClipboardContentCategorySet::from_snapshot(&s),
            set_of(&[
                ClipboardContentCategory::Text,
                ClipboardContentCategory::Link
            ])
        );
    }

    #[test]
    fn plain_text_url_with_surrounding_whitespace_is_still_link() {
        // Trim is intentional — macOS sometimes adds a trailing newline.
        let s = snap(vec![rep("text", Some("text/plain"), b"  https://x.com\n")]);
        assert_eq!(
            ClipboardContentCategorySet::from_snapshot(&s),
            set_of(&[
                ClipboardContentCategory::Text,
                ClipboardContentCategory::Link
            ])
        );
    }

    #[test]
    fn mailto_and_tel_uris_classify_as_link() {
        let s = snap(vec![rep("text", Some("text/plain"), b"mailto:a@b.co")]);
        assert_eq!(
            ClipboardContentCategorySet::from_snapshot(&s),
            set_of(&[
                ClipboardContentCategory::Text,
                ClipboardContentCategory::Link
            ])
        );
        let s = snap(vec![rep("text", Some("text/plain"), b"tel:+15551234567")]);
        assert_eq!(
            ClipboardContentCategorySet::from_snapshot(&s),
            set_of(&[
                ClipboardContentCategory::Text,
                ClipboardContentCategory::Link
            ])
        );
    }

    #[test]
    fn url_embedded_in_prose_does_not_classify_as_link() {
        // The whole rep must be a single URI; mixed text → just Text.
        let s = snap(vec![rep(
            "text",
            Some("text/plain"),
            b"see https://x.com for details",
        )]);
        assert_eq!(
            ClipboardContentCategorySet::from_snapshot(&s),
            set_of(&[ClipboardContentCategory::Text])
        );
    }

    #[test]
    fn malformed_scheme_is_not_a_link() {
        // No "://", not a recognised schemeless prefix.
        let s = snap(vec![rep("text", Some("text/plain"), b"ftp:noslashes")]);
        assert_eq!(
            ClipboardContentCategorySet::from_snapshot(&s),
            set_of(&[ClipboardContentCategory::Text])
        );
    }

    #[test]
    fn oversize_text_skips_link_heuristic() {
        // 5 KiB plain text starting with a URL: too long to inspect.
        let mut payload = Vec::with_capacity(5000);
        payload.extend_from_slice(b"https://x.com");
        payload.resize(5000, b'a');
        let s = snap(vec![rep("text", Some("text/plain"), &payload)]);
        assert_eq!(
            ClipboardContentCategorySet::from_snapshot(&s),
            set_of(&[ClipboardContentCategory::Text])
        );
    }

    #[test]
    fn empty_or_unknown_snapshot_yields_empty_set() {
        assert!(ClipboardContentCategorySet::from_snapshot(&snap(vec![])).is_empty());
        let s = snap(vec![rep("weird", Some("application/x-private"), b"???")]);
        assert!(ClipboardContentCategorySet::from_snapshot(&s).is_empty());
    }

    #[test]
    fn zero_byte_reps_are_ignored() {
        // A type-only rep with no bytes should not contribute a category.
        let s = snap(vec![
            rep("text", Some("text/plain"), b"hi"),
            rep("image", Some("image/png"), b""),
        ]);
        assert_eq!(
            ClipboardContentCategorySet::from_snapshot(&s),
            set_of(&[ClipboardContentCategory::Text])
        );
    }

    // ── gate semantics: AND-of-allowed ─────────────────────────────────

    #[test]
    fn empty_set_fails_open_against_all_off_filter() {
        let mut all_off = ContentTypes::default();
        all_off.text = false;
        all_off.image = false;
        all_off.link = false;
        all_off.file = false;
        all_off.rich_text = false;
        all_off.code_snippet = false;
        let empty = ClipboardContentCategorySet::empty();
        assert!(empty.allowed_by(&all_off));
        assert!(empty.denied_by(&all_off).is_empty());
    }

    #[test]
    fn screenshot_blocked_when_image_disabled_even_if_file_allowed() {
        // `{File, Image}` with image=false → blocked (AND-of-allowed).
        let s = set_of(&[
            ClipboardContentCategory::File,
            ClipboardContentCategory::Image,
        ]);
        let mut ct = ContentTypes::default();
        ct.image = false;
        assert!(!s.allowed_by(&ct));
        assert_eq!(s.denied_by(&ct), vec![ClipboardContentCategory::Image]);
    }

    #[test]
    fn screenshot_blocked_when_file_disabled_even_if_image_allowed() {
        let s = set_of(&[
            ClipboardContentCategory::File,
            ClipboardContentCategory::Image,
        ]);
        let mut ct = ContentTypes::default();
        ct.file = false;
        assert!(!s.allowed_by(&ct));
        assert_eq!(s.denied_by(&ct), vec![ClipboardContentCategory::File]);
    }

    #[test]
    fn webpage_blocked_when_rich_text_disabled_even_if_text_allowed() {
        // `{Text, RichText}` with rich_text=false → blocked. This is the
        // strict-AND tradeoff: disabling rich_text rejects browser copies
        // because the html rep would still go on the wire otherwise.
        let s = set_of(&[
            ClipboardContentCategory::Text,
            ClipboardContentCategory::RichText,
        ]);
        let mut ct = ContentTypes::default();
        ct.rich_text = false;
        assert!(!s.allowed_by(&ct));
        assert_eq!(s.denied_by(&ct), vec![ClipboardContentCategory::RichText]);
    }

    #[test]
    fn all_present_categories_allowed_passes_gate() {
        let s = set_of(&[
            ClipboardContentCategory::File,
            ClipboardContentCategory::Image,
        ]);
        let ct = ContentTypes::default(); // all true
        assert!(s.allowed_by(&ct));
        assert!(s.denied_by(&ct).is_empty());
    }

    #[test]
    fn multiple_disabled_categories_all_reported_in_denied_by() {
        let s = set_of(&[
            ClipboardContentCategory::Text,
            ClipboardContentCategory::RichText,
            ClipboardContentCategory::Image,
        ]);
        let mut ct = ContentTypes::default();
        ct.text = false;
        ct.image = false;
        let mut denied = s.denied_by(&ct);
        denied.sort_by_key(|c| c.bit());
        assert_eq!(
            denied,
            vec![
                ClipboardContentCategory::Image,
                ClipboardContentCategory::Text
            ]
        );
    }

    // ── labels ─────────────────────────────────────────────────────────

    #[test]
    fn labels_renders_present_categories_in_enum_order() {
        let s = set_of(&[
            ClipboardContentCategory::Image,
            ClipboardContentCategory::File,
        ]);
        // ALL_CATEGORIES ordering is File before Image, regardless of insert order.
        assert_eq!(s.labels(), "file,image");
    }

    #[test]
    fn labels_for_empty_set_is_unknown() {
        assert_eq!(ClipboardContentCategorySet::empty().labels(), "unknown");
    }
}
