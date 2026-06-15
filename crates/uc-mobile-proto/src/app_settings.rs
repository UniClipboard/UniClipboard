//! User-tunable application settings (`app_settings` blob) — spec §5.4.
//!
//! Port of uc-ios `Shared/Models/AppSettings.swift`. The persisted JSON is the
//! cross-platform single source of truth (desktop / iOS / future Android share
//! the `app_settings` key), so the byte shape must match Swift's `Codable`:
//! camelCase keys (verbatim Swift field names), `appearance` as a raw string,
//! `ignoredVersion` omitted when absent.
//!
//! Forward-compat rules (mirrors Swift `init(from:)`): missing keys are filled
//! from [`AppSettings::default`]; unknown keys are tolerated (serde ignores
//! them); an unknown `appearance` value falls back to `System`. The corruption
//! policy ([`decode_app_settings`]) returns the defaults for an undecodable
//! blob — stored data must never block app startup.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 200 MiB — the default on-device payload-cache disk cap (Swift
/// `AppSettings.defaults.payloadCacheMaxBytes`).
pub const DEFAULT_PAYLOAD_CACHE_MAX_BYTES: i64 = 200 * 1024 * 1024;

/// User-selectable UI appearance. Raw wire values `system` / `light` / `dark`
/// match Swift `AppearanceMode.rawValue`. Unknown values decode to `System`
/// (Swift `AppSettings.init(from:)` — safer than throwing and losing every
/// other setting in the blob).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppearanceMode {
    /// Defer to the OS appearance (Swift default).
    #[default]
    System,
    /// Force light scheme.
    Light,
    /// Force dark scheme.
    Dark,
}

impl AppearanceMode {
    /// The exact wire string (Swift `rawValue`).
    pub fn as_wire_str(self) -> &'static str {
        match self {
            AppearanceMode::System => "system",
            AppearanceMode::Light => "light",
            AppearanceMode::Dark => "dark",
        }
    }

    /// Parse a wire string; unknown values fall back to `System`.
    pub fn from_wire_str(raw: &str) -> Self {
        match raw {
            "light" => AppearanceMode::Light,
            "dark" => AppearanceMode::Dark,
            _ => AppearanceMode::System,
        }
    }
}

impl Serialize for AppearanceMode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_wire_str())
    }
}

impl<'de> Deserialize<'de> for AppearanceMode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Unknown raw value → System (never errors), so one stray field can't
        // sink the whole blob — exactly Swift's behavior.
        let raw = String::deserialize(deserializer)?;
        Ok(AppearanceMode::from_wire_str(&raw))
    }
}

/// User-tunable application settings persisted under the `app_settings` key
/// (spec §5.4). Field names map to camelCase JSON keys via
/// `rename_all = "camelCase"`; the container-level `default` fills any missing
/// key from [`AppSettings::default`] on decode (forward-compat).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// Skip TLS certificate validation for the sync server (self-signed LAN).
    pub trust_insecure_cert: bool,
    /// Whether the app checks for updates on launch.
    pub auto_check_update: bool,
    /// Whether the one-time manual-upload explainer has been shown.
    pub manual_upload_dialog_shown: bool,
    /// Relative download path under the app's documents dir.
    pub download_relative_path: String,
    /// Minimum log level surfaced in the in-app log viewer.
    pub log_view_level_filter: String,
    /// A release the user chose to skip; omitted from JSON when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignored_version: Option<String>,
    /// Whether the engine writes new server content straight to the pasteboard.
    pub auto_apply_server_changes: bool,
    /// Whether the engine actively reads the pasteboard and auto-pushes.
    pub auto_push_device_changes: bool,
    /// Whether incoming entries with payloads are prefetched into the cache.
    pub prefetch_attachments: bool,
    /// Gates `prefetch_attachments` on the current network class.
    pub prefetch_on_cellular: bool,
    /// Payload-cache disk cap in bytes.
    pub payload_cache_max_bytes: i64,
    /// UI appearance preference.
    pub appearance: AppearanceMode,
    /// Whether the keyboard extension plays a key-click sound.
    pub keyboard_sound_feedback: bool,
    /// Whether the keyboard extension fires a light haptic on key taps.
    pub keyboard_haptic_feedback: bool,
    /// Whether the first-run onboarding walkthrough has been shown.
    pub onboarding_shown: bool,
    /// Whether the Home paste-permission hint banner has been dismissed.
    pub paste_permission_hint_dismissed: bool,
    /// Whether the post-pairing enhancements carousel has been shown.
    pub enhancements_prompt_shown: bool,
}

impl Default for AppSettings {
    /// Mirrors `AppSettings.defaults` in `AppSettings.swift` (spec §5.4 — the
    /// E-section default table the regression checklist pins).
    fn default() -> Self {
        Self {
            trust_insecure_cert: false,
            auto_check_update: true,
            manual_upload_dialog_shown: false,
            download_relative_path: String::new(),
            log_view_level_filter: "info".to_string(),
            ignored_version: None,
            auto_apply_server_changes: true,
            auto_push_device_changes: false,
            prefetch_attachments: true,
            prefetch_on_cellular: false,
            payload_cache_max_bytes: DEFAULT_PAYLOAD_CACHE_MAX_BYTES,
            appearance: AppearanceMode::System,
            keyboard_sound_feedback: true,
            keyboard_haptic_feedback: true,
            onboarding_shown: false,
            paste_permission_hint_dismissed: false,
            enhancements_prompt_shown: false,
        }
    }
}

