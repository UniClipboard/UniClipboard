//! RFC 7578 `multipart/form-data` builder + history-query encoder for the
//! SyncClipboard mobile-sync protocol (spec 2.7 `POST /api/history/query`
//! and spec 2.9 `POST /api/history`).
//!
//! Byte-for-byte port of the iOS Swift implementation
//! (`Shared/Network/MultipartBody.swift` + `Shared/Network/HistoryQuery.swift`,
//! checklist A5). The Swift sources and their tests are the NORMATIVE
//! reference; the golden vectors below are copied verbatim from
//! `MultipartBodyTests.swift` and `QueryHistoryTests.swift`.
//!
//! # BYTE-CRITICAL INVARIANTS (regression checklist A5)
//!
//! - **Every line terminator is CRLF (`0x0D 0x0A`) — never a bare LF.**
//!   Part delimiter is `--{boundary}` + CRLF, each header line ends with
//!   CRLF, the empty header/body separator line is a lone CRLF, the part
//!   body is followed by CRLF, and the closing delimiter is
//!   `--{boundary}--` + CRLF (note the trailing CRLF after the final
//!   boundary — `MultipartBody.swift` `encoded()` appends it via
//!   `crlf(prefix:)`).
//! - **Quoted-string escaping (RFC 7578 4.2, `MultipartBody.swift`
//!   `quoted(_:)`)**: backslash → double-backslash FIRST, then
//!   double-quote → backslash-quote, then CR and LF are DROPPED entirely
//!   (not escaped). The order matters: escaping the quote first would
//!   double-escape its backslash.
//! - **Deterministic boundary**: the boundary is always a caller-supplied
//!   input. The Swift default (`"UCB-\(UUID().uuidString)"`) is a
//!   convenience for production call sites; this pure crate never
//!   generates randomness.
//! - **Field omission is meaningful**: a `None` query field is NOT sent at
//!   all, while an explicitly appended empty string still produces a
//!   zero-byte part. The server distinguishes "absent" from "empty"
//!   (`HistoryQuery.swift` doc comment).
//! - **Dates** are ISO-8601 UTC with exactly three fractional digits and a
//!   `Z` suffix (Swift `ISO8601DateFormatter` with `.withInternetDateTime`
//!   + `.withFractionalSeconds`), e.g. `2026-05-17T16:43:21.420Z`.
//! - **Type bitmask** (spec 2.7): Text=1, Image=2, File=4, Group=8,
//!   all=15. Encoded as a decimal string.

use chrono::{DateTime, Utc};

// ─── multipart builder ──────────────────────────────────────────────────

/// CRLF line terminator — the only line break this module ever emits.
/// BYTE-CRITICAL: `0x0D 0x0A`, never a bare `\n` (`MultipartBody.swift`
/// `Self.crlf`).
const CRLF: &[u8] = b"\r\n";

/// Minimal `multipart/form-data` builder (RFC 7578). Port of
/// `Shared/Network/MultipartBody.swift`.
///
/// Pure byte assembly; no I/O, no randomness. The boundary is an input so
/// that output is fully deterministic and testable byte-for-byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipartBody {
    boundary: String,
    /// Concatenated encoded parts (each already CRLF-terminated), matching
    /// the Swift `parts: [Data]` accumulation.
    parts: Vec<u8>,
}

impl MultipartBody {
    /// Create an empty body with the given boundary.
    ///
    /// Unlike the Swift initializer there is no random default boundary —
    /// determinism is a hard requirement of this crate.
    pub fn new(boundary: impl Into<String>) -> Self {
        Self {
            boundary: boundary.into(),
            parts: Vec::new(),
        }
    }

    /// The boundary this body was created with.
    pub fn boundary(&self) -> &str {
        &self.boundary
    }

    /// `Content-Type` header value announcing the boundary
    /// (`MultipartBody.swift` `contentType`).
    ///
    /// BYTE-CRITICAL: exact spelling/spacing
    /// `multipart/form-data; boundary={boundary}` (no quotes around the
    /// boundary, single space after the semicolon).
    pub fn content_type(&self) -> String {
        format!("multipart/form-data; boundary={}", self.boundary)
    }

