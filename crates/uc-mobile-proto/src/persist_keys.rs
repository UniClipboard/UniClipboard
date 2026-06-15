//! Persistence key + file names shared across platforms (spec §5.5).
//!
//! Single source of truth for the `UserDefaults` keys and the file-backed App
//! Group state names, so iOS, future Android, and any other consumer of this
//! crate agree on where each blob lives. Port of Swift
//! `AppSettings.PersistenceKey` + the file-name constants in `SettingsStore`.
//!
//! The native layer still performs the actual I/O (reads/writes these keys);
//! this module only pins the names so they never drift between platforms.

/// `UserDefaults` keys (also reused inside the App Group suite).
pub mod keys {
    /// The multi-server collection (`ServerConfigList`).
    pub const SERVER_CONFIG_LIST: &str = "server_config_list";
    /// User-tunable `AppSettings`.
    pub const APP_SETTINGS: &str = "app_settings";
    /// Pre-multi-URL single config, migrated away from on first read (§5.5).
    pub const LEGACY_SERVER_CONFIG: &str = "server_config";
    /// Legacy `UserDefaults` home of the synced-content hash, migrated to a
    /// file backend on first read.
    pub const LAST_SYNCED_CONTENT_HASH: &str = "last_synced_content_hash";
    /// The local clipboard observation log (`[ClipboardHistoryItem]`).
    pub const CLIPBOARD_HISTORY: &str = "clipboard_history";
    /// §2.7 incremental-sync watermark (highest `lastModified` seen).
    pub const HISTORY_MODIFIED_AFTER: &str = "history_modified_after";
    /// §2.7 throttle timestamp (when the last history pull finished).
    pub const LAST_HISTORY_SYNC_AT: &str = "last_history_sync_at";
    /// The `UIPasteboard.changeCount` the keyboard last synced.
    pub const LAST_SYNCED_CHANGE_COUNT: &str = "last_synced_change_count";
}

/// File-backed App Group state names (file-backed, NOT `UserDefaults`, for
/// cross-process freshness — `cfprefsd` caches the suite per-process).
pub mod files {
    /// The synced-content hash (plain text, uppercase hex SHA-256).
    pub const LAST_SYNCED_HASH: &str = "last_synced_hash";
    /// The last observed normalized Wi-Fi SSID (plain text).
    pub const LAST_KNOWN_SSID: &str = "last_known_ssid";
    /// The §5.3 probe-confirmed URL per profile (JSON `{configId: url}`).
    pub const LIVE_URLS: &str = "live_urls";
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the exact wire strings against Swift `AppSettings.PersistenceKey` /
    /// `SettingsStore` file names — these are a cross-platform contract.
    #[test]
    fn key_names_match_swift() {
        assert_eq!(keys::SERVER_CONFIG_LIST, "server_config_list");
        assert_eq!(keys::APP_SETTINGS, "app_settings");
        assert_eq!(keys::LEGACY_SERVER_CONFIG, "server_config");
        assert_eq!(keys::LAST_SYNCED_CONTENT_HASH, "last_synced_content_hash");
        assert_eq!(keys::CLIPBOARD_HISTORY, "clipboard_history");
        assert_eq!(keys::HISTORY_MODIFIED_AFTER, "history_modified_after");
        assert_eq!(keys::LAST_HISTORY_SYNC_AT, "last_history_sync_at");
        assert_eq!(keys::LAST_SYNCED_CHANGE_COUNT, "last_synced_change_count");
        assert_eq!(files::LAST_SYNCED_HASH, "last_synced_hash");
        assert_eq!(files::LAST_KNOWN_SSID, "last_known_ssid");
        assert_eq!(files::LIVE_URLS, "live_urls");
    }
}
