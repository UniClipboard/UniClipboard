//! SyncClipboard on-the-wire clipboard snapshot (`SyncClipboard.json`) and the
//! publish helpers that build it (spec §3 / §3.3 / §3.4 / §4.1 / §4.2).
//!
//! Normative source: `Clipboard.swift` (struct `Clipboard`, `fromText`,
//! `publishText`, `publishImage`, `publishFile`, `sanitizedFilename`) plus its
//! golden vectors in `PublishTests.swift`, `HashTests.swift`,
//! `FixturesTests.swift` and the `docs/examples/clipboard_*.json` wire
//! fixtures from the uc-ios app. Migration baseline:
//! `.planning/research/uc-ios-regression-checklist.md` items A2 (Clipboard
//! rows) and A4 (long-text overflow).
//!
//! ## BYTE-CRITICAL invariants (checklist A2/A4 🔴)
//! - JSON field names are exactly `type` / `hash` / `text` / `hasData` /
//!   `dataName` / `size`, encoded in that order (struct declaration order).
//!   `None` fields are **OMITTED ENTIRELY** — never serialized as `null`
//!   (spec §3.1; Swift uses `encodeIfPresent`).
//! - `type` enum raw values are `Text` / `Image` / `File` / `Group`.
//! - Long-text overflow threshold is **10240 GRAPHEME CLUSTERS** (Swift
//!   `String.count` semantics, UAX #29 extended grapheme clusters via
//!   `unicode-segmentation`) — NOT bytes, NOT Unicode scalars (spec §3.4).
//!   The rule is strictly `> 10240`: exactly 10240 stays inline.
//! - Overflow shape: `text` = first-10240-grapheme preview, `hasData = true`,
//!   `dataName = "text_{HASH}.txt"` where `HASH` is the uppercase SHA-256 of
//!   the **FULL** text, payload = full UTF-8 bytes, `size` = full-text
//!   **grapheme count** (Swift `text.count`), `hash` over the full text.
//! - Image/file `size` is the **byte** count; image/file `hash` is SHA-256
//!   over the raw payload bytes — the file name never participates (§4.2).
//!
//! Pure codec only: no I/O, no clipboard access, no upload logic.

use serde::{Deserialize, Deserializer, Serialize};
use unicode_segmentation::UnicodeSegmentation;

use crate::hash::sha256_hex_upper;

/// Spec §3.4 long-text overflow threshold, in **grapheme clusters**
/// (`Clipboard.swift` `publishText`: `let threshold = 10_240` compared
/// against Swift `text.count`).
pub const LONG_TEXT_THRESHOLD: usize = 10_240;

/// Fallback file name when [`sanitized_filename`] strips a name down to
/// nothing (`Clipboard.swift` `sanitizedFilename`).
const FALLBACK_FILENAME: &str = "file";

/// Wire value of the `type` field. Raw values per `Clipboard.Kind` in
/// `Clipboard.swift`: `Text` / `Image` / `File` / `Group` (BYTE-CRITICAL —
/// serde unit-variant names serialize verbatim, so no renames are needed,
/// but the variant spelling must never change).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ClipboardKind {
    Text,
    Image,
    File,
    Group,
}

impl ClipboardKind {
    /// The exact wire string (Swift `rawValue`).
    pub fn as_wire_str(self) -> &'static str {
        match self {
            ClipboardKind::Text => "Text",
            ClipboardKind::Image => "Image",
            ClipboardKind::File => "File",
            ClipboardKind::Group => "Group",
        }
    }
}

/// On-the-wire clipboard snapshot. Spec §3; mirrors `struct Clipboard` in
/// `Clipboard.swift`.
///
/// Field declaration order is the wire encode order (`type`, `hash`, `text`,
/// `hasData`, `dataName`, `size`) — Swift's hand-written `encode(to:)` emits
/// the keys in exactly this order, and serde serializes struct fields in
/// declaration order. Optional fields use `skip_serializing_if` so `None` is
/// omitted entirely, never written as `null` (spec §3.1, BYTE-CRITICAL).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Clipboard {
    /// Content kind; wire key `type`.
    #[serde(rename = "type")]
    pub kind: ClipboardKind,
    /// Uppercase SHA-256 content hash (§4.1/§4.2). Spec §3.1: empty /
    /// whitespace-only values normalize to `None` so the encoder omits the
    /// key (`Clipboard.normalizeHash` runs both in `init` and on decode).
    #[serde(
        default,
        deserialize_with = "de_normalized_hash",
        skip_serializing_if = "Option::is_none"
    )]
    pub hash: Option<String>,
    /// Inline text (full text, or the §3.4 preview when overflowed). For
    /// non-text kinds this is the basename label per §3.3.
    pub text: String,
    /// Whether a separate payload file accompanies this entry.
    #[serde(rename = "hasData")]
    pub has_data: bool,
    /// Payload file name (`text_{HASH}.txt`, `image.{ext}`, sanitized
    /// basename, ...). Omitted when absent.
    #[serde(rename = "dataName", default, skip_serializing_if = "Option::is_none")]
    pub data_name: Option<String>,
    /// Content size. UNIT IS KIND-DEPENDENT (matches Swift exactly): grapheme
    /// count of the full text for `Text` (Swift `text.count`), byte count for
    /// image/file payloads. Omitted when absent. `i64` mirrors Swift's
    /// signed `Int` (64-bit on Apple platforms) for exact decode parity: a
    /// degenerate negative `size` from a buggy peer decodes instead of
    /// rejecting the whole document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
}

