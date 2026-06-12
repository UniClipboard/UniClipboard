//! `history_record` — SyncClipboard on-the-wire history record model.
//!
//! Port of `Shared/Models/HistoryRecord.swift` (uc-ios), spec
//! `docs/SYNC_PROTOCOL.md` §3.6. The Swift implementation and its tests
//! (`FixturesTests.swift`) are the NORMATIVE source — every encode/decode
//! rule here matches Swift byte for byte.
//!
//! Returned by spec §2.7 (`POST /api/history/query`), §2.8
//! (`GET /api/history/<id>`), and the §2.10 PATCH reply. Accepted by §2.9 in
//! **multipart form**, not JSON — do not reuse [`HistoryRecord`]'s `Serialize`
//! for upload payloads; the multipart shape is built field-by-field by the
//! network layer (see `multipart.rs`).
//!
//! ## BYTE-CRITICAL invariants (the cross-client contract)
//!
//! 1. **Composite vs split id — two DIFFERENT formats.**
//!    - Composite `profileId` = `"<type>-<hash>"` (dash) — URL paths of
//!      §2.8 / §2.11. See [`composite_profile_id`].
//!    - Split PATCH id = `"<type>/<hash>"` (slash, two path segments) — URL
//!      path of §2.10 only. See [`split_patch_id`]. Never interchange them.
//! 2. **`isDeleted` vs `isDelete`.** Read/create shapes use `isDeleted`
//!    (spec §3.6 / §2.9); the §2.10 PATCH JSON body uses `isDelete` (NO
//!    trailing `d`). A misspelled field is *silently ignored* by servers.
//!    The raw string `"isDelete"` appears exactly once in this crate's
//!    production code — on [`HistoryRecordPatch::is_delete`]'s serde rename
//!    (golden tests necessarily repeat it in expected-JSON literals).
//! 3. **Unconditional flags.** `hasData` / `starred` / `pinned` / `isDeleted`
//!    are always encoded, even when `false` (Swift `encode`, matching the
//!    Android client). `text` / `size` / `version` / timestamps are encoded
//!    only when present (`encodeIfPresent`) — note: a present-but-empty
//!    `text` IS encoded; only `nil`/`None` is omitted.
//! 4. **ISO-8601 timestamps.** Decoding accepts fractional and plain
//!    seconds, with `Z` or `±hh:mm` offsets (all four combos). Encoding
//!    always emits exactly **3 fractional digits** (milliseconds) and a `Z`
//!    suffix, normalized to UTC — the Swift `ISO8601DateFormatter` with
//!    `.withFractionalSeconds` shape, e.g. `2026-05-17T16:43:21.420Z`.
//!
//! ## Version lifecycle (spec §3.6 "Lifecycle and version")
//!
//! - A record is created via §2.9 with `version: 0` ([`INITIAL_VERSION`]).
//! - Every successful §2.10 PATCH increments the server's stored version by
//!   1 and refreshes `lastModified`. The client MUST send the version it
//!   observed (its rebase point) in [`HistoryRecordPatch::version`].
//! - A stale version is rejected by the server with HTTP `409`; the response
//!   body is the server's current record so the client can rebase and retry.
//!   No HTTP lives here — this module only defines the wire shapes.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

// ─── kind ───────────────────────────────────────────────────────────────

/// Wire enum for a record's content kind (`Clipboard.Kind` in Swift).
///
/// Canonical definition lives in [`crate::clipboard_doc`]; re-exported here
/// because every history id/record is keyed by the same wire `type` values.
pub use crate::clipboard_doc::ClipboardKind;

// ─── version lifecycle ──────────────────────────────────────────────────

/// New records are created via §2.9 with `version: 0` (spec §3.6).
pub const INITIAL_VERSION: i64 = 0;

// ─── id helpers ─────────────────────────────────────────────────────────

/// Composite `profileId` = `"<type>-<hash>"` — addresses a record in the
/// URL paths of spec §2.8 and §2.11 (`GET /api/history/<profileId>/data`).
///
/// Port of Swift `HistoryRecord.profileId(type:hash:)` (HistoryRecord.swift).
/// BYTE-CRITICAL: plain `-` join of the kind raw value and the hash. Hashes
/// are uppercase hex by protocol (§4 — SHA-256 uppercase); like Swift, this
/// helper does NOT re-case its input, it interpolates the hash verbatim.
///
/// NOT the §2.10 PATCH id — that one is slash-split, see [`split_patch_id`].
pub fn composite_profile_id(kind: ClipboardKind, hash: &str) -> String {
    format!("{}-{hash}", kind.as_wire_str())
}

