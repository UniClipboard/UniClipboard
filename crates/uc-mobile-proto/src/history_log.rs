//! Local clipboard observation log (`clipboard_history` blob) + dedup-append.
//!
//! Port of uc-ios `Shared/Models/ClipboardHistoryItem.swift` and the
//! `SettingsStore.appendHistory` / `touchHistoryItem` transforms (also mirrored
//! in `AppViewModel.appendHistory` — migrating the algorithm here collapses the
//! two Swift copies to one source of truth).
//!
//! This is NOT part of the SyncClipboard wire protocol: the server keeps one
//! live record (§2.1) and each client accumulates its own log locally, capped
//! and newest-first.
//!
//! Byte-compat (BYTE-CRITICAL — Rust must read existing native blobs and write
//! blobs the native reader still parses):
//! - `id` is a UUID string (Swift `UUID` encodes its UPPERCASE `uuidString`).
//! - `timestamp` is Swift's default `Date` encoding: a JSON number of seconds
//!   since the reference date 2001-01-01 UTC (`timeIntervalSinceReferenceDate`).
//!   The FFI/in-memory representation is epoch-milliseconds (`i64`, consistent
//!   with the M2 history wire types); [`date_2001`] converts at the serde edge.
//! - `direction` is the lowercase raw value `pulled` / `pushed` / `local`.

use crate::clipboard_doc::Clipboard;
use serde::{Deserialize, Serialize};

/// Which way a logged entry flowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryDirection {
    /// Server → device (apply).
    Pulled,
    /// Device → server (push).
    Pushed,
    /// Observed locally, not yet attributed to a sync (provenance upgradeable).
    Local,
}

/// One row in the Home tab's time-descending clipboard list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipboardHistoryItem {
    /// Stable identity (UUID string, uppercase to match Swift `UUID` Codable).
    pub id: String,
    /// The observed clipboard snapshot.
    pub entry: Clipboard,
    /// Observation time, epoch-milliseconds at the FFI edge; serialized as
    /// seconds-since-2001 to match Swift's default `Date` encoding.
    #[serde(rename = "timestamp", with = "date_2001")]
    pub timestamp_millis: i64,
    /// Provenance direction.
    pub direction: HistoryDirection,
}

/// Serde adapter between epoch-milliseconds (`i64`) and Swift's default `Date`
/// JSON encoding (`Double` seconds since the 2001-01-01 UTC reference date).
mod date_2001 {
    use serde::{Deserialize, Deserializer, Serializer};

    /// Seconds between the Unix epoch (1970-01-01) and Swift's reference date
    /// (2001-01-01 UTC). `Date.timeIntervalSinceReferenceDate == unixSeconds -
    /// 978_307_200`.
    const REFERENCE_OFFSET_SECS: f64 = 978_307_200.0;

    pub fn serialize<S: Serializer>(millis: &i64, serializer: S) -> Result<S::Ok, S::Error> {
        let secs_since_2001 = (*millis as f64) / 1000.0 - REFERENCE_OFFSET_SECS;
        serializer.serialize_f64(secs_since_2001)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
        let secs_since_2001 = f64::deserialize(deserializer)?;
        let unix_millis = (secs_since_2001 + REFERENCE_OFFSET_SECS) * 1000.0;
        Ok(unix_millis.round() as i64)
    }
}

/// Default history cap (Swift `SettingsStore.appendHistory(cap: 200)`).
pub const DEFAULT_HISTORY_CAP: i64 = 200;

/// Append one observation to the log, newest-first, deduped against the head
/// and capped. Pure port of `SettingsStore.appendHistory`:
/// - same content already at the head (matching `entry.hash`) → never insert a
///   duplicate row, regardless of direction; upgrade a `.local` head's
///   provenance to pushed/pulled in place (keep the stronger direction
///   otherwise), then return unchanged.
/// - otherwise insert at index 0 with `new_id` / `timestamp_millis`, then
///   truncate to `cap`.
///
/// `new_id` (a fresh UUID) and `timestamp_millis` are supplied by the native
/// layer so this stays a pure, deterministic transform.
pub fn append_history(
    mut items: Vec<ClipboardHistoryItem>,
    entry: Clipboard,
    direction: HistoryDirection,
    timestamp_millis: i64,
    new_id: String,
    cap: i64,
) -> Vec<ClipboardHistoryItem> {
    if let (Some(hash), Some(head)) = (entry.hash.as_deref(), items.first()) {
        if head.entry.hash.as_deref() == Some(hash) {
            if direction != HistoryDirection::Local && head.direction != direction {
                items[0].direction = direction;
            }
            return items;
        }
    }
    items.insert(
        0,
        ClipboardHistoryItem {
            id: new_id,
            entry,
            timestamp_millis,
            direction,
        },
    );
    let cap = cap.max(0) as usize;
    if items.len() > cap {
        items.truncate(cap);
    }
    items
}