    /// Append a text field (`MultipartBody.swift` `append(name:value:)`).
    ///
    /// An empty `value` still emits a zero-byte part — the server
    /// distinguishes "field present but empty" from "field absent", so
    /// callers MUST omit by not calling rather than passing `""`.
    ///
    /// Emitted bytes (every line CRLF-terminated):
    /// ```text
    /// --{boundary}
    /// Content-Disposition: form-data; name="{quoted name}"
    ///
    /// {value}
    /// ```
    pub fn append_text(&mut self, name: &str, value: &str) {
        self.push_line(format!("--{}", self.boundary).as_bytes());
        self.push_line(
            format!("Content-Disposition: form-data; name=\"{}\"", quoted(name)).as_bytes(),
        );
        // Header/body separator: a lone CRLF.
        self.parts.extend_from_slice(CRLF);
        self.parts.extend_from_slice(value.as_bytes());
        self.parts.extend_from_slice(CRLF);
    }

    /// Append a binary field with filename and `Content-Type`
    /// (`MultipartBody.swift` `append(name:filename:contentType:body:)`).
    /// Used by spec 2.9 to attach payload bytes alongside metadata fields.
    ///
    /// Emitted bytes (every line CRLF-terminated):
    /// ```text
    /// --{boundary}
    /// Content-Disposition: form-data; name="{quoted name}"; filename="{quoted filename}"
    /// Content-Type: {content_type}
    ///
    /// {body bytes}
    /// ```
    pub fn append_file(&mut self, name: &str, filename: &str, content_type: &str, body: &[u8]) {
        self.push_line(format!("--{}", self.boundary).as_bytes());
        self.push_line(
            format!(
                "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"",
                quoted(name),
                quoted(filename)
            )
            .as_bytes(),
        );
        self.push_line(format!("Content-Type: {content_type}").as_bytes());
        // Header/body separator: a lone CRLF.
        self.parts.extend_from_slice(CRLF);
        self.parts.extend_from_slice(body);
        self.parts.extend_from_slice(CRLF);
    }

    /// Finalize the body (`MultipartBody.swift` `encoded()`).
    ///
    /// A multipart with zero fields is legal — the closing delimiter alone
    /// is a valid (if useless) body.
    ///
    /// BYTE-CRITICAL: the closing delimiter is `--{boundary}--` FOLLOWED BY
    /// a trailing CRLF (`crlf(prefix: "--\(boundary)--")` in Swift).
    pub fn encoded(&self) -> Vec<u8> {
        let mut data = self.parts.clone();
        data.extend_from_slice(format!("--{}--", self.boundary).as_bytes());
        data.extend_from_slice(CRLF);
        data
    }

    /// Append `line` bytes followed by CRLF (`MultipartBody.swift`
    /// `crlf(prefix:)`).
    fn push_line(&mut self, line: &[u8]) {
        self.parts.extend_from_slice(line);
        self.parts.extend_from_slice(CRLF);
    }
}

/// Escape a `Content-Disposition` `name=`/`filename=` parameter per
/// RFC 7578 4.2 (`MultipartBody.swift` `quoted(_:)`).
///
/// BYTE-CRITICAL ordering: backslash is escaped FIRST (`\` → `\\`), then
/// double-quote (`"` → `\"`), then CR and LF are DROPPED (removed, not
/// escaped). Matches the exact `replacingOccurrences` chain in Swift; the
/// final two sequential removals are collapsed into one char-set `replace`
/// (byte-equivalent, keeps clippy's `collapsible_str_replace` happy).
fn quoted(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\r', '\n'], "")
}

// ─── history query (spec 2.7) ───────────────────────────────────────────

/// Type bitmask for [`HistoryQuery::types`] (spec 2.7; Swift
/// `HistoryQuery.TypeMask`): Text=1, Image=2, File=4, Group=8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum TypeMask {
    /// Text record bit.
    Text = 1,
    /// Image record bit.
    Image = 2,
    /// File record bit.
    File = 4,
    /// Group record bit.
    Group = 8,
}

impl TypeMask {
    /// All four bits set (Swift `TypeMask.all`).
    pub const ALL: i64 = 15;