/// Split PATCH id = `"<type>/<hash>"` — the URL path *suffix* of spec §2.10
/// (`PATCH /api/history/<type>/<hash>`): kind and bare hash as two separate
/// path segments.
///
/// BYTE-CRITICAL: this is a DIFFERENT format from [`composite_profile_id`]
/// (slash, not dash; the hash carries no `<type>-` prefix). Mixing them up
/// produces 404s. Spec §2.8 note: "§2.10 (PATCH) uses a *split form* with
/// type as its own path segment."
pub fn split_patch_id(kind: ClipboardKind, hash: &str) -> String {
    format!("{}/{hash}", kind.as_wire_str())
}

// ─── ISO-8601 date helpers ──────────────────────────────────────────────

/// [`parse_iso8601_utc`] failure: the input is not an ISO-8601 timestamp in
/// the strict profile Swift's `ISO8601DateFormatter` accepts. The message
/// mirrors Swift `decodeISODate`'s `debugDescription`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Not a recognized ISO-8601 timestamp: {raw}")]
pub struct IsoTimestampError {
    /// The rejected input, verbatim.
    pub raw: String,
}

/// Tolerant ISO-8601 parse, normalized to UTC.
///
/// Port of Swift `HistoryRecord.decodeISODate` (HistoryRecord.swift):
/// accepts both `…Z` and `…+00:00` (any `±hh:mm` offset) flavors, with
/// fractional seconds optional — all four combos. The Android wire uses
/// fractional-seconds + `Z` (`2026-05-17T16:43:21.420Z`); some hand-rolled
/// servers truncate. Both parse here. Non-UTC offsets are converted to the
/// equivalent UTC instant (Swift `Date` is offset-agnostic too).
///
/// Swift's `ISO8601DateFormatter` is stricter than chrono's RFC-3339
/// parser: it requires an UPPERCASE `T` separator and `Z` suffix and
/// rejects a space separator. The guard below replicates that (in a valid
/// RFC-3339 string, `t`/`z`/space can only ever be those separators, so a
/// containment check is exact).
pub fn parse_iso8601_utc(raw: &str) -> Result<DateTime<Utc>, IsoTimestampError> {
    let reject = || IsoTimestampError {
        raw: raw.to_owned(),
    };
    if raw.bytes().any(|b| matches!(b, b't' | b'z' | b' ')) {
        return Err(reject());
    }
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| reject())
}

/// Emit the exact Swift `ISO8601DateFormatter` + `.withFractionalSeconds`
/// shape: BYTE-CRITICAL `yyyy-MM-ddTHH:mm:ss.SSSZ` — exactly **3**
/// fractional digits (milliseconds, zero-padded, e.g. `.000`), `Z` suffix,
/// always UTC.
pub fn format_iso8601_utc(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Decode-side optional timestamp rule (Swift `decodeISODate`):
/// - field absent or JSON `null` → `None` (handled by the caller's `Option`)
/// - empty / whitespace-only string → `None` (matches `Clipboard.hash`
///   normalization)
/// - otherwise the RAW string (not trimmed!) must parse, else error.
fn parse_optional_iso8601(raw: Option<&str>) -> Result<Option<DateTime<Utc>>, IsoTimestampError> {
    let Some(raw) = raw else { return Ok(None) };
    if raw.trim().is_empty() {
        return Ok(None);
    }
    parse_iso8601_utc(raw).map(Some)
}

// ─── wire model ─────────────────────────────────────────────────────────

/// On-the-wire history record (spec §3.6). Port of `HistoryRecord.swift`.
///
/// Decoders MUST tolerate missing/unknown fields: only `hash` and `type` are
/// required; everything else has a sensible default (timestamps unknown,
/// flags `false`). See the module docs for the byte-critical encode rules.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HistoryRecord {
    /// SHA-256 uppercase hex of the content (spec §4).
    pub hash: String,
    /// Wire field name `type` (Swift `type: Clipboard.Kind`).
    pub kind: ClipboardKind,
    /// Optional preview text. Encoded whenever `Some`, even if empty;
    /// omitted only when `None` (Swift `encodeIfPresent`).
    pub text: Option<String>,
    /// `true` ⇔ a payload file is downloadable via §2.11. Always encoded.
    pub has_data: bool,
    /// Same semantics as `Clipboard.size` (spec §3.2).
    pub size: Option<i64>,
    /// Wire `createTime`, ISO-8601 (see module docs for the emit format).
    pub create_time: Option<DateTime<Utc>>,
    /// Wire `lastModified` — drives incremental sync (§2.7 `modifiedAfter`).
    pub last_modified: Option<DateTime<Utc>>,
    /// Wire `lastAccessed` — when the record was last surfaced/applied.
    pub last_accessed: Option<DateTime<Utc>>,
    /// User-marked favorite. Always encoded.
    pub starred: bool,
    /// User-pinned (sticks to the top). Always encoded.
    pub pinned: bool,
    /// Server-side optimistic-lock version (see [`INITIAL_VERSION`] and the
    /// module-level lifecycle docs).
    pub version: Option<i64>,
    /// Soft-delete tombstone — the **read** name (spec §3.6). The PATCH
    /// update body uses `isDelete` (no trailing `d`); that asymmetry lives
    /// on [`HistoryRecordPatch`] so it can't leak here.
    pub is_deleted: bool,
}