/// Move the item with `id` to the head, restamping its timestamp. Mirrors
/// `SettingsStore.touchHistoryItem` (the `Date()` now-stamp is passed in). A
/// missing `id` is a no-op.
pub fn touch_history(
    mut items: Vec<ClipboardHistoryItem>,
    id: &str,
    timestamp_millis: i64,
) -> Vec<ClipboardHistoryItem> {
    if let Some(idx) = items.iter().position(|it| it.id == id) {
        let mut item = items.remove(idx);
        item.timestamp_millis = timestamp_millis;
        items.insert(0, item);
    }
    items
}

/// Decode the `clipboard_history` blob; corruption returns `[]` (Swift
/// `SettingsStore.loadHistory` corruption policy — never block startup).
pub fn decode_history(bytes: &[u8]) -> Vec<ClipboardHistoryItem> {
    serde_json::from_slice(bytes).unwrap_or_default()
}

/// Encode the log to its persisted JSON form. An empty array still encodes to
/// `[]` (Swift `saveHistory` keeps the key present so a reload round-trips).
pub fn encode_history(items: &[ClipboardHistoryItem]) -> Vec<u8> {
    serde_json::to_vec(items).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard_doc::ClipboardKind;

    fn text_entry(hash: &str, text: &str) -> Clipboard {
        Clipboard::new(
            ClipboardKind::Text,
            Some(hash.to_string()),
            text.to_string(),
            false,
            None,
            Some(text.chars().count() as i64),
        )
    }

    fn item(id: &str, hash: &str, dir: HistoryDirection, ts: i64) -> ClipboardHistoryItem {
        ClipboardHistoryItem {
            id: id.to_string(),
            entry: text_entry(hash, "x"),
            timestamp_millis: ts,
            direction: dir,
        }
    }

    #[test]
    fn append_inserts_newest_first() {
        let items = append_history(
            Vec::new(),
            text_entry("AA", "one"),
            HistoryDirection::Pulled,
            1000,
            "id1".to_string(),
            DEFAULT_HISTORY_CAP,
        );
        let items = append_history(
            items,
            text_entry("BB", "two"),
            HistoryDirection::Pushed,
            2000,
            "id2".to_string(),
            DEFAULT_HISTORY_CAP,
        );
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "id2", "newest at head");
        assert_eq!(items[1].id, "id1");
    }

    #[test]
    fn append_dedups_same_hash_at_head() {
        let items = vec![item("id1", "AA", HistoryDirection::Pulled, 1000)];
        let after = append_history(
            items,
            text_entry("AA", "again"),
            HistoryDirection::Pulled,
            2000,
            "id2".to_string(),
            DEFAULT_HISTORY_CAP,
        );
        assert_eq!(after.len(), 1, "same hash at head not re-inserted");
        assert_eq!(after[0].id, "id1");
    }

    #[test]
    fn append_upgrades_local_head_to_directional() {
        let items = vec![item("id1", "AA", HistoryDirection::Local, 1000)];
        let after = append_history(
            items,
            text_entry("AA", "x"),
            HistoryDirection::Pushed,
            2000,
            "id2".to_string(),
            DEFAULT_HISTORY_CAP,
        );
        assert_eq!(after.len(), 1);
        assert_eq!(
            after[0].direction,
            HistoryDirection::Pushed,
            "local upgraded"
        );
        assert_eq!(after[0].id, "id1");
    }

    #[test]
    fn append_local_does_not_downgrade_directional_head() {
        let items = vec![item("id1", "AA", HistoryDirection::Pushed, 1000)];
        let after = append_history(
            items,
            text_entry("AA", "x"),
            HistoryDirection::Local,
            2000,
            "id2".to_string(),
            DEFAULT_HISTORY_CAP,
        );
        assert_eq!(
            after[0].direction,
            HistoryDirection::Pushed,
            "local never downgrades"
        );
    }

    #[test]
    fn append_same_hash_not_at_head_inserts_new_row() {
        // Head is a different hash → the matching-hash rule only checks the
        // head, so this inserts a fresh row.
        let items = vec![
            item("idHead", "BB", HistoryDirection::Pulled, 2000),
            item("idOld", "AA", HistoryDirection::Pulled, 1000),
        ];
        let after = append_history(
            items,
            text_entry("AA", "x"),
            HistoryDirection::Pushed,
            3000,
            "idNew".to_string(),
            DEFAULT_HISTORY_CAP,
        );
        assert_eq!(after.len(), 3);
        assert_eq!(after[0].id, "idNew");
    }

    #[test]
    fn append_caps_oldest_dropped() {
        let mut items = Vec::new();
        for i in 0..3 {
            items = append_history(
                items,
                text_entry(&format!("H{i}"), "x"),
                HistoryDirection::Pulled,
                i as i64,
                format!("id{i}"),
                2,
            );
        }
        assert_eq!(items.len(), 2, "capped at 2");
        assert_eq!(items[0].id, "id2", "newest kept");
        assert_eq!(items[1].id, "id1", "oldest (id0) dropped");
    }

    #[test]
    fn append_entry_without_hash_always_inserts() {
        // A None-hash entry can never match the head, so it always inserts.
        let no_hash = Clipboard::new(
            ClipboardKind::Text,
            None,
            "x".to_string(),
            false,
            None,
            None,
        );
        let items = vec![item("id1", "AA", HistoryDirection::Pulled, 1000)];
        let after = append_history(
            items,
            no_hash,
            HistoryDirection::Pulled,
            2000,
            "id2".to_string(),
            DEFAULT_HISTORY_CAP,
        );
        assert_eq!(after.len(), 2);
    }

    #[test]
    fn touch_moves_to_head_and_restamps() {
        let items = vec![
            item("a", "AA", HistoryDirection::Pulled, 1000),
            item("b", "BB", HistoryDirection::Pulled, 2000),
        ];
        let after = touch_history(items, "b", 9999);
        assert_eq!(after[0].id, "b");
        assert_eq!(after[0].timestamp_millis, 9999);
        assert_eq!(after[1].id, "a");
    }

    #[test]
    fn touch_missing_id_is_noop() {
        let items = vec![item("a", "AA", HistoryDirection::Pulled, 1000)];
        let after = touch_history(items.clone(), "ghost", 9999);
        assert_eq!(after, items);
    }

    #[test]
    fn decode_empty_or_corrupt_returns_empty() {
        assert_eq!(decode_history(b""), Vec::new());
        assert_eq!(decode_history(b"not json"), Vec::new());
    }

    #[test]
    fn round_trip_preserves_ids_directions_and_timestamps() {
        let items = vec![
            item("ID-AAA", "11", HistoryDirection::Pulled, 1_700_000_000_000),
            item("ID-BBB", "22", HistoryDirection::Pushed, 1_700_000_100_000),
        ];
        let bytes = encode_history(&items);
        assert_eq!(decode_history(&bytes), items);
    }

    #[test]
    fn empty_array_round_trips() {
        let bytes = encode_history(&[]);
        assert_eq!(bytes, b"[]");
        assert_eq!(decode_history(&bytes), Vec::new());
    }

    /// BYTE-CRITICAL: `timestamp` serializes as seconds-since-2001 (Swift
    /// default `Date` encoding), `direction` as the lowercase raw value.
    #[test]
    fn timestamp_serializes_as_seconds_since_2001() {
        // 2001-01-01T00:00:00Z is unix 978_307_200 → millis 978_307_200_000 →
        // serialized value 0.0.
        let items = vec![item("ID", "AA", HistoryDirection::Pushed, 978_307_200_000)];
        let json = String::from_utf8(encode_history(&items)).unwrap();
        assert!(json.contains("\"timestamp\":0"), "got {json}");
        assert!(json.contains("\"direction\":\"pushed\""));
        // 1 hour after the reference date → 3600 seconds.
        let items = vec![item(
            "ID",
            "AA",
            HistoryDirection::Pushed,
            978_307_200_000 + 3_600_000,
        )];
        let json = String::from_utf8(encode_history(&items)).unwrap();
        assert!(json.contains("\"timestamp\":3600"), "got {json}");
    }

    /// A blob written with Swift's reference-date `Double` decodes to the right
    /// epoch-millis (1_700_000_000_000 → secs-since-2001 721_692_800).
    #[test]
    fn decodes_swift_reference_date_double() {
        let blob = br#"[{"id":"ID","entry":{"type":"Text","text":"x","hasData":false},"timestamp":721692800,"direction":"pulled"}]"#;
        let items = decode_history(blob);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].timestamp_millis, 1_700_000_000_000);
        assert_eq!(items[0].direction, HistoryDirection::Pulled);
    }
}