    /// This variant's bit value, for OR-combining into
    /// [`HistoryQuery::types`].
    pub const fn bit(self) -> i64 {
        self as i64
    }
}

/// Filter parameters for `POST /api/history/query` (spec 2.7). Port of
/// `Shared/Network/HistoryQuery.swift`.
///
/// All fields optional. The encoder emits ONLY the fields that are `Some`
/// — missing fields default to "no filter" on the server side, so omission
/// is meaningful (never send `page=""` etc.).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoryQuery {
    /// 1-indexed page, encoded as a decimal string. Omit to fetch from the
    /// start. An empty result page signals end-of-list.
    pub page: Option<i64>,
    /// Strict upper bound on `createTime`: records with
    /// `createTime < before` are kept.
    pub before: Option<DateTime<Utc>>,
    /// Inclusive lower bound on `createTime`: records with
    /// `createTime >= after` are kept.
    pub after: Option<DateTime<Utc>>,
    /// STRICT lower bound on `lastModified` — the incremental-sync
    /// primitive. Pass the highest `lastModified` seen across the prior
    /// merged pages to fetch only what changed since.
    pub modified_after: Option<DateTime<Utc>>,
    /// Bitmask: Text=1, Image=2, File=4, Group=8 (see [`TypeMask`]). Use
    /// `15` for "all" or `12` for "files + groups". `None` = no type
    /// filter. Encoded as a decimal string.
    pub types: Option<i64>,
    /// Server-side substring match against the record's `text`.
    pub search_text: Option<String>,
    /// Encoded as the literal strings `"true"` / `"false"`.
    pub starred: Option<bool>,
    /// When `true`, server sorts by `lastAccessed` desc instead of the
    /// default `createTime` desc. Encoded as `"true"` / `"false"`.
    pub sort_by_last_accessed: Option<bool>,
}

impl HistoryQuery {
    /// Build the multipart body for this query
    /// (`HistoryQuery.swift` `multipartEncoded(boundary:)`).
    ///
    /// BYTE-CRITICAL field append order (exactly as in Swift): `page`,
    /// `before`, `after`, `modifiedAfter`, `types`, `searchText`,
    /// `starred`, `sortByLastAccessed`. `None` fields are skipped
    /// entirely.
    ///
    /// The boundary is required here (no random default) so the encoding
    /// is deterministic.
    pub fn multipart_encoded(&self, boundary: &str) -> MultipartBody {
        let mut body = MultipartBody::new(boundary);
        if let Some(page) = self.page {
            body.append_text("page", &page.to_string());
        }
        if let Some(before) = self.before {
            body.append_text("before", &iso8601_millis(before));
        }
        if let Some(after) = self.after {
            body.append_text("after", &iso8601_millis(after));
        }
        if let Some(modified_after) = self.modified_after {
            body.append_text("modifiedAfter", &iso8601_millis(modified_after));
        }
        if let Some(types) = self.types {
            body.append_text("types", &types.to_string());
        }
        if let Some(search_text) = &self.search_text {
            body.append_text("searchText", search_text);
        }
        if let Some(starred) = self.starred {
            body.append_text("starred", if starred { "true" } else { "false" });
        }
        if let Some(sort) = self.sort_by_last_accessed {
            body.append_text("sortByLastAccessed", if sort { "true" } else { "false" });
        }
        body
    }

    /// Convenience: encode straight to the final multipart bytes.
    pub fn multipart_bytes(&self, boundary: &str) -> Vec<u8> {
        self.multipart_encoded(boundary).encoded()
    }
}