impl Clipboard {
    /// Construct a snapshot, normalizing `hash` like Swift's `Clipboard.init`
    /// (empty / whitespace-only → `None`, see [`normalize_hash`]).
    pub fn new(
        kind: ClipboardKind,
        hash: Option<String>,
        text: String,
        has_data: bool,
        data_name: Option<String>,
        size: Option<i64>,
    ) -> Self {
        Self {
            kind,
            hash: normalize_hash(hash),
            text,
            has_data,
            data_name,
            size,
        }
    }

    /// Build a `Clipboard` from a plain device-pasteboard string. Mirrors
    /// `Clipboard.fromText` in `Clipboard.swift`: always computes the §4.1
    /// hash; `hasData` is false and `size` is the **grapheme count**. The
    /// §3.4 long-text overflow transform is the publish path's job
    /// ([`publish_text`]), NOT the observe path's — long text stays whole
    /// here (`HashTests.test_H4`).
    pub fn from_text(text: &str) -> Self {
        Self {
            kind: ClipboardKind::Text,
            hash: Some(sha256_hex_upper(text.as_bytes())),
            text: text.to_string(),
            has_data: false,
            data_name: None,
            size: Some(grapheme_count(text) as i64),
        }
    }
}

/// Spec §3.4 — produce the publishable `Clipboard` + optional payload bytes
/// for a piece of plain text. Mirrors `Clipboard.publishText`.
///
/// Long text (**strictly more than** [`LONG_TEXT_THRESHOLD`] grapheme
/// clusters — exactly 10240 stays inline, `PublishTests.test_Pub2`) triggers
/// the file-overflow branch:
/// - `text` = only the first 10240 graphemes (Swift `text.prefix(threshold)`)
/// - `has_data = true`
/// - `data_name = "text_{HASH}.txt"` with `HASH` = uppercase SHA-256 of the
///   FULL text (§4.1)
/// - payload = the full UTF-8 bytes
/// - `size` = the FULL text's grapheme count, NOT the preview length
///   (`PublishTests.test_Pub7`)
///
/// Short text fits inline: `has_data = false`, full text, no payload.
pub fn publish_text(text: &str) -> (Clipboard, Option<Vec<u8>>) {
    let hash = sha256_hex_upper(text.as_bytes());
    let count = grapheme_count(text);
    if count > LONG_TEXT_THRESHOLD {
        // Byte offset where grapheme #LONG_TEXT_THRESHOLD (0-based) starts =
        // end of the first LONG_TEXT_THRESHOLD graphemes. `count > threshold`
        // guarantees `nth` yields a value; `map_or` keeps the no-unwrap rule.
        let preview_end = text
            .grapheme_indices(true)
            .nth(LONG_TEXT_THRESHOLD)
            .map_or(text.len(), |(i, _)| i);
        let preview = text[..preview_end].to_string();
        let data_name = format!("text_{hash}.txt");
        let entry = Clipboard {
            kind: ClipboardKind::Text,
            hash: Some(hash),
            text: preview,
            has_data: true,
            data_name: Some(data_name),
            size: Some(count as i64),
        };
        return (entry, Some(text.as_bytes().to_vec()));
    }
    let entry = Clipboard {
        kind: ClipboardKind::Text,
        hash: Some(hash),
        text: text.to_string(),
        has_data: false,
        data_name: None,
        size: Some(count as i64),
    };
    (entry, None)
}