/// Decode the `app_settings` blob. Corruption policy (Swift
/// `SettingsStore.loadAppSettings`): an undecodable blob returns
/// [`AppSettings::default`] — stored data must never block startup.
pub fn decode_app_settings(bytes: &[u8]) -> AppSettings {
    serde_json::from_slice(bytes).unwrap_or_default()
}

/// Encode `app_settings` to its persisted JSON form. Infallible for this struct
/// (no map keys, no non-finite floats); the `unwrap_or_default` is a
/// belt-and-suspenders fallback that never fires in practice.
pub fn encode_app_settings(settings: &AppSettings) -> Vec<u8> {
    serde_json::to_vec(settings).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression checklist E — the §5.4 default table, pinned value-by-value.
    #[test]
    fn defaults_match_spec_table() {
        let d = AppSettings::default();
        assert!(!d.trust_insecure_cert);
        assert!(d.auto_apply_server_changes);
        assert!(!d.auto_push_device_changes);
        assert!(d.prefetch_attachments);
        assert!(!d.prefetch_on_cellular);
        assert_eq!(d.payload_cache_max_bytes, 200 * 1024 * 1024);
        assert_eq!(d.appearance, AppearanceMode::System);
        assert!(d.keyboard_sound_feedback);
        assert!(d.keyboard_haptic_feedback);
        assert_eq!(d.log_view_level_filter, "info");
        assert!(d.auto_check_update);
    }

    /// Swift `SettingsStoreTests.test_loadAppSettings_whenEmptyDefaults...` +
    /// `...whenJSONIsCorrupt_returnsDefaults`.
    #[test]
    fn decode_empty_or_corrupt_returns_defaults() {
        assert_eq!(decode_app_settings(b""), AppSettings::default());
        assert_eq!(decode_app_settings(b"not json"), AppSettings::default());
        assert_eq!(decode_app_settings(b"{}"), AppSettings::default());
    }

    /// Swift `test_loadAppSettings_whenJSONIsPartial_missingKeysGetDefaults`.
    #[test]
    fn partial_json_fills_missing_with_defaults() {
        let loaded = decode_app_settings(br#"{ "trustInsecureCert": true }"#);
        let d = AppSettings::default();
        assert!(loaded.trust_insecure_cert, "present key preserved");
        assert_eq!(loaded.auto_check_update, d.auto_check_update);
        assert_eq!(
            loaded.manual_upload_dialog_shown,
            d.manual_upload_dialog_shown
        );
        assert_eq!(loaded.download_relative_path, d.download_relative_path);
        assert_eq!(loaded.log_view_level_filter, d.log_view_level_filter);
        assert_eq!(loaded.ignored_version, None);
        assert_eq!(loaded.prefetch_attachments, d.prefetch_attachments);
        assert_eq!(loaded.prefetch_on_cellular, d.prefetch_on_cellular);
        assert_eq!(loaded.payload_cache_max_bytes, d.payload_cache_max_bytes);
    }

    /// Swift `test_loadAppSettings_unknownKeysAreTolerated`.
    #[test]
    fn unknown_keys_are_tolerated() {
        let loaded = decode_app_settings(
            br#"{ "trustInsecureCert": true, "prefetchAttachments": false, "futureKnobFromTomorrow": 42 }"#,
        );
        assert!(loaded.trust_insecure_cert);
        assert!(!loaded.prefetch_attachments);
    }

    /// Spec §5.4 — an unknown `appearance` raw value falls back to System
    /// rather than failing the whole decode.
    #[test]
    fn unknown_appearance_falls_back_to_system() {
        let loaded = decode_app_settings(br#"{ "appearance": "midnight" }"#);
        assert_eq!(loaded.appearance, AppearanceMode::System);
        let dark = decode_app_settings(br#"{ "appearance": "dark" }"#);
        assert_eq!(dark.appearance, AppearanceMode::Dark);
    }

    #[test]
    fn round_trip_is_equal() {
        let original = AppSettings {
            trust_insecure_cert: true,
            auto_check_update: false,
            manual_upload_dialog_shown: true,
            download_relative_path: "Inbox".to_string(),
            log_view_level_filter: "warn".to_string(),
            ignored_version: Some("1.2.3".to_string()),
            appearance: AppearanceMode::Dark,
            payload_cache_max_bytes: 500 * 1024 * 1024,
            ..AppSettings::default()
        };
        let bytes = encode_app_settings(&original);
        assert_eq!(decode_app_settings(&bytes), original);
    }

    #[test]
    fn encode_uses_camelcase_keys_and_appearance_raw_string() {
        let json = String::from_utf8(encode_app_settings(&AppSettings::default())).unwrap();
        assert!(json.contains("\"trustInsecureCert\""));
        assert!(json.contains("\"payloadCacheMaxBytes\""));
        assert!(json.contains("\"appearance\":\"system\""));
    }

    /// Swift uses `encodeIfPresent` for `ignoredVersion`: omitted when nil.
    #[test]
    fn ignored_version_omitted_when_none() {
        let json = String::from_utf8(encode_app_settings(&AppSettings::default())).unwrap();
        assert!(!json.contains("ignoredVersion"));
        let with = AppSettings {
            ignored_version: Some("9.9.9".to_string()),
            ..AppSettings::default()
        };
        let json = String::from_utf8(encode_app_settings(&with)).unwrap();
        assert!(json.contains("\"ignoredVersion\":\"9.9.9\""));
    }
}