/// ISO-8601 UTC with EXACTLY three fractional digits and a `Z` suffix,
/// matching Swift's `ISO8601DateFormatter` with `.withInternetDateTime` +
/// `.withFractionalSeconds` (`HistoryQuery.swift` `isoFormatter`), e.g.
/// `2026-05-17T16:43:21.420Z`.
///
/// Single source of truth for this wire format is
/// [`crate::history_record::format_iso8601_utc`] (spec §3.6); this thin
/// alias keeps the call sites readable.
fn iso8601_millis(date: DateTime<Utc>) -> String {
    crate::history_record::format_iso8601_utc(&date)
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse an RFC 3339 timestamp into `DateTime<Utc>` for test fixtures.
    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s)
            .expect("test fixture timestamp must parse")
            .with_timezone(&Utc)
    }

    fn encoded_string(body: &MultipartBody) -> String {
        String::from_utf8(body.encoded()).expect("encoded body must be valid UTF-8")
    }

    /// CRLF discipline: every 0x0A in `bytes` must be immediately preceded
    /// by 0x0D (no bare LF anywhere). Raw-byte assertion on purpose.
    fn assert_no_bare_lf(bytes: &[u8]) {
        for (i, b) in bytes.iter().enumerate() {
            if *b == 0x0A {
                assert!(
                    i > 0 && bytes[i - 1] == 0x0D,
                    "bare LF at offset {i}: every line terminator must be CRLF"
                );
            }
        }
    }

    // ── MultipartBody: shape / contentType ─────────────────────────────

    // Swift: MultipartBodyTests.test_contentType_includesBoundary
    #[test]
    fn content_type_includes_boundary() {
        let body = MultipartBody::new("TESTBND");
        assert_eq!(body.content_type(), "multipart/form-data; boundary=TESTBND");
    }

    // Swift: MultipartBodyTests.test_emptyBody_isOnlyTheClosingDelimiter
    #[test]
    fn empty_body_is_only_the_closing_delimiter() {
        let body = MultipartBody::new("TESTBND");
        assert_eq!(body.encoded(), b"--TESTBND--\r\n");
    }

    // ── MultipartBody: text fields ─────────────────────────────────────

    // Swift: MultipartBodyTests.test_singleTextField_encodesWithCRLFLineEndings
    #[test]
    fn single_text_field_encodes_with_crlf_line_endings() {
        let mut body = MultipartBody::new("BND");
        body.append_text("page", "1");

        let want: &[u8] = b"--BND\r\n\
            Content-Disposition: form-data; name=\"page\"\r\n\
            \r\n\
            1\r\n\
            --BND--\r\n";
        // Raw byte equality — asserting on bytes, not a lossy string view.
        assert_eq!(body.encoded(), want);
        assert_no_bare_lf(&body.encoded());
    }

    // Swift: MultipartBodyTests.test_multipleTextFields_areOrderedAsAppended
    #[test]
    fn multiple_text_fields_are_ordered_as_appended() {
        let mut body = MultipartBody::new("B");
        body.append_text("page", "1");
        body.append_text("types", "15");
        body.append_text("modifiedAfter", "2026-05-17T00:00:00Z");

        let encoded = encoded_string(&body);
        // Ordering: page comes before types comes before modifiedAfter.
        let page_idx = encoded.find("name=\"page\"").expect("page present");
        let types_idx = encoded.find("name=\"types\"").expect("types present");
        let mod_after_idx = encoded
            .find("name=\"modifiedAfter\"")
            .expect("modifiedAfter present");
        assert!(page_idx < types_idx);
        assert!(types_idx < mod_after_idx);
    }

    // Swift: MultipartBodyTests.test_unicodeValuesArePassedThroughAsUTF8
    #[test]
    fn unicode_values_are_passed_through_as_utf8() {
        let mut body = MultipartBody::new("B");
        body.append_text("searchText", "搜索 — emoji ✨");

        let raw = body.encoded();
        let as_string = String::from_utf8(raw.clone()).expect("utf8");
        assert!(as_string.contains("搜索 — emoji ✨"));
        // Spot-check the UTF-8 bytes (e.g., "搜" is E6 90 9C in UTF-8).
        let needle: &[u8] = &[0xE6, 0x90, 0x9C];
        assert!(
            raw.windows(needle.len()).any(|w| w == needle),
            "UTF-8 bytes of '搜' must appear verbatim"
        );
    }

    // ── MultipartBody: file fields ─────────────────────────────────────

    // Swift: MultipartBodyTests.test_fileField_emitsFilenameContentTypeAndBody
    #[test]
    fn file_field_emits_filename_content_type_and_body() {
        let mut body = MultipartBody::new("B");
        body.append_file(
            "file",
            "snap.png",
            "application/octet-stream",
            &[0x00, 0xFF, 0x42],
        );

        let raw = body.encoded();
        // Header substring checks on the raw bytes (the body contains 0xFF,
        // so the buffer is not valid UTF-8 as a whole).
        let contains = |needle: &[u8]| raw.windows(needle.len()).any(|w| w == needle);
        assert!(contains(
            b"Content-Disposition: form-data; name=\"file\"; filename=\"snap.png\"\r\n"
        ));
        assert!(contains(b"Content-Type: application/octet-stream\r\n"));
        // Raw bytes survive intact — the binary part sits immediately
        // before the trailing CRLF + closing boundary.
        let closer: &[u8] = b"\r\n--B--\r\n";
        let closer_pos = raw
            .windows(closer.len())
            .position(|w| w == closer)
            .expect("closer not found in encoded body");
        assert_eq!(&raw[closer_pos - 3..closer_pos], &[0x00, 0xFF, 0x42]);
    }

    // Rust-side hardening (stronger than the Swift substring checks):
    // full byte-exact vector for a binary part, including the header/body
    // separator and the trailing CRLF before the closing delimiter.
    #[test]
    fn file_field_full_byte_vector() {
        let mut body = MultipartBody::new("B");
        body.append_file(
            "file",
            "snap.png",
            "application/octet-stream",
            &[0x00, 0xFF, 0x42],
        );

        let mut want: Vec<u8> = Vec::new();
        want.extend_from_slice(b"--B\r\n");
        want.extend_from_slice(
            b"Content-Disposition: form-data; name=\"file\"; filename=\"snap.png\"\r\n",
        );
        want.extend_from_slice(b"Content-Type: application/octet-stream\r\n");
        want.extend_from_slice(b"\r\n");
        want.extend_from_slice(&[0x00, 0xFF, 0x42]);
        want.extend_from_slice(b"\r\n");
        want.extend_from_slice(b"--B--\r\n");
        assert_eq!(body.encoded(), want);
    }

    // ── MultipartBody: escaping ────────────────────────────────────────

    // Swift: MultipartBodyTests.test_disposition_escapesQuotesInFieldNamesAndFilenames
    #[test]
    fn disposition_escapes_quotes_in_field_names_and_filenames() {
        let mut body = MultipartBody::new("B");
        body.append_file("weird\"name", "a\"b.txt", "text/plain", &[]);

        let head = encoded_string(&body);
        assert!(head.contains(r#"name="weird\"name""#));
        assert!(head.contains(r#"filename="a\"b.txt""#));
    }

    // Swift: MultipartBodyTests.test_disposition_stripsEmbeddedNewlines
    #[test]
    fn disposition_strips_embedded_newlines() {
        let mut body = MultipartBody::new("B");
        body.append_text("bad\r\nfield", "x");

        let head = encoded_string(&body);
        assert!(
            head.contains(r#"name="badfield""#),
            "CR/LF in field name MUST be stripped, not preserved"
        );
    }

    // Rust-side hardening: backslash escaping and its ordering relative to
    // quote escaping (RFC 7578 4.2; MultipartBody.swift quoted(_:)).
    #[test]
    fn disposition_escapes_backslash_before_quote() {
        let mut body = MultipartBody::new("B");
        // Lone backslash doubles.
        body.append_text("a\\b", "1");
        // Backslash-quote: `\` doubles first, THEN `"` gains its own
        // backslash → `a\"b` becomes `a\\\"b` (not `a\\\\"b` or `a\"b`).
        body.append_text("a\\\"b", "2");

        let head = encoded_string(&body);
        assert!(head.contains("name=\"a\\\\b\""), "got: {head}");
        assert!(head.contains("name=\"a\\\\\\\"b\""), "got: {head}");
    }

    // Rust-side hardening: lone CR and lone LF are each dropped (the Swift
    // test only covers the combined "\r\n" sequence).
    #[test]
    fn disposition_drops_lone_cr_and_lone_lf() {
        let mut body = MultipartBody::new("B");
        body.append_text("a\rb", "1");
        body.append_text("c\nd", "2");

        let head = encoded_string(&body);
        assert!(head.contains(r#"name="ab""#));
        assert!(head.contains(r#"name="cd""#));
    }

    // Rust-side hardening: an explicitly appended empty value still emits a
    // zero-byte part (MultipartBody.swift append(name:value:) doc comment —
    // "present but empty" differs from "absent").
    #[test]
    fn empty_string_value_still_emits_a_part() {
        let mut body = MultipartBody::new("B");
        body.append_text("x", "");
        let want: &[u8] = b"--B\r\n\
            Content-Disposition: form-data; name=\"x\"\r\n\
            \r\n\
            \r\n\
            --B--\r\n";
        assert_eq!(body.encoded(), want);
    }

    // ── MultipartBody: closing ─────────────────────────────────────────

    // Swift: MultipartBodyTests.test_encoded_endsExactlyOnClosingBoundary
    #[test]
    fn encoded_ends_exactly_on_closing_boundary() {
        let mut body = MultipartBody::new("Z");
        body.append_text("a", "1");
        body.append_text("b", "2");

        let encoded = body.encoded();
        assert!(
            encoded.ends_with(b"--Z--\r\n"),
            "final delimiter must be `--<boundary>--\\r\\n`"
        );
        assert_no_bare_lf(&encoded);
    }

    // ── HistoryQuery encoding ──────────────────────────────────────────

    // Swift: QueryHistoryTests.test_historyQuery_emptyEncodesToClosingDelimiterOnly
    #[test]
    fn history_query_empty_encodes_to_closing_delimiter_only() {
        let q = HistoryQuery::default();
        // No fields → multipart body is just the trailing boundary.
        assert_eq!(q.multipart_bytes("BND"), b"--BND--\r\n");
    }

    // Swift: QueryHistoryTests.test_historyQuery_pageOnlyEncodesAsIntString
    #[test]
    fn history_query_page_only_encodes_as_int_string() {
        let q = HistoryQuery {
            page: Some(3),
            ..Default::default()
        };
        let encoded = encoded_string(&q.multipart_encoded("BND"));
        assert!(encoded.contains("name=\"page\"\r\n\r\n3\r\n"));
    }

    // Swift: QueryHistoryTests.test_historyQuery_typesEncodesBitmaskAsString
    #[test]
    fn history_query_types_encodes_bitmask_as_string() {
        let q = HistoryQuery {
            types: Some(TypeMask::ALL),
            ..Default::default()
        };
        let encoded = encoded_string(&q.multipart_encoded("BND"));
        assert!(encoded.contains("name=\"types\"\r\n\r\n15\r\n"));
    }

    // Swift: QueryHistoryTests.test_historyQuery_starredEncodesAsTrueFalseString
    #[test]
    fn history_query_starred_encodes_as_true_false_string() {
        let true_query = HistoryQuery {
            starred: Some(true),
            ..Default::default()
        };
        let false_query = HistoryQuery {
            starred: Some(false),
            ..Default::default()
        };
        let true_encoded = encoded_string(&true_query.multipart_encoded("B"));
        let false_encoded = encoded_string(&false_query.multipart_encoded("B"));
        assert!(true_encoded.contains("name=\"starred\"\r\n\r\ntrue\r\n"));
        assert!(false_encoded.contains("name=\"starred\"\r\n\r\nfalse\r\n"));
    }

    // Swift: QueryHistoryTests.test_historyQuery_modifiedAfterEncodesAsFractionalISO
    #[test]
    fn history_query_modified_after_encodes_as_fractional_iso() {
        let q = HistoryQuery {
            modified_after: Some(utc("2026-05-17T16:43:21.420Z")),
            ..Default::default()
        };
        let encoded = encoded_string(&q.multipart_encoded("B"));
        assert!(
            encoded.contains("name=\"modifiedAfter\"\r\n\r\n2026-05-17T16:43:21.420Z\r\n"),
            "modifiedAfter must be ISO-8601 with fractional seconds and Z suffix.\nGot: {encoded}"
        );
    }

    // Swift: QueryHistoryTests.test_historyQuery_omitsNilFields
    #[test]
    fn history_query_omits_nil_fields() {
        let q = HistoryQuery {
            page: Some(1),
            ..Default::default()
        };
        let encoded = encoded_string(&q.multipart_encoded("B"));
        assert!(!encoded.contains("name=\"modifiedAfter\""));
        assert!(!encoded.contains("name=\"types\""));
        assert!(!encoded.contains("name=\"starred\""));
        assert!(!encoded.contains("name=\"searchText\""));
    }

    // Swift: QueryHistoryTests.test_queryHistory_bodyContainsRequestedFilters
    // (the multipart-body assertions; the HTTP plumbing around them is
    // network code and out of scope for this pure crate).
    #[test]
    fn history_query_body_contains_requested_filters() {
        let q = HistoryQuery {
            page: Some(2),
            modified_after: Some(utc("2026-05-17T16:43:21.420Z")),
            types: Some(TypeMask::Text.bit() | TypeMask::Image.bit()),
            starred: Some(true),
            ..Default::default()
        };
        let body = encoded_string(&q.multipart_encoded("B"));
        assert!(body.contains("name=\"page\"\r\n\r\n2\r\n"));
        assert!(body.contains("name=\"modifiedAfter\"\r\n\r\n2026-05-17T16:43:21.420Z\r\n"));
        assert!(body.contains("name=\"types\"\r\n\r\n3\r\n")); // 1 | 2 = 3
        assert!(body.contains("name=\"starred\"\r\n\r\ntrue\r\n"));
    }

    // Rust-side hardening: TypeMask bit values are locked (checklist A5).
    #[test]
    fn type_mask_bits_match_spec() {
        assert_eq!(TypeMask::Text.bit(), 1);
        assert_eq!(TypeMask::Image.bit(), 2);
        assert_eq!(TypeMask::File.bit(), 4);
        assert_eq!(TypeMask::Group.bit(), 8);
        assert_eq!(TypeMask::ALL, 15);
    }

    // Rust-side hardening: a whole-second date still carries EXACTLY three
    // fractional digits (Swift ISO8601DateFormatter with
    // .withFractionalSeconds always emits .000).
    #[test]
    fn whole_second_date_encodes_with_000_millis() {
        let q = HistoryQuery {
            before: Some(utc("2026-05-17T00:00:00Z")),
            ..Default::default()
        };
        let encoded = encoded_string(&q.multipart_encoded("B"));
        assert!(
            encoded.contains("name=\"before\"\r\n\r\n2026-05-17T00:00:00.000Z\r\n"),
            "got: {encoded}"
        );
    }

    // Rust-side hardening: full byte-exact vector with every field set, in
    // the Swift append order page → before → after → modifiedAfter → types
    // → searchText → starred → sortByLastAccessed.
    #[test]
    fn history_query_all_fields_full_byte_vector() {
        let q = HistoryQuery {
            page: Some(2),
            before: Some(utc("2026-05-18T00:00:00.000Z")),
            after: Some(utc("2026-05-16T12:30:00.500Z")),
            modified_after: Some(utc("2026-05-17T16:43:21.420Z")),
            types: Some(TypeMask::ALL),
            search_text: Some("搜索".to_string()),
            starred: Some(false),
            sort_by_last_accessed: Some(true),
        };
        let want = "--B\r\nContent-Disposition: form-data; name=\"page\"\r\n\r\n2\r\n\
            --B\r\nContent-Disposition: form-data; name=\"before\"\r\n\r\n2026-05-18T00:00:00.000Z\r\n\
            --B\r\nContent-Disposition: form-data; name=\"after\"\r\n\r\n2026-05-16T12:30:00.500Z\r\n\
            --B\r\nContent-Disposition: form-data; name=\"modifiedAfter\"\r\n\r\n2026-05-17T16:43:21.420Z\r\n\
            --B\r\nContent-Disposition: form-data; name=\"types\"\r\n\r\n15\r\n\
            --B\r\nContent-Disposition: form-data; name=\"searchText\"\r\n\r\n搜索\r\n\
            --B\r\nContent-Disposition: form-data; name=\"starred\"\r\n\r\nfalse\r\n\
            --B\r\nContent-Disposition: form-data; name=\"sortByLastAccessed\"\r\n\r\ntrue\r\n\
            --B--\r\n";
        let got = q.multipart_bytes("B");
        assert_eq!(got, want.as_bytes());
        assert_no_bare_lf(&got);
    }
}