/// Spec §3.3 + §4.2 — produce the publishable `Clipboard` + payload bytes for
/// raw image bytes of a known extension. Mirrors `Clipboard.publishImage`.
///
/// `data_name` and `text` are both `"image.{ext}"` (spec §3.3: non-text
/// `text` = label = `basename(dataName)`); `hash` is the raw-bytes SHA-256
/// per §4.2 (the name does NOT participate); `size` is the **byte** length.
/// Image is always `has_data = true`. The payload is returned byte-identical
/// to the input — no re-encoding (`HashTests.test_F6`).
pub fn publish_image(bytes: &[u8], ext: &str) -> (Clipboard, Vec<u8>) {
    let data_name = format!("image.{ext}");
    let entry = Clipboard {
        kind: ClipboardKind::Image,
        hash: Some(sha256_hex_upper(bytes)),
        text: data_name.clone(),
        has_data: true,
        data_name: Some(data_name),
        size: Some(bytes.len() as i64),
    };
    (entry, bytes.to_vec())
}

/// Spec §3.3 + §4.2 — produce the publishable `Clipboard` + payload bytes for
/// an arbitrary file. Mirrors `Clipboard.publishFile`.
///
/// `name` is sanitized to a bytewise basename via [`sanitized_filename`]
/// (the upload endpoint rejects `/` and `\`). `text` mirrors the basename
/// per §3.3. `hash` is the raw-bytes SHA-256 per §4.2; `has_data = true`;
/// `size` is the **byte** length.
pub fn publish_file(name: &str, bytes: &[u8]) -> (Clipboard, Vec<u8>) {
    let safe = sanitized_filename(name);
    let entry = Clipboard {
        kind: ClipboardKind::File,
        hash: Some(sha256_hex_upper(bytes)),
        text: safe.clone(),
        has_data: true,
        data_name: Some(safe),
        size: Some(bytes.len() as i64),
    };
    (entry, bytes.to_vec())
}

/// Strip path components from a filename and reject empty results. Mirrors
/// `Clipboard.sanitizedFilename` in `Clipboard.swift` exactly, including the
/// order of operations: take the substring after the last `/`, THEN after the
/// last `\`, then trim whitespace; fall back to `"file"` when the stripped
/// name is empty or whitespace-only. No percent-decoding, no normalization.
///
/// Swift's `lastIndex(of: "/")` compares `Character`s, i.e. extended
/// grapheme clusters: a separator immediately followed by a combining mark
/// (`"/\u{301}"`) is ONE grapheme that does NOT equal `"/"`, so Swift does
/// not split there. The search below is grapheme-based for the same
/// semantics — a scalar `rfind('/')` would diverge on that input.
pub fn sanitized_filename(raw: &str) -> String {
    let mut name = raw;
    if let Some(i) = rfind_grapheme(name, "/") {
        name = &name[i + '/'.len_utf8()..];
    }
    if let Some(i) = rfind_grapheme(name, "\\") {
        name = &name[i + '\\'.len_utf8()..];
    }
    let name = name.trim();
    if name.is_empty() {
        FALLBACK_FILENAME.to_string()
    } else {
        name.to_string()
    }
}

/// Byte offset of the last grapheme cluster exactly equal to `target`
/// (Swift `String.lastIndex(of: Character)` semantics).
fn rfind_grapheme(s: &str, target: &str) -> Option<usize> {
    s.grapheme_indices(true)
        .rev()
        .find(|(_, g)| *g == target)
        .map(|(i, _)| i)
}

/// Swift `String.count` equivalent: extended grapheme cluster count
/// (UAX #29). BYTE-CRITICAL for the §3.4 threshold — never replace with
/// `len()` (bytes) or `chars().count()` (Unicode scalars).
fn grapheme_count(text: &str) -> usize {
    text.graphemes(true).count()
}

