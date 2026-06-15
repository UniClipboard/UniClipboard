//! Cross-process file-/UserDefaults-backed state primitives — the pure codec
//! halves of the App Group state in uc-ios `Shared/Models/SettingsStore.swift`.
//!
//! The native layer owns the actual storage (atomic file writes, the App Group
//! container, `UserDefaults`); these functions own the value normalization and
//! the byte/string shapes so desktop / iOS / future Android agree. Each is a
//! pure transform: text/bytes in → normalized value out.
//!
//! Covered: `last_synced_hash` (plain text), the §2.7 history watermark +
//! throttle timestamp (ISO-8601 strings), and the `live_urls` JSON map. SSID
//! normalization already lives in [`crate::net_class::normalize_ssid`] and is
//! used verbatim by `last_known_ssid`.

use crate::history_record::{format_iso8601_utc, parse_iso8601_utc};
use chrono::{TimeZone, Utc};
use std::collections::BTreeMap;

// ─── last_synced_hash (plain-text file) ──────────────────────────────────

/// Normalize a `last_synced_hash` value (Swift `loadLastSyncedHash` /
/// `saveLastSyncedHash`): trim, treat empty as absent, uppercase. Use on both
/// read (raw file text → `Option`) and write (`None`/empty ⇒ the native layer
/// removes the file; `Some` ⇒ write the normalized value).
pub fn normalize_synced_hash(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_uppercase())
    }
}

// ─── history watermark + throttle timestamp (ISO-8601 strings) ────────────

/// Parse a stored ISO-8601 watermark / throttle timestamp into epoch-millis
/// (Swift `loadHistoryWatermark` / `loadLastHistorySyncAt`): trim, empty →
/// `None`, then the tolerant §3.6 parse (fractional OR plain seconds). An
/// unparseable value reads as `None` ("fetch everything next round").
pub fn parse_watermark(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    parse_iso8601_utc(trimmed)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

/// Format an epoch-millis instant as the persisted ISO-8601 string (Swift
/// `saveHistoryWatermark` / `saveLastHistorySyncAt`): the same fractional-
/// seconds + `Z` shape the §3.6 wire uses, so the persisted and wire forms
/// match. `None` only for an out-of-range instant the native layer never
/// produces (a real `Date`).
pub fn format_watermark(millis: i64) -> Option<String> {
    Utc.timestamp_millis_opt(millis)
        .single()
        .map(|dt| format_iso8601_utc(&dt))
}

// ─── live_urls map (§5.3 probe result, JSON {configId: url}) ──────────────

/// Decode the `live_urls` JSON map (`{configId: url}`). Corruption reads as an
/// empty map (Swift `loadLiveURLMap` — never blocks the caller).
pub fn decode_live_urls(bytes: &[u8]) -> BTreeMap<String, String> {
    serde_json::from_slice(bytes).unwrap_or_default()
}

/// Encode the `live_urls` map to its persisted JSON form.
pub fn encode_live_urls(map: &BTreeMap<String, String>) -> Vec<u8> {
    serde_json::to_vec(map).unwrap_or_default()
}

/// Apply a per-profile live-URL update (Swift `saveLiveURL`): `Some(url)` sets
/// it, `None` clears it. Returns the new map; an EMPTY result signals the
/// native layer to delete the backing file rather than write `{}`.
pub fn update_live_url(
    mut map: BTreeMap<String, String>,
    config_id: String,
    url: Option<String>,
) -> BTreeMap<String, String> {
    match url {
        Some(u) => {
            map.insert(config_id, u);
        }
        None => {
            map.remove(&config_id);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── last_synced_hash ──
    #[test]
    fn synced_hash_normalizes_and_trims() {
        // Swift `test_lastSyncedHash_isNormalizedToUppercase`.
        assert_eq!(
            normalize_synced_hash("abababab"),
            Some("ABABABAB".to_string())
        );
        assert_eq!(
            normalize_synced_hash("  DEADBEEF \n"),
            Some("DEADBEEF".to_string())
        );
    }

    #[test]
    fn synced_hash_empty_is_absent() {
        assert_eq!(normalize_synced_hash(""), None);
        assert_eq!(normalize_synced_hash("   "), None);
    }

    // ── watermark ──
    #[test]
    fn watermark_round_trips_to_millisecond() {
        // Swift `test_historyWatermark_saveThenLoad_roundTripsToMillisecond`.
        let millis = parse_watermark("2026-05-17T16:43:21.420Z").expect("parses");
        let formatted = format_watermark(millis).expect("formats");
        assert_eq!(formatted, "2026-05-17T16:43:21.420Z");
        assert_eq!(parse_watermark(&formatted), Some(millis));
    }

    #[test]
    fn watermark_accepts_plain_iso_without_fractional() {
        // Swift `test_historyWatermark_acceptsPlainISOWithoutFractionalSeconds`.
        assert!(parse_watermark("2026-05-17T16:43:21Z").is_some());
    }

    #[test]
    fn watermark_corrupt_or_empty_is_none() {
        // Swift `test_historyWatermark_corruptStringReturnsNil` + empty case.
        assert_eq!(parse_watermark("not a date"), None);
        assert_eq!(parse_watermark(""), None);
        assert_eq!(parse_watermark("   "), None);
    }

    // ── live_urls ──
    #[test]
    fn live_url_set_get_clear() {
        // Swift `test_liveURL_*` (the pure map half).
        let mut map = BTreeMap::new();
        map = update_live_url(
            map,
            "c1".to_string(),
            Some("http://192.168.1.9:5033".to_string()),
        );
        map = update_live_url(
            map,
            "c2".to_string(),
            Some("https://wan.example".to_string()),
        );
        assert_eq!(
            map.get("c1").map(String::as_str),
            Some("http://192.168.1.9:5033")
        );

        // Clearing c1 leaves c2.
        map = update_live_url(map, "c1".to_string(), None);
        assert_eq!(map.get("c1"), None);
        assert_eq!(
            map.get("c2").map(String::as_str),
            Some("https://wan.example")
        );
        assert!(!map.is_empty(), "non-empty ⇒ native rewrites the file");
    }

    #[test]
    fn live_url_clearing_last_entry_yields_empty() {
        let mut map = BTreeMap::new();
        map = update_live_url(map, "c1".to_string(), Some("http://x".to_string()));
        map = update_live_url(map, "c1".to_string(), None);
        assert!(map.is_empty(), "empty ⇒ native deletes the backing file");
    }

    #[test]
    fn live_url_round_trips() {
        let mut map = BTreeMap::new();
        map.insert("c1".to_string(), "http://192.168.1.9:5033".to_string());
        let bytes = encode_live_urls(&map);
        assert_eq!(decode_live_urls(&bytes), map);
    }

    #[test]
    fn live_url_corrupt_reads_as_empty() {
        // Swift `test_liveURL_corruptFileReadsAsAbsent`.
        assert!(decode_live_urls(b"not json").is_empty());
        assert!(decode_live_urls(b"").is_empty());
    }
}