impl HistoryRecord {
    /// Construct with the Swift initializer's defaults: everything optional
    /// is `None`, all flags `false` (HistoryRecord.swift `init`).
    ///
    /// Note: `version` defaults to `None` here (matching Swift); the server
    /// assigns [`INITIAL_VERSION`] (0) on §2.9 create.
    pub fn new(hash: impl Into<String>, kind: ClipboardKind) -> Self {
        Self {
            hash: hash.into(),
            kind,
            text: None,
            has_data: false,
            size: None,
            create_time: None,
            last_modified: None,
            last_accessed: None,
            starred: false,
            pinned: false,
            version: None,
            is_deleted: false,
        }
    }

    /// Swift `Identifiable.id`: the composite `"<type>-<hash>"` key used by
    /// §2.8 / §2.11 URL paths. See [`composite_profile_id`].
    pub fn profile_id(&self) -> String {
        composite_profile_id(self.kind, &self.hash)
    }

    /// The §2.10 split `"<type>/<hash>"` PATCH path id — a DIFFERENT format
    /// from [`Self::profile_id`]. See [`split_patch_id`].
    pub fn patch_path_id(&self) -> String {
        split_patch_id(self.kind, &self.hash)
    }
}

/// Outbound wire shape. Field declaration order mirrors the Swift
/// `encode(to:)` call order (hash, type, text, hasData, size, dates,
/// starred, pinned, version, isDeleted) so the JSON key order is stable
/// across implementations.
#[derive(Serialize)]
struct WireOut<'a> {
    hash: &'a str,
    #[serde(rename = "type")]
    kind: ClipboardKind,
    // `encodeIfPresent`: omitted when None, encoded when Some (even "").
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<&'a str>,
    // BYTE-CRITICAL: hasData/starred/pinned/isDeleted encoded
    // unconditionally (HistoryRecord.swift `encode`, Android parity).
    #[serde(rename = "hasData")]
    has_data: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<i64>,
    #[serde(rename = "createTime", skip_serializing_if = "Option::is_none")]
    create_time: Option<String>,
    #[serde(rename = "lastModified", skip_serializing_if = "Option::is_none")]
    last_modified: Option<String>,
    #[serde(rename = "lastAccessed", skip_serializing_if = "Option::is_none")]
    last_accessed: Option<String>,
    starred: bool,
    pinned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<i64>,
    #[serde(rename = "isDeleted")]
    is_deleted: bool,
}

impl Serialize for HistoryRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        WireOut {
            hash: &self.hash,
            kind: self.kind,
            text: self.text.as_deref(),
            has_data: self.has_data,
            size: self.size,
            create_time: self.create_time.as_ref().map(format_iso8601_utc),
            last_modified: self.last_modified.as_ref().map(format_iso8601_utc),
            last_accessed: self.last_accessed.as_ref().map(format_iso8601_utc),
            starred: self.starred,
            pinned: self.pinned,
            version: self.version,
            is_deleted: self.is_deleted,
        }
        .serialize(serializer)
    }
}

