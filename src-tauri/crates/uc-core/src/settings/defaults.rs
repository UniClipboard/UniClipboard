use std::collections::HashMap;
use std::time::Duration;

use super::model::*;

impl Default for UpdateChannel {
    /// Returns the default `UpdateChannel`, which is `Stable`.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::UpdateChannel;
    ///
    /// let channel = UpdateChannel::default();
    /// assert_eq!(channel, UpdateChannel::Stable);
    /// ```
    fn default() -> Self {
        UpdateChannel::Stable
    }
}

impl Default for GeneralSettings {
    /// Returns the default `GeneralSettings` used when no user preferences are configured.
    ///
    /// The defaults are:
    /// - `auto_start`: false
    /// - `silent_start`: false
    /// - `auto_check_update`: true
    /// - `theme`: `Theme::System`
    /// - `theme_color`: `None`
    /// - `device_name`: `None`
    /// - `language`: `None`
    /// - `update_channel`: `None` (auto-detect from version)
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::{GeneralSettings, Theme};
    ///
    /// let settings = GeneralSettings::default();
    /// assert_eq!(settings.auto_start, false);
    /// assert_eq!(settings.silent_start, false);
    /// assert_eq!(settings.auto_check_update, true);
    /// assert_eq!(settings.theme, Theme::System);
    /// assert!(settings.theme_color.is_none());
    /// assert!(settings.device_name.is_none());
    /// assert!(settings.language.is_none());
    /// assert!(settings.update_channel.is_none());
    /// ```
    fn default() -> Self {
        Self {
            auto_start: false,
            silent_start: false,
            auto_check_update: true,
            theme: Theme::System,
            theme_color: None,
            device_name: None,
            language: None,
            update_channel: None,
            telemetry_enabled: true,
        }
    }
}

impl Default for ContentTypes {
    /// Returns default `ContentTypes` with all fields set to `true`.
    ///
    /// New devices sync everything by default. Users can then disable
    /// specific content types per device.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::ContentTypes;
    ///
    /// let ct = ContentTypes::default();
    /// assert!(ct.text);
    /// assert!(ct.image);
    /// assert!(ct.link);
    /// assert!(ct.file);
    /// assert!(ct.code_snippet);
    /// assert!(ct.rich_text);
    /// ```
    fn default() -> Self {
        Self {
            text: true,
            image: true,
            link: true,
            file: true,
            code_snippet: true,
            rich_text: true,
        }
    }
}

impl Default for SyncSettings {
    /// Creates a `SyncSettings` populated with sensible defaults.
    ///
    /// The defaults enable automatic syncing, use realtime sync frequency, and include the
    /// default content types.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::{SyncSettings, SyncFrequency};
    ///
    /// let s = SyncSettings::default();
    /// assert!(s.auto_sync);
    /// assert_eq!(s.sync_frequency, SyncFrequency::Realtime);
    /// ```
    fn default() -> Self {
        Self {
            auto_sync: true,
            sync_frequency: SyncFrequency::Realtime,
            content_types: ContentTypes::default(),
        }
    }
}

impl Default for RetentionPolicy {
    /// Creates a `RetentionPolicy` populated with sensible defaults.
    ///
    /// The default policy is enabled, skips pinned items, evaluates rules using `AnyMatch`,
    /// and includes two rules: keep items younger than 30 days and keep up to 500 most recent items.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use uc_core::settings::model::{RetentionPolicy, RuleEvaluation, RetentionRule};
    ///
    /// let p = RetentionPolicy::default();
    /// assert!(p.enabled);
    /// assert!(p.skip_pinned);
    /// assert_eq!(p.evaluation, RuleEvaluation::AnyMatch);
    /// assert!(matches!(p.rules.get(0), Some(RetentionRule::ByAge { .. })));
    /// assert!(matches!(p.rules.get(1), Some(RetentionRule::ByCount { .. })));
    /// if let Some(RetentionRule::ByAge { max_age }) = p.rules.get(0) {
    ///     assert_eq!(*max_age, Duration::from_secs(60 * 60 * 24 * 30));
    /// }
    /// if let Some(RetentionRule::ByCount { max_items }) = p.rules.get(1) {
    ///     assert_eq!(*max_items, 500);
    /// }
    /// ```
    fn default() -> Self {
        Self {
            enabled: true,
            skip_pinned: true,
            evaluation: RuleEvaluation::AnyMatch,
            rules: vec![
                RetentionRule::ByAge {
                    max_age: Duration::from_secs(60 * 60 * 24 * 30), // 30 days
                },
                RetentionRule::ByCount { max_items: 500 },
            ],
        }
    }
}

impl Default for SecuritySettings {
    /// Creates default security settings with encryption disabled and no passphrase configured.
    ///
    /// The default has `encryption_enabled = false`, `passphrase_configured = false`,
    /// and `auto_unlock_enabled = false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::SecuritySettings;
    ///
    /// let s = SecuritySettings::default();
    /// assert!(!s.encryption_enabled);
    /// assert!(!s.passphrase_configured);
    /// assert!(!s.auto_unlock_enabled);
    /// ```
    fn default() -> Self {
        Self {
            encryption_enabled: false,
            passphrase_configured: false,
            auto_unlock_enabled: false,
        }
    }
}