/// Spec §3.1 — empty / whitespace-only hash normalizes to `None` so the
/// encoder omits the key. Mirrors `Clipboard.normalizeHash` (Swift trims
/// `.whitespacesAndNewlines`; `str::trim` is the equivalent Unicode
/// `White_Space` trim).
fn normalize_hash(raw: Option<String>) -> Option<String> {
    let trimmed = raw.as_deref().map(str::trim).unwrap_or_default();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Field-level deserializer applying [`normalize_hash`], matching Swift's
/// `init(from:)` which routes `decodeIfPresent(.hash)` through
/// `normalizeHash`. Absent key → `None` via `#[serde(default)]`; explicit
/// `null` and whitespace-only strings also land on `None`.
fn de_normalized_hash<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    Ok(normalize_hash(raw))
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    use super::*;

    const THRESHOLD: usize = LONG_TEXT_THRESHOLD;

    /// 8×8 red-square PNG, ~70 bytes — fixture base64 copied verbatim from
    /// `HashTests.swift` (`red8x8PNG`).
    fn red_8x8_png() -> Vec<u8> {
        STANDARD
            .decode(concat!(
                "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAIAQMAAAD+wSzIAAAABGdBTUEAALGP",
                "C/xhBQAAAAFzUkdCAK7OHOkAAAAGUExURf8AAP///8jJRKEAAAAOSURBVAjX",
                "Y/jPwMDAAAAEAQEAQYxqNwAAAABJRU5ErkJggg=="
            ))
            .expect("fixture base64 decodes")
    }

    /// Key set of a JSON object string (Swift tests' `keys(_:)` helper).
    fn keys(json: &str) -> BTreeSet<String> {
        match serde_json::from_str::<serde_json::Value>(json).expect("valid json") {
            serde_json::Value::Object(m) => m.keys().cloned().collect(),
            other => panic!("expected JSON object, got {other:?}"),
        }
    }

    fn key_set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    fn encode(clip: &Clipboard) -> String {
        serde_json::to_string(clip).expect("encode ok")
    }

    fn decode(json: &str) -> Clipboard {
        serde_json::from_str(json).expect("decode ok")
    }

    // ── fromText factory (HashTests.swift) ─────────────────────────────

    /// Ported from `HashTests.test_H3_fromText_setsTypeHashSize_andHasDataFalse`.
    #[test]
    fn h3_from_text_sets_type_hash_size_and_has_data_false() {
        let c = Clipboard::from_text("hi");
        assert_eq!(c.kind, ClipboardKind::Text);
        assert_eq!(c.hash.as_deref(), Some(sha256_hex_upper(b"hi").as_str()));
        assert_eq!(c.text, "hi");
        assert_eq!(c.size, Some(2));
        assert!(!c.has_data);
        assert_eq!(c.data_name, None);
    }

    /// Ported from `HashTests.test_H4_fromText_longString_doesNotApplySection34Transform`.
    /// §3.4's preview/payload split is upload-time only; observe-time
    /// `from_text` must keep the full text and `has_data = false`.
    #[test]
    fn h4_from_text_long_string_does_not_apply_section_3_4_transform() {
        let long = "a".repeat(10_500);
        let c = Clipboard::from_text(&long);
        assert_eq!(
            c.text.graphemes(true).count(),
            10_500,
            "must keep full text, not truncate to preview"
        );
        assert!(
            !c.has_data,
            "§3.4 transform must NOT be applied at observe time"
        );
        assert_eq!(c.data_name, None);
        assert_eq!(c.hash, Some(sha256_hex_upper(long.as_bytes())));
    }

    // ── file/image hash §4.2 + publishImage (HashTests.swift) ──────────

    /// Ported from `HashTests.test_F1_fileHash_isRawBytesSHA256_nameDoesNotParticipate`.
    /// §4.2 — the file/image hash is plain SHA256(bytes); the filename must
    /// NOT bind into the hash.
    #[test]
    fn f1_file_hash_is_raw_bytes_sha256_name_does_not_participate() {
        let bytes = red_8x8_png();
        let (as_image, _) = publish_image(&bytes, "png");
        let (as_file, _) = publish_file("anything.png", &bytes);
        assert_eq!(as_image.hash, Some(sha256_hex_upper(&bytes)));
        assert_eq!(as_file.hash, Some(sha256_hex_upper(&bytes)));
        assert_eq!(
            as_image.hash, as_file.hash,
            "filename must NOT bind into the hash — same bytes, same hash"
        );
    }

    /// Ported from `HashTests.test_F3_fileHash_differentBytes_differentHash`.
    #[test]
    fn f3_file_hash_different_bytes_different_hash() {
        let h1 = sha256_hex_upper(&red_8x8_png());
        let h2 = sha256_hex_upper(&[0xFF, 0xD8, 0xFF]); // not red8x8
        assert_ne!(h1, h2);
    }

    /// Ported from `HashTests.test_F5_publishImage_producesCorrectShape`.
    #[test]
    fn f5_publish_image_produces_correct_shape() {
        let bytes = red_8x8_png();
        let (clip, _) = publish_image(&bytes, "png");
        assert_eq!(clip.kind, ClipboardKind::Image);
        assert!(clip.has_data);
        assert_eq!(clip.data_name.as_deref(), Some("image.png"));
        assert_eq!(
            clip.text, "image.png",
            "non-text text-field must be the basename per §3.3"
        );
        assert_eq!(clip.size, Some(bytes.len() as i64));
        assert_eq!(clip.hash, Some(sha256_hex_upper(&bytes)));
    }

    /// Ported from `HashTests.test_F6_publishImage_payload_isByteIdenticalToInput`.
    /// Bytes-preservation discipline: `publish_image` must NOT re-encode the
    /// input, or the §4.2 wire hash silently stops matching.
    #[test]
    fn f6_publish_image_payload_is_byte_identical_to_input() {
        let bytes = red_8x8_png();
        let (_, payload) = publish_image(&bytes, "png");
        assert_eq!(payload, bytes);
    }

    // ── publishText short path (PublishTests.swift) ────────────────────

    /// Ported from `PublishTests.test_Pub1_shortText_hasDataFalse_payloadNil_textIsFull`.
    #[test]
    fn pub1_short_text_has_data_false_payload_none_text_is_full() {
        let (clip, payload) = publish_text("hi");
        assert!(!clip.has_data);
        assert_eq!(payload, None);
        assert_eq!(clip.text, "hi");
        assert_eq!(clip.size, Some(2));
        assert_eq!(clip.kind, ClipboardKind::Text);
        assert_eq!(clip.data_name, None);
        assert_eq!(clip.hash, Some(sha256_hex_upper(b"hi")));
    }

    /// Ported from `PublishTests.test_Pub2_atThreshold_staysInline`.
    #[test]
    fn pub2_at_threshold_stays_inline() {
        let s = "a".repeat(THRESHOLD);
        let (clip, payload) = publish_text(&s);
        assert!(
            !clip.has_data,
            "exactly threshold-length must be inline (rule is `> threshold`)"
        );
        assert_eq!(payload, None);
        assert_eq!(clip.text.graphemes(true).count(), THRESHOLD);
        assert_eq!(clip.size, Some(THRESHOLD as i64));
    }

    /// Ported from `PublishTests.test_Pub8_emptyString_isInline`.
    #[test]
    fn pub8_empty_string_is_inline() {
        let (clip, payload) = publish_text("");
        assert!(!clip.has_data);
        assert_eq!(payload, None);
        assert_eq!(clip.size, Some(0));
        assert_eq!(clip.hash, Some(sha256_hex_upper(b"")));
    }

    // ── publishText long path (PublishTests.swift) ─────────────────────

    /// Ported from `PublishTests.test_Pub3_aboveThreshold_overflowTriggers`.
    #[test]
    fn pub3_above_threshold_overflow_triggers() {
        let s = "a".repeat(THRESHOLD + 1);
        let (clip, payload) = publish_text(&s);
        assert!(clip.has_data);
        assert!(payload.is_some());
    }

    /// Ported from `PublishTests.test_Pub4_dataNameBindsToHash`.
    #[test]
    fn pub4_data_name_binds_to_hash() {
        let s = "z".repeat(THRESHOLD + 100);
        let (clip, _) = publish_text(&s);
        let hash = clip.hash.clone().expect("overflow entry carries a hash");
        assert_eq!(clip.data_name, Some(format!("text_{hash}.txt")));
    }

    /// Ported from `PublishTests.test_Pub5_textIsExactlyFirstThresholdChars`.
    #[test]
    fn pub5_text_is_exactly_first_threshold_chars() {
        let s = "x".repeat(THRESHOLD + 500) + "_TAIL";
        let (clip, _) = publish_text(&s);
        assert_eq!(clip.text.graphemes(true).count(), THRESHOLD);
        assert_eq!(clip.text, s[..THRESHOLD].to_string());
        assert!(!clip.text.contains("_TAIL"));
    }

    /// Ported from `PublishTests.test_Pub6_payloadIsFullUTF8Bytes`.
    /// Multi-byte UTF-8: each "你" is 3 UTF-8 bytes but ONE grapheme — the
    /// threshold counts graphemes while the payload carries all the bytes.
    #[test]
    fn pub6_payload_is_full_utf8_bytes() {
        let s = "你".repeat(THRESHOLD + 10);
        let (_, payload) = publish_text(&s);
        let payload = payload.expect("multi-byte long text must overflow");
        assert_eq!(payload, s.as_bytes());
        // sanity: 10250 chars × 3 bytes = 30750 bytes
        assert_eq!(payload.len(), (THRESHOLD + 10) * 3);
    }

    /// Ported from `PublishTests.test_Pub7_sizeIsFullCharacterCount_notPreviewLength`.
    #[test]
    fn pub7_size_is_full_character_count_not_preview_length() {
        let full = THRESHOLD + 777;
        let s = "a".repeat(full);
        let (clip, _) = publish_text(&s);
        assert_eq!(
            clip.size,
            Some(full as i64),
            "size MUST be the full text count, not the preview length"
        );
    }

    // ── Rust-only grapheme-boundary attacks (Swift `String.count` parity) ──

    /// Multi-scalar emoji: "👨‍👩‍👧‍👦" is 7 Unicode scalars / 25 UTF-8 bytes but ONE
    /// grapheme cluster (Swift `"👨‍👩‍👧‍👦".count == 1`). 10240 of them must stay
    /// inline; a byte- or scalar-based threshold would overflow long before.
    #[test]
    fn threshold_counts_graphemes_not_bytes_or_scalars() {
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}"; // 👨‍👩‍👧‍👦
        assert_eq!(family.graphemes(true).count(), 1);
        let s = family.repeat(THRESHOLD);
        let (clip, payload) = publish_text(&s);
        assert!(!clip.has_data, "10240 grapheme clusters must stay inline");
        assert_eq!(payload, None);
        assert_eq!(clip.size, Some(THRESHOLD as i64));

        let (clip, payload) = publish_text(&family.repeat(THRESHOLD + 1));
        assert!(clip.has_data, "10241 grapheme clusters must overflow");
        assert!(payload.is_some());
        assert_eq!(clip.size, Some((THRESHOLD + 1) as i64));
    }

    /// Preview slicing when the 10240th grapheme is multi-byte: the preview
    /// must end exactly after the 10240th GRAPHEME (the emoji), never split
    /// it mid-cluster.
    #[test]
    fn overflow_preview_keeps_multibyte_boundary_grapheme_intact() {
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}"; // 👨‍👩‍👧‍👦
        let s = "a".repeat(THRESHOLD - 1) + family + "TAIL";
        assert_eq!(s.graphemes(true).count(), THRESHOLD + 4);
        let (clip, _) = publish_text(&s);
        assert_eq!(clip.text.graphemes(true).count(), THRESHOLD);
        let expected_preview = "a".repeat(THRESHOLD - 1) + family;
        assert_eq!(
            clip.text, expected_preview,
            "preview must end after the emoji"
        );
        assert!(!clip.text.contains("TAIL"));
    }

    /// Combining mark: "e\u{301}" (e + COMBINING ACUTE ACCENT) is ONE grapheme
    /// (Swift `"e\u{301}".count == 1`); the preview must not orphan the mark.
    #[test]
    fn combining_marks_count_as_single_graphemes() {
        let e_acute = "e\u{301}";
        let s = e_acute.repeat(THRESHOLD + 1);
        let (clip, payload) = publish_text(&s);
        assert!(clip.has_data);
        assert_eq!(clip.size, Some((THRESHOLD + 1) as i64));
        assert_eq!(clip.text, e_acute.repeat(THRESHOLD));
        assert_eq!(
            payload.as_deref(),
            Some(s.as_bytes()),
            "payload is the full UTF-8 bytes"
        );
    }

    // ── sanitizedFilename (Clipboard.swift; checklist A4) ──────────────

    /// Behavior of `Clipboard.sanitizedFilename`: strip after the last `/`,
    /// then after the last `\`, trim, fall back to `"file"`.
    #[test]
    fn sanitized_filename_strips_path_components_and_falls_back() {
        assert_eq!(sanitized_filename("report.pdf"), "report.pdf");
        assert_eq!(sanitized_filename("/tmp/dir/report.pdf"), "report.pdf");
        assert_eq!(
            sanitized_filename("C:\\Users\\me\\report.pdf"),
            "report.pdf"
        );
        // Mixed separators: '/' is stripped first, then '\' (Swift order).
        assert_eq!(sanitized_filename("a/b\\c.txt"), "c.txt");
        assert_eq!(sanitized_filename("a\\b/c.txt"), "c.txt");
        assert_eq!(sanitized_filename("  spaced.txt  "), "spaced.txt");
        assert_eq!(sanitized_filename(""), "file");
        assert_eq!(sanitized_filename("   "), "file");
        assert_eq!(sanitized_filename("dir/"), "file");
        assert_eq!(sanitized_filename("dir\\"), "file");
    }

    /// Swift `lastIndex(of: "/")` is grapheme-based: a separator glued to a
    /// following combining mark (`"/\u{301}"` = ONE grapheme) is NOT a
    /// separator. A scalar `rfind('/')` would split there and diverge.
    #[test]
    fn sanitized_filename_ignores_separator_fused_with_combining_mark() {
        // No standalone "/" grapheme at all → name passes through whole.
        assert_eq!(sanitized_filename("a/\u{301}b"), "a/\u{301}b");
        assert_eq!(sanitized_filename("a\\\u{301}b"), "a\\\u{301}b");
        // The LAST standalone "/" wins; the fused one later is skipped.
        assert_eq!(sanitized_filename("dir/x/\u{301}y"), "x/\u{301}y");
    }

    // ── wire fixtures (FixturesTests.swift + docs/examples/*.json) ─────

    /// Ported from `FixturesTests.test_clipboardTextShort_decodes_and_roundTripsWithoutNullKeys`.
    /// Fixture JSON embedded verbatim from `docs/examples/clipboard_text_short.json`.
    #[test]
    fn fixture_text_short_decodes_and_round_trips_without_null_keys() {
        let data = r#"{
  "type": "Text",
  "hash": "3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F",
  "text": "Hello, SyncClipboard!",
  "hasData": false,
  "size": 21
}"#;
        let entry = decode(data);
        assert_eq!(entry.kind, ClipboardKind::Text);
        assert_eq!(
            entry.hash.as_deref(),
            Some("3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F")
        );
        assert_eq!(entry.text, "Hello, SyncClipboard!");
        assert_eq!(entry.size, Some(21));
        assert!(!entry.has_data);
        assert_eq!(entry.data_name, None);

        let re = encode(&entry);
        assert!(!re.contains("null"));
        assert_eq!(
            keys(&re),
            key_set(&["type", "hash", "text", "hasData", "size"])
        );
    }

    /// Ported from `FixturesTests.test_clipboardTextLong_hasDataTrueWithDataName`.
    /// Fixture JSON embedded verbatim from `docs/examples/clipboard_text_long.json`.
    #[test]
    fn fixture_text_long_has_data_true_with_data_name() {
        let data = r#"{
  "type": "Text",
  "hash": "B7E8C3D4F5A6071829304152637485A6B7C8D9E0F1A2B3C4D5E6F70819203142",
  "text": "<<PREVIEW: 10240 characters of the original text — first slice exactly 10240 chars>>",
  "hasData": true,
  "dataName": "text_B7E8C3D4F5A6071829304152637485A6B7C8D9E0F1A2B3C4D5E6F70819203142.txt",
  "size": 23457
}"#;
        let entry = decode(data);
        assert_eq!(entry.kind, ClipboardKind::Text);
        assert!(entry.has_data);
        assert_eq!(
            entry.data_name.as_deref(),
            Some("text_B7E8C3D4F5A6071829304152637485A6B7C8D9E0F1A2B3C4D5E6F70819203142.txt")
        );
        assert_eq!(entry.size, Some(23457));

        let re = encode(&entry);
        assert_eq!(
            keys(&re),
            key_set(&["type", "hash", "text", "hasData", "dataName", "size"])
        );
    }

    /// Ported from `FixturesTests.test_clipboardImage_decodesImageKind`.
    /// Fixture JSON embedded verbatim from `docs/examples/clipboard_image.json`.
    #[test]
    fn fixture_image_decodes_image_kind() {
        let data = r#"{
  "type": "Image",
  "hash": "4DD7CC4227AA3FB2FDAC2597CB4F88EAC6F69A10BC1994F6B87CF8890C345AFC",
  "text": "photo_2026.png",
  "hasData": true,
  "dataName": "photo_2026.png",
  "size": 184320
}"#;
        let entry = decode(data);
        assert_eq!(entry.kind, ClipboardKind::Image);
        assert_eq!(entry.data_name.as_deref(), Some("photo_2026.png"));
        assert_eq!(entry.size, Some(184320));
        assert!(entry.has_data);

        let re = encode(&entry);
        assert_eq!(
            keys(&re),
            key_set(&["type", "hash", "text", "hasData", "dataName", "size"])
        );
    }

    /// Ported from `FixturesTests.test_clipboardFile_decodesFileKind`.
    /// Fixture JSON embedded verbatim from `docs/examples/clipboard_file.json`.
    #[test]
    fn fixture_file_decodes_file_kind() {
        let data = r#"{
  "type": "File",
  "hash": "088EA33D054B64459EA2EB0CBD9F9152DD0BE4C38C6350963BBA00FDDC94CCEA",
  "text": "report.pdf",
  "hasData": true,
  "dataName": "report.pdf",
  "size": 1048576
}"#;
        let entry = decode(data);
        assert_eq!(entry.kind, ClipboardKind::File);
        assert_eq!(entry.data_name.as_deref(), Some("report.pdf"));

        let re = encode(&entry);
        assert_eq!(
            keys(&re),
            key_set(&["type", "hash", "text", "hasData", "dataName", "size"])
        );
    }

    /// Ported from `FixturesTests.test_clipboardGroup_decodesGroupKind`.
    /// Fixture JSON embedded verbatim from `docs/examples/clipboard_group.json`.
    #[test]
    fn fixture_group_decodes_group_kind() {
        let data = r#"{
  "type": "Group",
  "hash": "C9D8E7F605142332A1B0C9D8E7F60514233241506A7B8C9DAEBFC0D1E2F30415",
  "text": "screenshots.zip",
  "hasData": true,
  "dataName": "screenshots.zip",
  "size": 5242880
}"#;
        let entry = decode(data);
        assert_eq!(entry.kind, ClipboardKind::Group);
        assert_eq!(entry.data_name.as_deref(), Some("screenshots.zip"));

        let re = encode(&entry);
        assert_eq!(
            keys(&re),
            key_set(&["type", "hash", "text", "hasData", "dataName", "size"])
        );
    }

    /// Ported from `FixturesTests.test_clipboardNoHash_optionalKeysAreOmittedNotNullified`.
    /// Fixture JSON embedded verbatim from `docs/examples/clipboard_no_hash.json`.
    #[test]
    fn fixture_no_hash_optional_keys_are_omitted_not_nullified() {
        let data = r#"{
  "type": "Text",
  "text": "publisher omitted hash; receivers must treat as 'matches anything'",
  "hasData": false
}"#;
        let entry = decode(data);
        assert_eq!(entry.kind, ClipboardKind::Text);
        assert_eq!(entry.hash, None);
        assert_eq!(entry.data_name, None);
        assert_eq!(entry.size, None);
        assert!(!entry.has_data);

        let re = encode(&entry);
        assert!(
            !re.contains("null"),
            "optional fields must be omitted, not encoded as null"
        );
        assert_eq!(keys(&re), key_set(&["type", "text", "hasData"]));
    }

    /// Ported from `FixturesTests.test_clipboard_hashWhitespaceNormalizesToNil`.
    #[test]
    fn hash_whitespace_normalizes_to_none() {
        let json = r#"{"type":"Text","hash":"   ","text":"x","hasData":false}"#;
        let entry = decode(json);
        assert_eq!(entry.hash, None);
    }

    /// Ported from `FixturesTests.test_clipboard_kindRawValuesMatchWire`.
    #[test]
    fn kind_raw_values_match_wire() {
        let raw = |k: ClipboardKind| serde_json::to_string(&k).expect("kind encodes");
        assert_eq!(raw(ClipboardKind::Text), "\"Text\"");
        assert_eq!(raw(ClipboardKind::Image), "\"Image\"");
        assert_eq!(raw(ClipboardKind::File), "\"File\"");
        assert_eq!(raw(ClipboardKind::Group), "\"Group\"");
    }

    // ── Rust-only wire-shape guards ─────────────────────────────────────

    /// Swift's hand-written `encode(to:)` emits keys in declaration order
    /// (type, hash, text, hasData, dataName, size). serde must match — the
    /// struct's field declaration order IS the wire order.
    #[test]
    fn encode_emits_fields_in_swift_declaration_order() {
        let (clip, _) = publish_file("report.pdf", b"bytes");
        let json = encode(&clip);
        let pos = |k: &str| {
            json.find(&format!("\"{k}\""))
                .unwrap_or_else(|| panic!("missing key {k} in {json}"))
        };
        assert!(pos("type") < pos("hash"));
        assert!(pos("hash") < pos("text"));
        assert!(pos("text") < pos("hasData"));
        assert!(pos("hasData") < pos("dataName"));
        assert!(pos("dataName") < pos("size"));
    }

    /// Constructor parity with Swift `Clipboard.init`: a whitespace-only
    /// hash normalizes to `None` at construction time, not just on decode.
    #[test]
    fn new_normalizes_whitespace_hash_like_swift_init() {
        let c = Clipboard::new(
            ClipboardKind::Text,
            Some("   \n".to_string()),
            "x".to_string(),
            false,
            None,
            None,
        );
        assert_eq!(c.hash, None);
        let trimmed = Clipboard::new(
            ClipboardKind::Text,
            Some("  ABC  ".to_string()),
            "x".to_string(),
            false,
            None,
            None,
        );
        assert_eq!(trimmed.hash.as_deref(), Some("ABC"));
    }

    /// Explicit JSON `null` for optional keys decodes like an absent key
    /// (Swift `decodeIfPresent` returns nil for null).
    #[test]
    fn decode_tolerates_explicit_null_hash() {
        let json = r#"{"type":"Text","hash":null,"text":"x","hasData":false}"#;
        let entry = decode(json);
        assert_eq!(entry.hash, None);
    }

    /// `size` is Swift `Int?` (signed): a degenerate negative value from a
    /// buggy peer decodes instead of rejecting the whole document. A `u64`
    /// field would error out where Swift succeeds.
    #[test]
    fn decode_accepts_negative_size_like_swift_int() {
        let json = r#"{"type":"Text","text":"x","hasData":false,"size":-1}"#;
        let entry = decode(json);
        assert_eq!(entry.size, Some(-1));
    }
}