/// Inbound wire shape. Tolerant like the Swift decoder: only `hash` and
/// `type` are required; unknown fields are ignored (no deny_unknown_fields);
/// JSON `null` counts as absent (`decodeIfPresent` parity); timestamps come
/// in as raw strings and are parsed leniently afterwards.
#[derive(Deserialize)]
struct WireIn {
    hash: String,
    #[serde(rename = "type")]
    kind: ClipboardKind,
    #[serde(default)]
    text: Option<String>,
    #[serde(default, rename = "hasData")]
    has_data: Option<bool>,
    #[serde(default)]
    size: Option<i64>,
    #[serde(default, rename = "createTime")]
    create_time: Option<String>,
    #[serde(default, rename = "lastModified")]
    last_modified: Option<String>,
    #[serde(default, rename = "lastAccessed")]
    last_accessed: Option<String>,
    #[serde(default)]
    starred: Option<bool>,
    #[serde(default)]
    pinned: Option<bool>,
    #[serde(default)]
    version: Option<i64>,
    #[serde(default, rename = "isDeleted")]
    is_deleted: Option<bool>,
}

impl<'de> Deserialize<'de> for HistoryRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = WireIn::deserialize(deserializer)?;
        // Swift `decodeISODate`: empty/whitespace → nil; bad strings throw.
        let create_time =
            parse_optional_iso8601(raw.create_time.as_deref()).map_err(serde::de::Error::custom)?;
        let last_modified = parse_optional_iso8601(raw.last_modified.as_deref())
            .map_err(serde::de::Error::custom)?;
        let last_accessed = parse_optional_iso8601(raw.last_accessed.as_deref())
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            hash: raw.hash,
            kind: raw.kind,
            text: raw.text,
            has_data: raw.has_data.unwrap_or(false),
            size: raw.size,
            create_time,
            last_modified,
            last_accessed,
            starred: raw.starred.unwrap_or(false),
            pinned: raw.pinned.unwrap_or(false),
            version: raw.version,
            is_deleted: raw.is_deleted.unwrap_or(false),
        })
    }
}

// ─── PATCH update body (§2.10) ──────────────────────────────────────────

/// JSON body for `PATCH /api/history/<type>/<hash>` (spec §2.10) — the
/// `HistoryRecordUpdate` DTO the Swift model docs reserve for this
/// asymmetry. All fields optional; `None` fields are omitted entirely.
///
/// BYTE-CRITICAL: the soft-delete key here is `isDelete` — **no trailing
/// `d`** — unlike the read/create shapes' `isDeleted`. Spec §2.10: "This
/// looks like a typo in the server contract but is load-bearing — sending
/// `isDeleted` here is silently ignored." The raw string appears exactly
/// once in this crate's production code, on the serde rename below. Always
/// build PATCH bodies through this type; never hand-roll the JSON.
///
/// Field order mirrors the spec §2.10 example body.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HistoryRecordPatch {
    /// Star/unstar the record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starred: Option<bool>,
    /// Pin/unpin the record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    /// Soft-delete tombstone flag — wire name `isDelete` (see type docs).
    #[serde(rename = "isDelete", skip_serializing_if = "Option::is_none")]
    pub is_delete: Option<bool>,
    /// The client-known version (rebase point). Stale → server `409` with
    /// the current record in the body; reload, re-apply, retry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
    /// Optional `lastModified` override, emitted in the §3.6 ISO shape.
    #[serde(
        rename = "lastModified",
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_opt_iso8601"
    )]
    pub last_modified: Option<DateTime<Utc>>,
    /// Optional `lastAccessed` override, emitted in the §3.6 ISO shape.
    #[serde(
        rename = "lastAccessed",
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_opt_iso8601"
    )]
    pub last_accessed: Option<DateTime<Utc>>,
}

impl HistoryRecordPatch {
    /// Serialize the PATCH body to the exact wire JSON (minified, fields in
    /// declaration order, `None` fields absent).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Convenience soft-delete body: `{"isDelete":true}` plus the observed
    /// `version` when known (spec §3.6 lifecycle: soft delete is a §2.10
    /// PATCH with `"isDelete": true`).
    pub fn soft_delete(version: Option<i64>) -> Self {
        Self {
            is_delete: Some(true),
            version,
            ..Self::default()
        }
    }
}