impl Default for PairingSettings {
    /// Creates default pairing settings for handshake timers and retry behavior.
    ///
    /// Defaults are:
    /// - `step_timeout`: 30 seconds
    /// - `user_verification_timeout`: 120 seconds
    /// - `session_timeout`: 300 seconds
    /// - `max_retries`: 3
    /// - `protocol_version`: "1.0.0"
    fn default() -> Self {
        Self {
            step_timeout: Duration::from_secs(30),
            user_verification_timeout: Duration::from_secs(120),
            session_timeout: Duration::from_secs(300),
            max_retries: 3,
            protocol_version: "1.0.0".to_string(),
        }
    }
}

impl Default for FileSyncSettings {
    /// Returns default `FileSyncSettings` enabling file sync with sensible limits.
    ///
    /// Defaults:
    /// - `file_sync_enabled`: true
    /// - `small_file_threshold`: 10 MB (inline transfer threshold)
    /// - `max_file_size`: 5 GB
    /// - `file_cache_quota_per_device`: 500 MB
    /// - `file_retention_hours`: 24
    /// - `file_auto_cleanup`: true
    fn default() -> Self {
        Self {
            file_sync_enabled: true,
            small_file_threshold: 10 * 1024 * 1024, // 10 MB
            max_file_size: 5 * 1024 * 1024 * 1024,  // 5 GB
            file_cache_quota_per_device: 500 * 1024 * 1024, // 500 MB
            file_retention_hours: 24,
            file_auto_cleanup: true,
        }
    }
}

impl Default for NetworkSettings {
    /// Returns default `NetworkSettings` allowing iroh to fall back to public
    /// relays when direct connectivity fails (existing v0.6.x behavior).
    ///
    /// Defaults:
    /// - `allow_relay_fallback`: true
    ///
    // 默认 true = 允许 fallback。
    // 改成 false 会让所有跨网段老用户突然离线，属于 breaking change。
    // 修改默认值前请先 grep `LAN-only Mode` 文档与 changelog。
    fn default() -> Self {
        Self {
            allow_relay_fallback: true,
        }
    }
}

impl Default for Settings {
    /// Constructs a Settings instance populated with the current schema version and sensible nested defaults.
    ///
    /// The created `Settings` uses `CURRENT_SCHEMA_VERSION` for `schema_version` and the `Default` implementations
    /// of the nested settings types for `general`, `sync`, `retention_policy`, `security`, and `pairing`.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::{Settings, CURRENT_SCHEMA_VERSION};
    ///
    /// let settings = Settings::default();
    /// assert_eq!(settings.schema_version, CURRENT_SCHEMA_VERSION);
    /// // Nested defaults are available:
    /// let _ = settings.general;
    /// let _ = settings.sync;
    /// let _ = settings.retention_policy;
    /// let _ = settings.security;
    /// let _ = settings.pairing;
    /// ```
    ///
    /// # Returns
    ///
    /// `Settings` initialized with `CURRENT_SCHEMA_VERSION` and default values for `general`, `sync`, `retention_policy`, `security`, and `pairing`.
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            general: GeneralSettings::default(),
            sync: SyncSettings::default(),
            retention_policy: RetentionPolicy::default(),
            security: SecuritySettings::default(),
            pairing: PairingSettings::default(),
            keyboard_shortcuts: HashMap::new(),
            file_sync: FileSyncSettings::default(),
            network: NetworkSettings::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::model::{NetworkSettings, Settings, CURRENT_SCHEMA_VERSION};

    /// Pitfall 2 防御：默认值必须为 true（允许 fallback），保护老用户
    /// 跨网段同步行为。改 false = breaking change。
    #[test]
    fn network_settings_default_allows_relay_fallback() {
        let n = NetworkSettings::default();
        assert!(
            n.allow_relay_fallback,
            "NetworkSettings::default().allow_relay_fallback MUST be true (Pitfall 2)"
        );
    }

    /// Pitfall 2 防御：顶层 Settings::default 把 network 装配进去，
    /// 字段值与子结构 default 保持一致。
    #[test]
    fn settings_default_includes_network_with_fallback_allowed() {
        let s = Settings::default();
        assert!(s.network.allow_relay_fallback);
        assert_eq!(s.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(
            s.schema_version, 1,
            "schema_version MUST stay 1 (no migration)"
        );
    }

    /// NETSET-02 success criterion #2：缺 `network` 段的旧 settings.json
    /// 反序列化必须回填默认值 true。
    #[test]
    fn old_settings_json_without_network_section_falls_back_to_default() {
        // 模拟 v0.6.x 时代写出的 settings.json 片段（无 network 字段）
        let json = r#"{}"#;
        let s: Settings = serde_json::from_str(json).expect("parse minimal settings");
        assert!(
            s.network.allow_relay_fallback,
            "missing network section MUST default to true"
        );
        assert_eq!(s.schema_version, 1);
    }

    /// 显式 false 的 JSON 必须保留 false 语义（确认未误取反）。
    #[test]
    fn explicit_allow_relay_fallback_false_is_preserved() {
        let json = r#"{ "network": { "allow_relay_fallback": false } }"#;
        let s: Settings = serde_json::from_str(json).expect("parse explicit false");
        assert!(!s.network.allow_relay_fallback);
    }

    /// 反向断言：显式 true 也保留 true 语义。
    #[test]
    fn explicit_allow_relay_fallback_true_is_preserved() {
        let json = r#"{ "network": { "allow_relay_fallback": true } }"#;
        let s: Settings = serde_json::from_str(json).expect("parse explicit true");
        assert!(s.network.allow_relay_fallback);
    }
}