/// Serde helper: emit an optional timestamp through [`format_iso8601_utc`]
/// so PATCH bodies share the exact §3.6 millisecond+`Z` shape.
fn serialize_opt_iso8601<S: Serializer>(
    value: &Option<DateTime<Utc>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(dt) => serializer.serialize_str(&format_iso8601_utc(dt)),
        // Unreachable in practice (skip_serializing_if), kept total for safety.
        None => serializer.serialize_none(),
    }
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    // Fixtures embedded VERBATIM from uc-ios docs/examples/*.json (the
    // FixturesTests.swift single-source fixtures).

    /// uc-ios docs/examples/history_record_text.json
    const FIXTURE_TEXT: &str = r#"{
  "hash": "3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F",
  "type": "Text",
  "text": "Hello, SyncClipboard!",
  "hasData": false,
  "size": 21,
  "createTime": "2026-05-17T16:43:00.000Z",
  "lastModified": "2026-05-17T16:43:21.420Z",
  "lastAccessed": "2026-05-17T16:43:21.420Z",
  "starred": false,
  "pinned": false,
  "version": 0,
  "isDeleted": false
}"#;

    /// uc-ios docs/examples/history_record_minimal.json
    const FIXTURE_MINIMAL: &str = r#"{
  "hash": "088EA33D054B64459EA2EB0CBD9F9152DD0BE4C38C6350963BBA00FDDC94CCEA",
  "type": "File"
}"#;

    /// uc-ios docs/examples/history_record_deleted.json
    const FIXTURE_DELETED: &str = r#"{
  "hash": "C9D8E7F605142332A1B0C9D8E7F60514233241506A7B8C9DAEBFC0D1E2F30415",
  "type": "Image",
  "text": "screenshot.png",
  "hasData": true,
  "size": 4096,
  "createTime": "2026-05-15T08:00:00.000Z",
  "lastModified": "2026-05-16T14:22:00.000Z",
  "version": 3,
  "isDeleted": true
}"#;

    fn keys(json: &str) -> Vec<String> {
        let v: Value = serde_json::from_str(json).expect("valid json");
        v.as_object()
            .expect("json object")
            .keys()
            .cloned()
            .collect()
    }

    // ── Swift golden vectors (FixturesTests.swift, §3.6 section) ──────

    /// Swift: `test_historyRecordText_decodesAllOptionalFields`
    #[test]
    fn history_record_text_decodes_all_optional_fields() {
        let record: HistoryRecord = serde_json::from_str(FIXTURE_TEXT).expect("decode ok");

        assert_eq!(record.kind, ClipboardKind::Text);
        assert_eq!(
            record.hash,
            "3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F"
        );
        assert_eq!(record.text.as_deref(), Some("Hello, SyncClipboard!"));
        assert!(!record.has_data);
        assert_eq!(record.size, Some(21));
        assert!(record.create_time.is_some());
        assert!(record.last_modified.is_some());
        assert!(record.last_accessed.is_some());
        assert!(!record.starred);
        assert!(!record.pinned);
        assert_eq!(record.version, Some(0));
        assert!(!record.is_deleted);
    }

    /// Swift: `test_historyRecordText_roundTripPreservesISOTimestamps`
    #[test]
    fn history_record_text_round_trip_preserves_iso_timestamps() {
        let decoded: HistoryRecord = serde_json::from_str(FIXTURE_TEXT).expect("decode ok");
        let re_encoded = serde_json::to_string(&decoded).expect("encode ok");
        let redecoded: HistoryRecord = serde_json::from_str(&re_encoded).expect("redecode ok");
        // Equality holds exactly because the fractional-ISO emitter has
        // millisecond resolution and the fixture's timestamps already round
        // to whole milliseconds.
        assert_eq!(decoded, redecoded);
    }

    /// Swift: `test_historyRecordMinimal_fillsDefaults`
    #[test]
    fn history_record_minimal_fills_defaults() {
        let record: HistoryRecord = serde_json::from_str(FIXTURE_MINIMAL).expect("decode ok");

        assert_eq!(record.kind, ClipboardKind::File);
        assert_eq!(
            record.hash,
            "088EA33D054B64459EA2EB0CBD9F9152DD0BE4C38C6350963BBA00FDDC94CCEA"
        );
        assert_eq!(record.text, None);
        assert!(!record.has_data);
        assert_eq!(record.size, None);
        assert_eq!(record.create_time, None);
        assert_eq!(record.last_modified, None);
        assert_eq!(record.last_accessed, None);
        assert!(!record.starred);
        assert!(!record.pinned);
        assert_eq!(record.version, None);
        assert!(!record.is_deleted);
    }

    /// Swift: `test_historyRecordDeleted_isDeletedReadAndRoundTrip`
    #[test]
    fn history_record_deleted_is_deleted_read_and_round_trip() {
        let record: HistoryRecord = serde_json::from_str(FIXTURE_DELETED).expect("decode ok");

        assert!(
            record.is_deleted,
            "Read shape uses isDeleted (not isDelete)"
        );
        assert_eq!(record.version, Some(3));

        // Re-encoding must NOT introduce isDelete (no trailing d) — that
        // key is the PATCH-update-body convention, not the read shape.
        let re_encoded = serde_json::to_string(&record).expect("encode ok");
        let keys = keys(&re_encoded);
        assert!(keys.iter().any(|k| k == "isDeleted"));
        assert!(!keys.iter().any(|k| k == "isDelete"));
    }

    /// Swift: `test_historyRecord_idIsCompositeTypeDashHash`
    #[test]
    fn history_record_id_is_composite_type_dash_hash() {
        let r = HistoryRecord::new("ABCDEF", ClipboardKind::Text);
        assert_eq!(r.profile_id(), "Text-ABCDEF");
        assert_eq!(
            composite_profile_id(ClipboardKind::Image, "XYZ"),
            "Image-XYZ"
        );
    }

    /// Swift: `test_historyRecord_decodesPlainISOWithoutFractionalSeconds`
    /// (the Android wire emits `…Z`; some hand-rolled servers truncate
    /// fractional seconds — both shapes MUST decode).
    #[test]
    fn history_record_decodes_plain_iso_without_fractional_seconds() {
        let json = r#"{
          "hash": "AA",
          "type": "Text",
          "createTime": "2026-05-17T16:43:00Z"
        }"#;
        let r: HistoryRecord = serde_json::from_str(json).expect("decode ok");
        assert!(r.create_time.is_some());
    }

    /// Swift: `test_historyRecord_emptyTimestampDecodesToNil`
    /// (empty/whitespace timestamps decode to None rather than erroring,
    /// matching the `hash` normalization on `Clipboard`).
    #[test]
    fn history_record_empty_timestamp_decodes_to_none() {
        let json = r#"{
          "hash": "AA",
          "type": "Text",
          "createTime": "",
          "lastModified": "   "
        }"#;
        let r: HistoryRecord = serde_json::from_str(json).expect("decode ok");
        assert_eq!(r.create_time, None);
        assert_eq!(r.last_modified, None);
    }

    // ── composite vs split id (spec §2.10 vs §2.8/§2.11) ──────────────

    #[test]
    fn split_patch_id_uses_slash_not_dash() {
        // Spec §2.10 example path suffix:
        // /api/history/Text/3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F
        let hash = "3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F";
        assert_eq!(
            split_patch_id(ClipboardKind::Text, hash),
            format!("Text/{hash}")
        );
        // The two id forms must never coincide.
        assert_ne!(
            split_patch_id(ClipboardKind::Text, hash),
            composite_profile_id(ClipboardKind::Text, hash)
        );
        let r = HistoryRecord::new(hash, ClipboardKind::Text);
        assert_eq!(r.patch_path_id(), format!("Text/{hash}"));
    }

    // ── PATCH body (spec §2.10) ────────────────────────────────────────

    #[test]
    fn patch_body_full_matches_spec_example() {
        // Spec §2.10 example body, minified, declaration order.
        let dt = parse_iso8601_utc("2026-05-17T16:43:21.420Z").expect("parse ok");
        let patch = HistoryRecordPatch {
            starred: Some(true),
            pinned: Some(false),
            is_delete: Some(true),
            version: Some(3),
            last_modified: Some(dt),
            last_accessed: Some(dt),
        };
        assert_eq!(
            patch.to_json().expect("encode ok"),
            r#"{"starred":true,"pinned":false,"isDelete":true,"version":3,"lastModified":"2026-05-17T16:43:21.420Z","lastAccessed":"2026-05-17T16:43:21.420Z"}"#
        );
    }

    #[test]
    fn patch_body_uses_is_delete_without_trailing_d() {
        let json = HistoryRecordPatch::soft_delete(Some(3))
            .to_json()
            .expect("encode ok");
        assert_eq!(json, r#"{"isDelete":true,"version":3}"#);
        let keys = keys(&json);
        assert!(keys.iter().any(|k| k == "isDelete"));
        assert!(
            !keys.iter().any(|k| k == "isDeleted"),
            "PATCH body must not use the read-shape isDeleted key"
        );
    }

    #[test]
    fn patch_body_omits_none_fields_entirely() {
        let json = HistoryRecordPatch::default().to_json().expect("encode ok");
        assert_eq!(json, "{}");
    }

    // ── encode rules (HistoryRecord.swift `encode(to:)`) ──────────────

    #[test]
    fn encode_emits_flags_unconditionally_and_omits_absent_optionals() {
        let r = HistoryRecord::new("AA", ClipboardKind::Text);
        let json = serde_json::to_string(&r).expect("encode ok");
        let v: Value = serde_json::from_str(&json).expect("valid json");
        let obj = v.as_object().expect("object");
        // hasData / starred / pinned / isDeleted: always present, even false.
        assert_eq!(obj.get("hasData"), Some(&Value::Bool(false)));
        assert_eq!(obj.get("starred"), Some(&Value::Bool(false)));
        assert_eq!(obj.get("pinned"), Some(&Value::Bool(false)));
        assert_eq!(obj.get("isDeleted"), Some(&Value::Bool(false)));
        // Optionals: whole key omitted (never null).
        for absent in [
            "text",
            "size",
            "createTime",
            "lastModified",
            "lastAccessed",
            "version",
        ] {
            assert!(!obj.contains_key(absent), "{absent} must be omitted");
        }
    }

    #[test]
    fn encode_emits_present_but_empty_text() {
        // Swift `encodeIfPresent` keys off nil-ness, NOT emptiness: a
        // present empty string IS encoded.
        let mut r = HistoryRecord::new("AA", ClipboardKind::Text);
        r.text = Some(String::new());
        let json = serde_json::to_string(&r).expect("encode ok");
        let v: Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(
            v.as_object().expect("object").get("text"),
            Some(&Value::String(String::new()))
        );
    }

    #[test]
    fn encode_key_order_matches_swift_encode_call_order() {
        let decoded: HistoryRecord = serde_json::from_str(FIXTURE_TEXT).expect("decode ok");
        let json = serde_json::to_string(&decoded).expect("encode ok");
        assert_eq!(
            json,
            r#"{"hash":"3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F","type":"Text","text":"Hello, SyncClipboard!","hasData":false,"size":21,"createTime":"2026-05-17T16:43:00.000Z","lastModified":"2026-05-17T16:43:21.420Z","lastAccessed":"2026-05-17T16:43:21.420Z","starred":false,"pinned":false,"version":0,"isDeleted":false}"#
        );
    }

    // ── ISO-8601 date helpers ──────────────────────────────────────────

    #[test]
    fn parse_accepts_all_four_iso_combos() {
        // fractional × (Z | +00:00) — all four decode to the same instant
        // (HistoryRecord.swift decodeISODate tolerance).
        let expected = parse_iso8601_utc("2026-05-17T16:43:21.420Z").expect("parse ok");
        for raw in ["2026-05-17T16:43:21.420Z", "2026-05-17T16:43:21.420+00:00"] {
            assert_eq!(parse_iso8601_utc(raw).expect("parse ok"), expected, "{raw}");
        }
        let expected_plain = parse_iso8601_utc("2026-05-17T16:43:21Z").expect("parse ok");
        for raw in ["2026-05-17T16:43:21Z", "2026-05-17T16:43:21+00:00"] {
            assert_eq!(
                parse_iso8601_utc(raw).expect("parse ok"),
                expected_plain,
                "{raw}"
            );
        }
    }

    #[test]
    fn format_emits_exactly_three_fractional_digits_and_z() {
        // BYTE-CRITICAL: Swift ISO8601DateFormatter+.withFractionalSeconds
        // emits milliseconds (3 digits) and `Z`, zero-padded.
        let dt = parse_iso8601_utc("2026-05-17T16:43:00Z").expect("parse ok");
        assert_eq!(format_iso8601_utc(&dt), "2026-05-17T16:43:00.000Z");
        let dt = parse_iso8601_utc("2026-05-17T16:43:21.420Z").expect("parse ok");
        assert_eq!(format_iso8601_utc(&dt), "2026-05-17T16:43:21.420Z");
    }

    #[test]
    fn format_normalizes_non_utc_offset_to_z() {
        // +02:00 input is the same instant as 16:43:21.420Z; emission is
        // always UTC + `Z` (Swift Date is offset-agnostic, formatter is GMT).
        let dt = parse_iso8601_utc("2026-05-17T18:43:21.420+02:00").expect("parse ok");
        assert_eq!(format_iso8601_utc(&dt), "2026-05-17T16:43:21.420Z");
    }

    #[test]
    fn parse_rejects_lowercase_t_z_and_space_separator() {
        // Swift ISO8601DateFormatter only accepts UPPERCASE `T`/`Z` and no
        // space separator; chrono's RFC-3339 parser tolerates all three, so
        // the helper guards explicitly to stay decode-equivalent with Swift.
        for raw in [
            "2026-05-17t16:43:21Z",
            "2026-05-17T16:43:21z",
            "2026-05-17 16:43:21Z",
        ] {
            assert!(parse_iso8601_utc(raw).is_err(), "{raw} must be rejected");
        }
    }

    #[test]
    fn unparseable_timestamp_fails_decoding() {
        // Swift decodeISODate fails loud rather than guessing.
        let json = r#"{"hash":"AA","type":"Text","createTime":"yesterday"}"#;
        assert!(serde_json::from_str::<HistoryRecord>(json).is_err());
    }

    // ── decode tolerance ───────────────────────────────────────────────

    #[test]
    fn decode_ignores_unknown_fields() {
        let json = r#"{"hash":"AA","type":"Text","futureField":42}"#;
        let r: HistoryRecord = serde_json::from_str(json).expect("decode ok");
        assert_eq!(r.hash, "AA");
    }

    #[test]
    fn decode_treats_null_optionals_as_absent() {
        // Swift decodeIfPresent: JSON null == missing.
        let json = r#"{
          "hash": "AA",
          "type": "Text",
          "text": null,
          "hasData": null,
          "size": null,
          "createTime": null,
          "version": null,
          "isDeleted": null
        }"#;
        let r: HistoryRecord = serde_json::from_str(json).expect("decode ok");
        assert_eq!(r.text, None);
        assert!(!r.has_data);
        assert_eq!(r.size, None);
        assert_eq!(r.create_time, None);
        assert_eq!(r.version, None);
        assert!(!r.is_deleted);
    }

    #[test]
    fn decode_requires_hash_and_type() {
        assert!(serde_json::from_str::<HistoryRecord>(r#"{"type":"Text"}"#).is_err());
        assert!(serde_json::from_str::<HistoryRecord>(r#"{"hash":"AA"}"#).is_err());
        // Unknown kind raw value fails, like Swift's Codable enum.
        assert!(serde_json::from_str::<HistoryRecord>(r#"{"hash":"AA","type":"Video"}"#).is_err());
    }

    #[test]
    fn kind_raw_values_match_wire() {
        // Clipboard.swift Kind raw values (FixturesTests
        // `test_clipboard_kindRawValuesMatchWire` for the shared enum).
        assert_eq!(ClipboardKind::Text.as_wire_str(), "Text");
        assert_eq!(ClipboardKind::Image.as_wire_str(), "Image");
        assert_eq!(ClipboardKind::File.as_wire_str(), "File");
        assert_eq!(ClipboardKind::Group.as_wire_str(), "Group");
        for kind in [
            ClipboardKind::Text,
            ClipboardKind::Image,
            ClipboardKind::File,
            ClipboardKind::Group,
        ] {
            let json = serde_json::to_string(&kind).expect("encode ok");
            assert_eq!(json, format!("\"{}\"", kind.as_wire_str()));
        }
    }

    #[test]
    fn initial_version_is_zero() {
        // Spec §3.6 lifecycle: records are created via §2.9 with version 0.
        assert_eq!(INITIAL_VERSION, 0);
        // The model default mirrors Swift's init (version: nil) — the
        // server, not the client model, assigns 0 on create.
        assert_eq!(HistoryRecord::new("AA", ClipboardKind::Text).version, None);
    }
}
